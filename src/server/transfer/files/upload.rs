use crate::error::{Result, ServerError, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    TransferComplete, TransferFailure, TransferFailureCode, TransferFrame, TransferReady,
    read_next_frame, write_put_complete, write_put_ready, write_transfer_error,
};
use tokio::io::AsyncWriteExt;

use crate::server::transfer::ShellContext;
use crate::server::transfer::helpers::{
    PreparedPutDestination, atomic_rename_failure, prepare_put_destination, spawn_upload_helper,
    target_exists_failure,
};

pub(crate) async fn handle_put_request(
    stream: &mut IrohDuplex,
    request: crate::transport::transfer::PutRequest,
    context: ShellContext,
) -> Result<()> {
    if request.recursive {
        return handle_recursive_put_request(stream, request, context).await;
    }

    let prepared = match prepare_put_destination(context, &request.path).await? {
        Some(prepared) => prepared,
        None => {
            let dest_path = context.resolve_path(&request.path).await?;
            write_transfer_error(stream, &target_exists_failure(&dest_path))
                .await
                .map_err(TransportError::from)?;
            return Ok(());
        }
    };
    let PreparedPutDestination {
        final_arg,
        part_arg,
    } = prepared;

    let mut child = spawn_upload_helper(context, &part_arg).await?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| ServerError::TransferFailed {
            details: "upload helper stdin unavailable".to_string(),
        })?;

    write_put_ready(
        stream,
        &TransferReady {
            size: request.size,
            mode: request.mode,
        },
    )
    .await
    .map_err(TransportError::from)?;

    let mut received = 0u64;
    let mut transfer_failed = false;
    loop {
        match read_next_frame(stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::PutChunk(chunk) => {
                received += chunk.len() as u64;
                if let Err(err) = stdin.write_all(&chunk).await {
                    tracing::warn!("Failed to write to upload helper: {}", err);
                    transfer_failed = true;
                    break;
                }
            }
            TransferFrame::PutComplete(complete) => {
                if complete.size != received {
                    write_transfer_error(
                        stream,
                        &TransferFailure::new(
                            TransferFailureCode::SizeMismatch,
                            format!("received {}, client reported {}", received, complete.size),
                        ),
                    )
                    .await
                    .map_err(TransportError::from)?;
                    transfer_failed = true;
                }
                break;
            }
            TransferFrame::Error(_) => {
                transfer_failed = true;
                break;
            }
            other => {
                let _ = write_transfer_error(
                    stream,
                    &TransferFailure::new(
                        TransferFailureCode::UnexpectedFrame,
                        format!("{other:?}"),
                    ),
                )
                .await;
                transfer_failed = true;
                break;
            }
        }
    }

    let _ = stdin.flush().await;
    drop(stdin);

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| ServerError::TransferFailed {
            details: format!("waiting for upload helper failed: {e}"),
        })?;

    if transfer_failed || !output.status.success() {
        context.remove_file_if_present(&part_arg).await;

        if !output.status.success() && !transfer_failed {
            write_transfer_error(
                stream,
                &TransferFailure::new(
                    TransferFailureCode::HelperFailed,
                    String::from_utf8_lossy(&output.stderr).trim().to_string(),
                ),
            )
            .await
            .map_err(TransportError::from)?;
        }
        return Ok(());
    }

    if !context.rename(&part_arg, &final_arg).await? {
        write_transfer_error(stream, &atomic_rename_failure(&final_arg))
            .await
            .map_err(TransportError::from)?;
        return Ok(());
    }

    if let Some(mode) = request.mode {
        context.chmod(&final_arg, mode).await;
    }

    write_put_complete(stream, &TransferComplete { size: received })
        .await
        .map_err(TransportError::from)?;
    Ok(())
}

async fn handle_recursive_put_request(
    stream: &mut IrohDuplex,
    request: crate::transport::transfer::PutRequest,
    context: ShellContext,
) -> Result<()> {
    let dest_root = context.resolve_path(&request.path).await?;
    context.create_dir_all(&dest_root).await?;

    write_put_ready(
        stream,
        &TransferReady {
            size: 0,
            mode: None,
        },
    )
    .await
    .map_err(TransportError::from)?;

    let mut total_received = 0u64;
    loop {
        match read_next_frame(stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::NewEntry(header) => {
                let full_path = dest_root.join(&header.path);
                let full_path_str = full_path.display().to_string();

                if header.is_dir {
                    context.create_dir_all(&full_path).await?;
                    if let Some(mode) = header.mode {
                        context.chmod(&full_path_str, mode).await;
                    }
                } else {
                    // Use atomic rename pattern for each file in the recursive stream
                    let prepared = match prepare_put_destination(context, &full_path_str).await? {
                        Some(p) => p,
                        None => {
                            write_transfer_error(stream, &target_exists_failure(&full_path))
                                .await
                                .map_err(TransportError::from)?;
                            return Ok(()); // Fail whole recursive transfer on collision
                        }
                    };

                    let mut child = spawn_upload_helper(context, &prepared.part_arg).await?;
                    let mut stdin =
                        child
                            .stdin
                            .take()
                            .ok_or_else(|| ServerError::TransferFailed {
                                details: "upload helper stdin unavailable".to_string(),
                            })?;

                    let mut file_received = 0u64;
                    let mut entry_failed = false;
                    loop {
                        match read_next_frame(stream)
                            .await
                            .map_err(TransportError::from)?
                        {
                            TransferFrame::PutChunk(chunk) => {
                                file_received += chunk.len() as u64;
                                if let Err(e) = stdin.write_all(&chunk).await {
                                    tracing::warn!("Failed to write to upload helper: {}", e);
                                    entry_failed = true;
                                    break;
                                }
                            }
                            TransferFrame::EntryComplete(_) => break,
                            other => {
                                return Err(ServerError::TransferFailed {
                                    details: format!(
                                        "unexpected frame during recursive entry stream: {other:?}"
                                    ),
                                }
                                .into());
                            }
                        }
                    }
                    drop(stdin);
                    let output = child.wait_with_output().await.map_err(|e| {
                        ServerError::TransferFailed {
                            details: format!("waiting for upload helper failed: {e}"),
                        }
                    })?;

                    if entry_failed || !output.status.success() {
                        context.remove_file_if_present(&prepared.part_arg).await;
                        return Err(ServerError::TransferFailed {
                            details: "recursive entry upload failed".to_string(),
                        }
                        .into());
                    }

                    // Perform atomic rename
                    if !context
                        .rename(&prepared.part_arg, &prepared.final_arg)
                        .await?
                    {
                        return Err(ServerError::TransferFailed {
                            details: format!("atomic rename failed for {}", prepared.final_arg),
                        }
                        .into());
                    }

                    if let Some(mode) = header.mode {
                        context.chmod(&prepared.final_arg, mode).await;
                    }
                    total_received += file_received;
                }
            }
            TransferFrame::PutComplete(complete) => {
                write_put_complete(
                    stream,
                    &TransferComplete {
                        size: total_received,
                    },
                )
                .await
                .map_err(TransportError::from)?;
                let _ = complete;
                return Ok(());
            }
            TransferFrame::Error(e) => {
                return Err(ServerError::TransferFailed {
                    details: e.to_string(),
                }
                .into());
            }
            other => {
                return Err(ServerError::TransferFailed {
                    details: format!("unexpected frame during recursive upload: {other:?}"),
                }
                .into());
            }
        }
    }
}

//! Server file upload.
use crate::error::{Result, ServerError, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    TransferComplete, TransferFailure, TransferFailureCode, TransferFrame, TransferFrameBorrowed,
    TransferReady, read_next_frame_into, write_put_complete, write_put_ready, write_transfer_error,
};
use tokio::io::AsyncWriteExt;

use crate::server::transfer::ShellContext;
use crate::server::transfer::helpers::{
    PreparedPutDestination, atomic_rename_failure, prepare_put_destination, spawn_upload_helper,
    target_exists_failure,
};

#[tracing::instrument(skip(stream, context, shell_state))]
pub(crate) async fn handle_put_request(
    stream: &mut IrohDuplex,
    request: crate::transport::transfer::PutRequest,
    context: ShellContext,
    shell_state: &super::super::ConnectionShellState,
) -> Result<()> {
    if request.recursive {
        return handle_recursive_put_request(stream, request, context, shell_state).await;
    }

    let Some(prepared) = prepare_put_destination(context, &request.path, shell_state).await? else {
        let dest_path = context.resolve_path(&request.path, shell_state).await?;
        write_transfer_error(stream, &target_exists_failure(&dest_path))
            .await
            .map_err(TransportError::from)?;
        return Ok(());
    };
    let PreparedPutDestination {
        final_arg,
        part_arg,
    } = prepared;

    let mut sink = spawn_upload_helper(context, &part_arg).await?;
    let mut transfer_failed = false;
    let mut received = 0u64;
    {
        let Some(mut stdin) = sink.stdin() else {
            let _ = context.remove_file_if_present(&part_arg).await;
            return Err(ServerError::TransferFailed {
                failure: TransferFailure::new(
                    TransferFailureCode::Internal,
                    "upload helper sink unavailable",
                ),
            }
            .into());
        };

        if let Err(err) = write_put_ready(
            stream,
            &TransferReady {
                size: request.size,
                mode: request.mode,
            },
        )
        .await
        {
            let _ = context.remove_file_if_present(&part_arg).await;
            return Err(TransportError::from(err).into());
        }

        let mut frame_buf = Vec::new();
        loop {
            let frame = match read_next_frame_into(stream, &mut frame_buf).await {
                Ok(frame) => frame,
                Err(err) => {
                    tracing::warn!("Upload stream read failed: {err}");
                    transfer_failed = true;
                    break;
                }
            };
            match frame {
                TransferFrameBorrowed::PutChunk(chunk) => {
                    received += chunk.len() as u64;
                    if received > request.size {
                        write_transfer_error(
                            stream,
                            &TransferFailure::new(
                                TransferFailureCode::SizeMismatch,
                                format!(
                                    "received {received} bytes, exceeding the declared size of {}",
                                    request.size
                                ),
                            ),
                        )
                        .await
                        .map_err(TransportError::from)?;
                        transfer_failed = true;
                        break;
                    }
                    if let Err(err) = stdin.write_all(chunk).await {
                        tracing::warn!("Failed to write to upload helper: {}", err);
                        transfer_failed = true;
                        break;
                    }
                }
                TransferFrameBorrowed::Owned(TransferFrame::PutComplete(complete)) => {
                    if complete.size != received || received != request.size {
                        write_transfer_error(
                            stream,
                            &TransferFailure::new(
                                TransferFailureCode::SizeMismatch,
                                format!(
                                    "received {received} bytes, declared {}, client reported {}",
                                    request.size, complete.size
                                ),
                            ),
                        )
                        .await
                        .map_err(TransportError::from)?;
                        transfer_failed = true;
                    }
                    break;
                }
                TransferFrameBorrowed::Owned(TransferFrame::Error(_)) => {
                    transfer_failed = true;
                    break;
                }
                TransferFrameBorrowed::GetChunk(_) => {
                    let _ = write_transfer_error(
                        stream,
                        &TransferFailure::new(
                            TransferFailureCode::UnexpectedFrame,
                            "unexpected get chunk frame during upload".to_string(),
                        ),
                    )
                    .await;
                    transfer_failed = true;
                    break;
                }
                TransferFrameBorrowed::Owned(other) => {
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
    }

    let helper_res = sink.wait().await;

    if transfer_failed || helper_res.is_err() {
        let _ = context.remove_file_if_present(&part_arg).await;

        if let Err(err) = helper_res {
            if !transfer_failed {
                write_transfer_error(
                    stream,
                    &TransferFailure::new(TransferFailureCode::HelperFailed, err.to_string()),
                )
                .await
                .map_err(TransportError::from)?;
            }
        }
        return Ok(());
    }

    match context.rename(&part_arg, &final_arg).await {
        Ok(true) => {}
        Ok(false) => {
            let _ = context.remove_file_if_present(&part_arg).await;
            write_transfer_error(stream, &atomic_rename_failure(&final_arg))
                .await
                .map_err(TransportError::from)?;
            return Ok(());
        }
        Err(err) => {
            let _ = context.remove_file_if_present(&part_arg).await;
            return Err(err);
        }
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
    shell_state: &super::super::ConnectionShellState,
) -> Result<()> {
    let dest_root = context.resolve_path(&request.path, shell_state).await?;
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
    let mut frame_buf = Vec::new();
    loop {
        match read_next_frame_into(stream, &mut frame_buf)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrameBorrowed::Owned(TransferFrame::NewEntry(header)) => {
                let entry_path = crate::server::transfer::state::sanitize_relative_path(
                    &header.path,
                )
                .map_err(|reason| {
                    crate::error::IroshError::from(ServerError::TransferFailed {
                        failure: TransferFailure::new(
                            TransferFailureCode::PathInvalid,
                            format!("unsafe entry path '{}': {reason}", header.path),
                        ),
                    })
                })?;
                let full_path = dest_root.join(entry_path);
                let full_path_str = full_path.display().to_string();

                if header.is_dir {
                    context.create_dir_all(&full_path).await?;
                    if let Some(mode) = header.mode {
                        context.chmod(&full_path_str, mode).await;
                    }
                } else {
                    // Use atomic rename pattern for each file in the recursive stream
                    let Some(prepared) =
                        prepare_put_destination(context, &full_path_str, shell_state).await?
                    else {
                        write_transfer_error(stream, &target_exists_failure(&full_path))
                            .await
                            .map_err(TransportError::from)?;
                        return Ok(()); // Fail whole recursive transfer on collision
                    };

                    let mut sink = spawn_upload_helper(context, &prepared.part_arg).await?;
                    let mut entry_failed = false;
                    let mut size_mismatch = false;
                    let mut file_received = 0u64;
                    {
                        let mut stdin =
                            sink.stdin().ok_or_else(|| ServerError::TransferFailed {
                                failure: TransferFailure::new(
                                    TransferFailureCode::Internal,
                                    "upload helper sink unavailable",
                                ),
                            })?;
                        loop {
                            let frame = match read_next_frame_into(stream, &mut frame_buf).await {
                                Ok(frame) => frame,
                                Err(e) => {
                                    tracing::warn!(
                                        "Recursive upload stream read failed for entry: {e}"
                                    );
                                    entry_failed = true;
                                    break;
                                }
                            };
                            match frame {
                                TransferFrameBorrowed::PutChunk(chunk) => {
                                    file_received += chunk.len() as u64;
                                    if file_received > header.size {
                                        size_mismatch = true;
                                        entry_failed = true;
                                        break;
                                    }
                                    if let Err(e) = stdin.write_all(chunk).await {
                                        tracing::warn!("Failed to write to upload helper: {}", e);
                                        entry_failed = true;
                                        break;
                                    }
                                }
                                TransferFrameBorrowed::Owned(TransferFrame::EntryComplete(_)) => {
                                    if file_received != header.size {
                                        size_mismatch = true;
                                        entry_failed = true;
                                    }
                                    break;
                                }
                                TransferFrameBorrowed::GetChunk(_) => {
                                    tracing::warn!(
                                        "unexpected get chunk frame during recursive upload"
                                    );
                                    entry_failed = true;
                                    break;
                                }
                                TransferFrameBorrowed::Owned(other) => {
                                    tracing::warn!(
                                        "unexpected frame during recursive entry stream: {other:?}"
                                    );
                                    entry_failed = true;
                                    break;
                                }
                            }
                        }
                        let _ = stdin.flush().await;
                    }
                    let helper_res = sink.wait().await;

                    if size_mismatch {
                        let _ = context.remove_file_if_present(&prepared.part_arg).await;
                        write_transfer_error(
                            stream,
                            &TransferFailure::new(
                                TransferFailureCode::SizeMismatch,
                                format!(
                                    "entry '{}' declared {} bytes, received {file_received}",
                                    header.path, header.size
                                ),
                            ),
                        )
                        .await
                        .map_err(TransportError::from)?;
                        return Ok(());
                    }

                    if entry_failed || helper_res.is_err() {
                        let _ = context.remove_file_if_present(&prepared.part_arg).await;
                        return Err(ServerError::TransferFailed {
                            failure: TransferFailure::new(
                                TransferFailureCode::HelperFailed,
                                format!(
                                    "recursive entry upload failed: {}",
                                    helper_res.err().map(|e| e.to_string()).unwrap_or_default()
                                ),
                            ),
                        }
                        .into());
                    }

                    // Perform atomic rename
                    if !context
                        .rename(&prepared.part_arg, &prepared.final_arg)
                        .await?
                    {
                        return Err(ServerError::TransferFailed {
                            failure: TransferFailure::new(
                                TransferFailureCode::AtomicRenameFailed,
                                format!("atomic rename failed for {}", prepared.final_arg),
                            ),
                        }
                        .into());
                    }

                    if let Some(mode) = header.mode {
                        context.chmod(&prepared.final_arg, mode).await;
                    }
                    total_received += file_received;
                }
            }
            TransferFrameBorrowed::Owned(TransferFrame::PutComplete(complete)) => {
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
            TransferFrameBorrowed::Owned(TransferFrame::Error(e)) => {
                return Err(ServerError::TransferFailed { failure: e }.into());
            }
            TransferFrameBorrowed::PutChunk(_) | TransferFrameBorrowed::GetChunk(_) => {
                return Err(ServerError::TransferFailed {
                    failure: TransferFailure::new(
                        TransferFailureCode::UnexpectedFrame,
                        "unexpected chunk frame at recursive upload top level".to_string(),
                    ),
                }
                .into());
            }
            TransferFrameBorrowed::Owned(other) => {
                return Err(ServerError::TransferFailed {
                    failure: TransferFailure::new(
                        TransferFailureCode::UnexpectedFrame,
                        format!("unexpected frame during recursive upload: {other:?}"),
                    ),
                }
                .into());
            }
        }
    }
}

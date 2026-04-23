use crate::error::{Result, ServerError, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    TransferComplete, TransferFailure, TransferFailureCode, TransferFrame, TransferReady,
    read_next_frame, write_put_complete, write_put_ready, write_transfer_error,
};
use tokio::io::AsyncWriteExt;

use crate::server::transfer::helpers::{
    PreparedPutDestination, atomic_rename_failure, prepare_put_destination, spawn_upload_helper,
    target_exists_failure,
};
use crate::server::transfer::{ShellContext, resolve_remote_path};

pub(crate) async fn handle_put_request(
    stream: &mut IrohDuplex,
    request: crate::transport::transfer::PutRequest,
    context: ShellContext,
) -> Result<()> {
    let prepared = match prepare_put_destination(context, &request.path).await? {
        Some(prepared) => prepared,
        None => {
            let dest_path = resolve_remote_path(&request.path)?;
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

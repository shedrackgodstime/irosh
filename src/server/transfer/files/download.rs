use crate::error::{Result, ServerError, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    MAX_CHUNK_BYTES, TransferComplete, TransferFailure, TransferFailureCode, TransferReady,
    write_get_chunk, write_get_complete, write_get_ready, write_transfer_error,
};
use tokio::io::AsyncReadExt;

use crate::server::transfer::ShellContext;
use crate::server::transfer::helpers::{probe_download_size, spawn_download_helper};

pub(crate) async fn handle_get_request(
    stream: &mut IrohDuplex,
    request: crate::transport::transfer::GetRequest,
    context: ShellContext,
) -> Result<()> {
    if request.recursive {
        return handle_recursive_get_request(stream, request, context).await;
    }

    let source_path = context.resolve_path(&request.path).await?;
    let expected_size = match probe_download_size(context, &source_path).await? {
        Ok(size) => size,
        Err(failure) => {
            write_transfer_error(stream, &failure)
                .await
                .map_err(TransportError::from)?;
            return Ok(());
        }
    };

    let (mut child, helper_source) = spawn_download_helper(context, &source_path).await?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| ServerError::TransferFailed {
            details: "stdout pipe unavailable".to_string(),
        })?;

    write_get_ready(
        stream,
        &TransferReady {
            size: expected_size,
            mode: None,
        },
    )
    .await
    .map_err(TransportError::from)?;

    let mut buffer = vec![0u8; MAX_CHUNK_BYTES];
    loop {
        let count = stdout
            .read(&mut buffer)
            .await
            .map_err(|e| ServerError::TransferFailed {
                details: format!("reading download helper stdout failed: {e}"),
            })?;
        if count == 0 {
            break;
        }
        write_get_chunk(stream, &buffer[..count])
            .await
            .map_err(TransportError::from)?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| ServerError::TransferFailed {
            details: format!("waiting for download helper failed: {e}"),
        })?;
    if !output.status.success() {
        write_transfer_error(
            stream,
            &TransferFailure::new(
                TransferFailureCode::HelperFailed,
                format!(
                    "{}; context={:?}; requested={}; helper_arg={}",
                    String::from_utf8_lossy(&output.stderr).trim(),
                    context,
                    source_path.display(),
                    helper_source
                ),
            ),
        )
        .await
        .map_err(TransportError::from)?;
        return Ok(());
    }

    write_get_complete(
        stream,
        &TransferComplete {
            size: expected_size,
        },
    )
    .await
    .map_err(TransportError::from)?;
    Ok(())
}

async fn handle_recursive_get_request(
    stream: &mut IrohDuplex,
    request: crate::transport::transfer::GetRequest,
    context: ShellContext,
) -> Result<()> {
    let source_root = context.resolve_path(&request.path).await?;

    // Preflight: list and calculate total size (optional, but good for TransferReady)
    // For now, we'll walk and stream.
    write_get_ready(
        stream,
        &TransferReady {
            size: 0,
            mode: None,
        },
    )
    .await
    .map_err(TransportError::from)?;

    let mut total_sent = 0u64;

    // Use walkdir to list remote files.
    // Note: This walkdir runs in the server's namespace.
    // If context is Live, we might need a different approach if we want to walk
    // within the target namespace.
    let walk = walkdir::WalkDir::new(&source_root);
    for entry in walk {
        let entry = entry.map_err(|e| ServerError::TransferFailed {
            details: format!("failed to walk remote directory: {e}"),
        })?;

        let relative =
            entry
                .path()
                .strip_prefix(&source_root)
                .map_err(|_| ServerError::TransferFailed {
                    details: "failed to resolve relative path during remote walk".to_string(),
                })?;

        if relative.as_os_str().is_empty() {
            continue;
        }

        let is_dir = entry.file_type().is_dir();
        let metadata = entry.metadata().map_err(|e| ServerError::TransferFailed {
            details: format!("failed to read remote metadata: {e}"),
        })?;

        let size = if is_dir { 0 } else { metadata.len() };
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            Some(metadata.permissions().mode() & 0o777)
        };
        #[cfg(not(unix))]
        let mode = None;

        crate::transport::transfer::write_new_entry(
            stream,
            &crate::transport::transfer::EntryHeader {
                path: relative.display().to_string(),
                size,
                mode,
                is_dir,
            },
        )
        .await
        .map_err(TransportError::from)?;

        if !is_dir {
            let (mut child, _) = spawn_download_helper(context, entry.path()).await?;
            let mut stdout = child
                .stdout
                .take()
                .ok_or_else(|| ServerError::TransferFailed {
                    details: "stdout pipe unavailable".to_string(),
                })?;

            let mut buffer = vec![0u8; MAX_CHUNK_BYTES];
            loop {
                let count =
                    stdout
                        .read(&mut buffer)
                        .await
                        .map_err(|e| ServerError::TransferFailed {
                            details: format!("reading download helper stdout failed: {e}"),
                        })?;
                if count == 0 {
                    break;
                }
                write_get_chunk(stream, &buffer[..count])
                    .await
                    .map_err(TransportError::from)?;
                total_sent += count as u64;
            }
            let _ = child.wait().await;

            crate::transport::transfer::write_entry_complete(
                stream,
                &crate::transport::transfer::EntryComplete,
            )
            .await
            .map_err(TransportError::from)?;
        }
    }

    write_get_complete(stream, &TransferComplete { size: total_sent })
        .await
        .map_err(TransportError::from)?;

    Ok(())
}

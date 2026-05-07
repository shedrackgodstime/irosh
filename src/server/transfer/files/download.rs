use crate::error::{Result, ServerError, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    MAX_CHUNK_BYTES, TransferComplete, TransferFailure, TransferFailureCode, TransferReady,
    write_get_chunk, write_get_complete, write_get_ready, write_transfer_error,
};
use tokio::io::AsyncReadExt;

use crate::server::transfer::ShellContext;
use crate::server::transfer::helpers::{
    DownloadSource, probe_download_size, spawn_download_helper,
};

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

    let (mut source, helper_source) = spawn_download_helper(context, &source_path).await?;
    {
        let mut stdout = source.stdout().ok_or_else(|| ServerError::TransferFailed {
            details: "download source pipe unavailable".to_string(),
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
            let count =
                stdout
                    .read(&mut buffer)
                    .await
                    .map_err(|e| ServerError::TransferFailed {
                        details: format!("reading download source failed: {e}"),
                    })?;
            if count == 0 {
                break;
            }
            write_get_chunk(stream, &buffer[..count])
                .await
                .map_err(TransportError::from)?;
        }
    }

    if let DownloadSource::Process(child) = source {
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

    #[allow(unused_mut)]
    let mut use_native_walk = true;
    #[cfg(target_os = "linux")]
    if let ShellContext::Live { .. } = context {
        use_native_walk = false;
    }

    if use_native_walk {
        let walk = walkdir::WalkDir::new(&source_root);
        for entry in walk {
            let entry = entry.map_err(|e| ServerError::TransferFailed {
                details: format!("failed to walk remote directory: {e}"),
            })?;

            let relative = entry.path().strip_prefix(&source_root).map_err(|_| {
                ServerError::TransferFailed {
                    details: "failed to resolve relative path during remote walk".to_string(),
                }
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
                stream_file_content(stream, context, entry.path(), &mut total_sent).await?;
            }
        }
    } else {
        #[cfg(target_os = "linux")]
        {
            // In a Live context on Linux, we MUST use an external 'find' command to see the
            // filesystem from the perspective of the target namespace.
            // We use null terminators to handle filenames with spaces, colons, or newlines.
            let mut find_cmd = tokio::process::Command::new("sh");
            find_cmd
                .arg("-c")
                .arg("find . -mindepth 1 -printf '%y\\0%P\\0%s\\0%m\\0'")
                .current_dir(&source_root)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            context.configure(&mut find_cmd);

            let mut child = find_cmd.spawn().map_err(|e| ServerError::TransferFailed {
                details: format!("failed to spawn find for recursive walk: {e}"),
            })?;

            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| ServerError::TransferFailed {
                    details: "find stdout unavailable".to_string(),
                })?;

            use tokio::io::{AsyncBufReadExt, BufReader};
            let mut reader = BufReader::new(stdout);

            loop {
                let mut type_buf = Vec::new();
                if reader.read_until(0, &mut type_buf).await? == 0 {
                    break;
                }
                let mut path_buf = Vec::new();
                reader.read_until(0, &mut path_buf).await?;
                let mut size_buf = Vec::new();
                reader.read_until(0, &mut size_buf).await?;
                let mut mode_buf = Vec::new();
                reader.read_until(0, &mut mode_buf).await?;

                // Strip the trailing null bytes and convert to strings
                let entry_type =
                    String::from_utf8_lossy(&type_buf[..type_buf.len().saturating_sub(1)]);
                let relative_path =
                    String::from_utf8_lossy(&path_buf[..path_buf.len().saturating_sub(1)]);
                let size_str =
                    String::from_utf8_lossy(&size_buf[..size_buf.len().saturating_sub(1)]);
                let mode_str =
                    String::from_utf8_lossy(&mode_buf[..mode_buf.len().saturating_sub(1)]);

                let is_dir = entry_type == "d";
                let size = size_str.parse::<u64>().unwrap_or(0);
                let mode = u32::from_str_radix(&mode_str, 8).ok();

                crate::transport::transfer::write_new_entry(
                    stream,
                    &crate::transport::transfer::EntryHeader {
                        path: relative_path.to_string(),
                        size,
                        mode,
                        is_dir,
                    },
                )
                .await
                .map_err(TransportError::from)?;

                if !is_dir {
                    let full_path = source_root.join(relative_path.as_ref());
                    stream_file_content(stream, context, &full_path, &mut total_sent).await?;
                }
            }

            let _ = child.wait().await;
        }
    }

    write_get_complete(stream, &TransferComplete { size: total_sent })
        .await
        .map_err(TransportError::from)?;

    Ok(())
}

async fn stream_file_content(
    stream: &mut IrohDuplex,
    context: ShellContext,
    path: &std::path::Path,
    total_sent: &mut u64,
) -> Result<()> {
    let (mut source, _) = spawn_download_helper(context, path).await?;
    let mut stdout = source.stdout().ok_or_else(|| ServerError::TransferFailed {
        details: "download source pipe unavailable".to_string(),
    })?;

    let mut buffer = vec![0u8; MAX_CHUNK_BYTES];
    loop {
        let count = stdout
            .read(&mut buffer)
            .await
            .map_err(|e| ServerError::TransferFailed {
                details: format!("reading download source failed: {e}"),
            })?;
        if count == 0 {
            break;
        }
        write_get_chunk(stream, &buffer[..count])
            .await
            .map_err(TransportError::from)?;
        *total_sent += count as u64;
    }

    crate::transport::transfer::write_entry_complete(
        stream,
        &crate::transport::transfer::EntryComplete,
    )
    .await
    .map_err(TransportError::from)?;

    Ok(())
}

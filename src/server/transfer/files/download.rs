use crate::error::{Result, ServerError, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    MAX_CHUNK_BYTES, TransferComplete, TransferFailure, TransferFailureCode, TransferReady,
    write_get_chunk, write_get_complete, write_get_ready, write_transfer_error,
};
use tokio::io::AsyncReadExt;

use crate::server::transfer::helpers::{probe_download_size, spawn_download_helper};
use crate::server::transfer::{ConnectionShellState, LiveShellContext, resolve_remote_path};

pub(crate) async fn handle_get_request(
    stream: &mut IrohDuplex,
    request: crate::transport::transfer::GetRequest,
    shell_state: ConnectionShellState,
) -> Result<()> {
    let Some(shell) = LiveShellContext::from_state(&shell_state) else {
        write_transfer_error(
            stream,
            &TransferFailure::new(
                TransferFailureCode::RemoteShellUnavailable,
                "no live shell process",
            ),
        )
        .await
        .map_err(TransportError::from)?;
        return Ok(());
    };

    let source_path = resolve_remote_path(&request.path)?;
    let expected_size = match probe_download_size(shell, &source_path).await? {
        Ok(size) => size,
        Err(failure) => {
            write_transfer_error(stream, &failure)
                .await
                .map_err(TransportError::from)?;
            return Ok(());
        }
    };

    let (mut child, helper_source) = spawn_download_helper(shell, &source_path).await?;

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
                    "{}; shell_pid={}; requested={}; helper_arg={}",
                    String::from_utf8_lossy(&output.stderr).trim(),
                    shell.pid(),
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

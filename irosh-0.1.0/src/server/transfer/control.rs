use tokio::process::Command;

use crate::error::{Result, ServerError, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    CwdResponse, ExistsRequest, ExistsResponse, TransferFailure, TransferFailureCode,
    write_cwd_response, write_exists_response, write_transfer_error,
};

use super::{LiveShellContext, resolve_remote_path};

pub(super) async fn handle_exists_request(
    stream: &mut IrohDuplex,
    request: ExistsRequest,
    shell_pid: Option<u32>,
) -> Result<()> {
    let resolved = resolve_remote_path(&request.path)?;
    let helper_path = resolved.display().to_string();

    let mut exists_cmd = Command::new("test");
    if let Some(shell) = LiveShellContext::from_pid(shell_pid) {
        let exists = shell.path_exists(&helper_path).await?;
        write_exists_response(stream, &ExistsResponse { exists })
            .await
            .map_err(TransportError::from)?;
        return Ok(());
    }
    exists_cmd.arg("-e").arg(&helper_path);
    let status = exists_cmd
        .status()
        .await
        .map_err(|e| ServerError::ShellError {
            details: format!("probing remote existence without live shell failed: {e}"),
        })?;
    write_exists_response(
        stream,
        &ExistsResponse {
            exists: status.success(),
        },
    )
    .await
    .map_err(TransportError::from)?;
    Ok(())
}

pub(super) async fn handle_cwd_request(
    stream: &mut IrohDuplex,
    shell_pid: Option<u32>,
) -> Result<()> {
    let Some(shell) = LiveShellContext::from_pid(shell_pid) else {
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

    let cwd = shell.cwd().await?;
    write_cwd_response(
        stream,
        &CwdResponse {
            path: cwd.display().to_string(),
        },
    )
    .await
    .map_err(TransportError::from)?;
    Ok(())
}

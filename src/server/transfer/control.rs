use crate::error::{Result, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    CwdResponse, ExistsRequest, ExistsResponse, write_cwd_response, write_exists_response,
};

use super::ShellContext;

pub(super) async fn handle_exists_request(
    stream: &mut IrohDuplex,
    request: ExistsRequest,
    context: ShellContext,
) -> Result<()> {
    let resolved = context.resolve_path(&request.path).await?;
    let path_str = resolved.display().to_string();

    let exists = context.path_exists(&path_str).await?;

    write_exists_response(stream, &ExistsResponse { exists })
        .await
        .map_err(TransportError::from)?;
    Ok(())
}

pub(super) async fn handle_cwd_request(
    stream: &mut IrohDuplex,
    context: ShellContext,
) -> Result<()> {
    let cwd = context.cwd().await?;
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

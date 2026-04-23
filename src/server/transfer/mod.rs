use crate::error::{Result, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    TransferFailure, TransferFailureCode, TransferFrame, read_next_frame, write_transfer_error,
};
use tracing::warn;

mod control;
mod files;
mod helpers;
mod state;

pub(crate) use state::ConnectionShellState;
pub(super) use state::{ShellContext, resolve_remote_path};

pub(crate) async fn handle_transfer_stream(
    send: iroh::endpoint::SendStream,
    recv: iroh::endpoint::RecvStream,
    shell_state: ConnectionShellState,
) -> Result<()> {
    let mut stream = IrohDuplex::new(send, recv);
    let context = ShellContext::from_state(&shell_state);

    match read_next_frame(&mut stream).await {
        Ok(TransferFrame::PutRequest(request)) => {
            if let Err(err) = files::handle_put_request(&mut stream, request, context).await {
                warn!("Put transfer handler failed: {}", err);
                let _ = write_transfer_error(
                    &mut stream,
                    &TransferFailure::new(TransferFailureCode::Internal, best_error_message(&err)),
                )
                .await;
            }
            Ok(())
        }
        Ok(TransferFrame::GetRequest(request)) => {
            if let Err(err) = files::handle_get_request(&mut stream, request, context).await {
                warn!("Get transfer handler failed: {}", err);
                let _ = write_transfer_error(
                    &mut stream,
                    &TransferFailure::new(TransferFailureCode::Internal, best_error_message(&err)),
                )
                .await;
            }
            Ok(())
        }
        Ok(TransferFrame::CwdRequest(_)) => {
            if let Err(err) = control::handle_cwd_request(&mut stream, context).await {
                warn!("Cwd request handler failed: {}", err);
                let _ = write_transfer_error(
                    &mut stream,
                    &TransferFailure::new(TransferFailureCode::Internal, best_error_message(&err)),
                )
                .await;
            }
            Ok(())
        }
        Ok(TransferFrame::ExistsRequest(req)) => {
            if let Err(err) = control::handle_exists_request(&mut stream, req, context).await {
                warn!("Exists request handler failed: {}", err);
                let _ = write_transfer_error(
                    &mut stream,
                    &TransferFailure::new(TransferFailureCode::Internal, best_error_message(&err)),
                )
                .await;
            }
            Ok(())
        }
        Ok(other) => {
            write_transfer_error(
                &mut stream,
                &TransferFailure::new(TransferFailureCode::UnexpectedFrame, format!("{other:?}")),
            )
            .await
            .map_err(TransportError::from)?;
            Ok(())
        }
        Err(err) => {
            warn!("Transfer frame decode failed: {}", err);
            Ok(())
        }
    }
}

fn best_error_message(err: &dyn std::error::Error) -> String {
    let mut best = None;
    let mut current: Option<&dyn std::error::Error> = Some(err);

    while let Some(cause) = current {
        let message = cause.to_string();
        if matches!(
            message.as_str(),
            "client error" | "server error" | "transport error" | "storage error"
        ) {
            current = cause.source();
            continue;
        }
        best = Some(message);
        current = cause.source();
    }

    best.unwrap_or_else(|| err.to_string())
}

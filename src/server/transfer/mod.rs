//! Server-side transfer protocol.
use crate::error::{Result, TransportError};
use crate::metrics::Metrics;
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    CURRENT_MAX_KIND, Capability, TransferFailure, TransferFailureCode, TransferFrame,
    read_next_frame, write_capability, write_transfer_error,
};
use tracing::warn;

mod control;
mod files;
mod helpers;
mod state;

pub use state::ConnectionShellState;
pub(super) use state::ShellContext;

/// Negotiate capabilities with the client, then dispatch the actual request.
async fn negotiate_and_dispatch(
    connection: iroh::endpoint::Connection,
    stream: &mut IrohDuplex,
    shell_state: &ConnectionShellState,
    metrics: &Metrics,
    context: ShellContext,
) -> Result<()> {
    match read_next_frame(stream).await {
        Ok(TransferFrame::Capability(client_cap)) => {
            let negotiated = std::cmp::min(client_cap.max_kind, CURRENT_MAX_KIND);
            write_capability(
                stream,
                &Capability {
                    max_kind: negotiated,
                },
            )
            .await?;
            match read_next_frame(stream).await {
                Ok(frame) => {
                    dispatch_transfer_frame(
                        connection,
                        stream,
                        frame,
                        shell_state,
                        metrics,
                        context,
                    )
                    .await
                }
                Err(err) => {
                    warn!(
                        "Failed to read request after capability negotiation: {}",
                        err
                    );
                    metrics.record_error();
                    Ok(())
                }
            }
        }
        Ok(frame) => {
            // Legacy client — no capability frame sent.
            dispatch_transfer_frame(connection, stream, frame, shell_state, metrics, context).await
        }
        Err(err) => {
            warn!("Transfer frame decode failed: {}", err);
            metrics.record_error();
            Ok(())
        }
    }
}

/// Dispatch a single decoded transfer frame to the appropriate handler.
async fn dispatch_transfer_frame(
    connection: iroh::endpoint::Connection,
    stream: &mut IrohDuplex,
    frame: TransferFrame,
    shell_state: &ConnectionShellState,
    metrics: &Metrics,
    context: ShellContext,
) -> Result<()> {
    match frame {
        TransferFrame::PutRequest(request) => {
            if let Err(err) = files::handle_put_request(stream, request, context, shell_state).await
            {
                warn!("Put transfer handler failed: {}", err);
                metrics.record_transfer_failed();
                let failure = extract_transfer_failure(&err);
                let _ = write_transfer_error(stream, &failure).await;
            } else {
                metrics.record_transfer_completed();
            }
            Ok(())
        }
        TransferFrame::BlobPutRequest(request) => {
            if let Err(err) =
                files::handle_blob_put_request(stream, connection, request, context, shell_state)
                    .await
            {
                warn!("Blob Put transfer handler failed: {}", err);
                metrics.record_transfer_failed();
                let failure = extract_transfer_failure(&err);
                let _ = write_transfer_error(stream, &failure).await;
            } else {
                metrics.record_transfer_completed();
            }
            Ok(())
        }
        TransferFrame::BlobGetRequest(request) => {
            if let Err(err) =
                files::handle_blob_get_request(stream, connection, request, context, shell_state)
                    .await
            {
                warn!("Blob Get transfer handler failed: {}", err);
                metrics.record_transfer_failed();
                let failure = extract_transfer_failure(&err);
                let _ = write_transfer_error(stream, &failure).await;
            } else {
                metrics.record_transfer_completed();
            }
            Ok(())
        }
        TransferFrame::GetRequest(request) => {
            if let Err(err) = files::handle_get_request(stream, request, context, shell_state).await
            {
                warn!("Get transfer handler failed: {}", err);
                metrics.record_transfer_failed();
                let failure = extract_transfer_failure(&err);
                let _ = write_transfer_error(stream, &failure).await;
            } else {
                metrics.record_transfer_completed();
            }
            Ok(())
        }
        TransferFrame::CwdRequest(_) => {
            if let Err(err) = control::handle_cwd_request(stream, context, shell_state).await {
                warn!("Cwd request handler failed: {}", err);
                metrics.record_error();
                let failure = extract_transfer_failure(&err);
                let _ = write_transfer_error(stream, &failure).await;
            }
            Ok(())
        }
        TransferFrame::ExistsRequest(req) => {
            if let Err(err) =
                control::handle_exists_request(stream, req, context, shell_state).await
            {
                warn!("Exists request handler failed: {}", err);
                metrics.record_error();
                let failure = extract_transfer_failure(&err);
                let _ = write_transfer_error(stream, &failure).await;
            }
            Ok(())
        }
        TransferFrame::CompletionRequest(req) => {
            if let Err(err) =
                control::handle_completion_request(stream, req, context, shell_state).await
            {
                warn!("Completion request handler failed: {}", err);
                metrics.record_error();
                let failure = extract_transfer_failure(&err);
                let _ = write_transfer_error(stream, &failure).await;
            }
            Ok(())
        }
        TransferFrame::Capability(_) => {
            // Already handled in negotiate_and_dispatch; this arm is unreachable.
            unreachable!("Capability frame should have been handled in negotiation");
        }
        other => {
            metrics.record_error();
            write_transfer_error(
                stream,
                &TransferFailure::new(TransferFailureCode::UnexpectedFrame, format!("{other:?}")),
            )
            .await
            .map_err(TransportError::from)?;
            Ok(())
        }
    }
}

pub(crate) async fn handle_transfer_stream(
    connection: iroh::endpoint::Connection,
    mut stream: IrohDuplex,
    shell_state: ConnectionShellState,
    metrics: Metrics,
) -> Result<()> {
    let context = ShellContext::from_state(&shell_state);
    metrics.record_transfer_initiated();
    negotiate_and_dispatch(connection, &mut stream, &shell_state, &metrics, context).await
}

fn extract_transfer_failure(err: &crate::error::IroshError) -> TransferFailure {
    if let crate::error::IroshError::Server(crate::error::ServerError::TransferFailed { failure }) =
        err
    {
        return failure.clone();
    }

    TransferFailure::new(TransferFailureCode::Internal, best_error_message(err))
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

use tokio::io::AsyncReadExt;
use tracing::warn;

use crate::error::{IroshError, TransportError};
use crate::server::transfer::{ConnectionShellState, handle_transfer_stream};
use crate::transport::metadata::{PeerMetadata, read_metadata_request, write_metadata};
use crate::transport::stream::IrohDuplex;

pub(crate) fn spawn_side_stream_listener(
    connection: iroh::endpoint::Connection,
    shell_state: ConnectionShellState,
) {
    tokio::spawn(async move {
        tracing::debug!("Side-stream listener started");
        loop {
            tokio::select! {
                biased;
                _ = connection.closed() => {
                    tracing::debug!("Side-stream listener: connection closed");
                    break;
                }
                res = connection.accept_bi() => {
                    match res {
                        Ok((send, recv)) => {
                            let shell_state = shell_state.clone();
                            let conn = connection.clone();
                            tokio::spawn(async move {
                                if let Err(err) = handle_side_stream_dispatch(conn, send, recv, shell_state).await {
                                    warn!("Side-stream handler failed: {}", err);
                                }
                            });
                        }
                        Err(err) => {
                            tracing::debug!("Side-stream listener: accept_bi failed: {}", err);
                            break;
                        }
                    }
                }
            }
        }
        tracing::debug!("Side-stream listener finished");
    });
}

async fn handle_side_stream_dispatch(
    connection: iroh::endpoint::Connection,
    send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
    shell_state: ConnectionShellState,
) -> crate::error::Result<()> {
    let mut magic = [0u8; 4];
    // Use tokio's AsyncReadExt explicitly to avoid conflict with Iroh's native read_exact
    AsyncReadExt::read_exact(&mut recv, &mut magic).await?;

    if magic == crate::transport::metadata::codec::MAGIC {
        let mut stream = IrohDuplex::with_prefix(send, recv, magic.to_vec());
        read_metadata_request(&mut stream)
            .await
            .map_err(|e| IroshError::Transport(TransportError::Metadata(e)))?;
        let metadata = PeerMetadata::current().await;
        write_metadata(&mut stream, &metadata)
            .await
            .map_err(|e| IroshError::Transport(TransportError::Metadata(e)))?;
        tracing::debug!("Metadata exchange complete");
    } else if magic == crate::transport::transfer::codec::MAGIC {
        let stream = IrohDuplex::with_prefix(send, recv, magic.to_vec());
        handle_transfer_stream(connection, stream, shell_state).await?;
    } else {
        warn!("Unknown side-stream magic: {:?}", magic);
    }
    Ok(())
}

use std::time::Duration;

use tracing::warn;

use crate::server::transfer::{ConnectionShellState, handle_transfer_stream};
use crate::transport::metadata::{PeerMetadata, read_metadata_request, write_metadata};
use crate::transport::stream::IrohDuplex;

const METADATA_ACCEPT_TIMEOUT: Duration = Duration::from_secs(5);
const METADATA_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn spawn_metadata_and_transfer_acceptor(
    connection: iroh::endpoint::Connection,
    shell_state: ConnectionShellState,
) {
    tokio::spawn(async move {
        let accept = tokio::time::timeout(METADATA_ACCEPT_TIMEOUT, connection.accept_bi()).await;

        let Ok(Ok((send, recv))) = accept else {
            return;
        };

        let mut metadata_stream = IrohDuplex::new(send, recv);
        let request = tokio::time::timeout(METADATA_REQUEST_TIMEOUT, async {
            read_metadata_request(&mut metadata_stream).await
        })
        .await;

        let Ok(Ok(())) = request else {
            return;
        };

        let metadata = PeerMetadata::current();
        if let Err(err) = write_metadata(&mut metadata_stream, &metadata).await {
            warn!("Metadata stream failed: {}", err);
        }

        while let Ok((send, recv)) = connection.accept_bi().await {
            let shell_state = shell_state.clone();
            tokio::spawn(async move {
                if let Err(err) = handle_transfer_stream(send, recv, shell_state).await {
                    warn!("Transfer stream failed: {}", err);
                }
            });
        }
    });
}

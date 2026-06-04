//! Transfer control messages.
use crate::client::Session;
use crate::error::{ClientError, Result, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    CwdRequest, ExistsRequest, TransferFrame, read_next_frame, write_cwd_request,
    write_exists_request,
};

impl Session {
    /// Queries the current working directory of the live remote shell process for this session.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer stream cannot be opened, the request
    /// or response frames cannot be exchanged, or the remote side rejects the
    /// request.
    #[must_use]
    pub async fn remote_cwd(&self) -> Result<std::path::PathBuf> {
        let mut stream = self.open_transfer_stream("remote cwd unavailable").await?;

        write_cwd_request(&mut stream, &CwdRequest)
            .await
            .map_err(TransportError::from)?;
        let res = read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?;
        match res {
            TransferFrame::CwdResponse(res) => Ok(std::path::PathBuf::from(res.path)),
            TransferFrame::Error(failure) => Err(ClientError::TransferRejected { failure }.into()),
            other => Err(ClientError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::UnexpectedFrame,
                    format!("remote cwd failed with unexpected frame: {other:?}"),
                ),
            }
            .into()),
        }
    }

    /// Checks if a file or directory exists on the remote system.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer stream cannot be opened, the request
    /// or response frames cannot be exchanged, or the remote side rejects the
    /// request.
    #[must_use]
    pub async fn remote_exists(&self, path: &std::path::Path) -> Result<bool> {
        let mut stream = self
            .open_transfer_stream("remote exists unavailable")
            .await?;

        write_exists_request(
            &mut stream,
            &ExistsRequest {
                path: path.display().to_string(),
            },
        )
        .await
        .map_err(TransportError::from)?;
        let res = read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?;
        match res {
            TransferFrame::ExistsResponse(res) => Ok(res.exists),
            TransferFrame::Error(failure) => Err(ClientError::TransferRejected { failure }.into()),
            other => Err(ClientError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::UnexpectedFrame,
                    format!("remote exists failed with unexpected frame: {other:?}"),
                ),
            }
            .into()),
        }
    }

    pub(crate) async fn open_transfer_stream(
        &self,
        unavailable_context: &'static str,
    ) -> Result<IrohDuplex> {
        let connection =
            self.connection
                .as_ref()
                .ok_or_else(|| ClientError::TransportUnavailable {
                    details: unavailable_context,
                })?;
        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(|source| ClientError::StreamOpenFailed { source })?;
        Ok(IrohDuplex::new(send, recv))
    }

    /// Open a transfer stream and negotiate capabilities with the peer.
    ///
    /// Returns the negotiated stream and the agreed-upon max frame kind.
    /// Falls back to legacy mode if the peer does not support capability negotiation.
    pub(crate) async fn open_negotiated_stream(
        &self,
        context: &'static str,
    ) -> Result<(IrohDuplex, u8)> {
        use crate::transport::transfer::{
            CURRENT_MAX_KIND, Capability, LEGACY_MAX_KIND, read_next_frame, write_capability,
        };

        let mut stream = self.open_transfer_stream(context).await?;

        write_capability(
            &mut stream,
            &Capability {
                max_kind: CURRENT_MAX_KIND,
            },
        )
        .await
        .map_err(TransportError::from)?;

        match read_next_frame(&mut stream).await {
            Ok(TransferFrame::Capability(server_cap)) => {
                let negotiated = std::cmp::min(CURRENT_MAX_KIND, server_cap.max_kind);
                Ok((stream, negotiated))
            }
            Ok(_) => {
                // Unexpected frame — drop stream and fall back to legacy
                drop(stream);
                let stream = self.open_transfer_stream(context).await?;
                Ok((stream, LEGACY_MAX_KIND))
            }
            Err(_) => {
                // Old server dropped the stream — fall back to legacy
                drop(stream);
                let stream = self.open_transfer_stream(context).await?;
                Ok((stream, LEGACY_MAX_KIND))
            }
        }
    }
}

use crate::client::Session;
use crate::error::{ClientError, Result, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    CwdRequest, ExistsRequest, TransferFrame, read_next_frame, write_cwd_request,
    write_exists_request,
};

impl Session {
    /// Queries the current working directory of the live remote shell process for this session.
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
            TransferFrame::Error(details) => Err(ClientError::TransferRejected {
                details: details.to_string(),
            }
            .into()),
            other => Err(ClientError::TransferFailed {
                details: format!("remote cwd failed with unexpected frame: {other:?}"),
            }
            .into()),
        }
    }

    /// Checks if a file or directory exists on the remote system.
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
            TransferFrame::Error(details) => Err(ClientError::TransferRejected {
                details: details.to_string(),
            }
            .into()),
            other => Err(ClientError::TransferFailed {
                details: format!("remote exists failed with unexpected frame: {other:?}"),
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
}

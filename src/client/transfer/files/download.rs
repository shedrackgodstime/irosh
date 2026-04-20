use tokio::io::AsyncWriteExt;

use crate::client::{Session, TransferProgress};
use crate::error::{ClientError, Result, TransportError};
use crate::transport::transfer::{TransferFrame, read_next_frame, write_get_request};

use crate::client::transfer::store::{persist_temp_file, temp_transfer_path};

impl Session {
    /// Downloads one remote file to a local path on a separate authenticated stream.
    pub async fn get_file(
        &mut self,
        remote: impl AsRef<std::path::Path>,
        local: impl AsRef<std::path::Path>,
    ) -> Result<()> {
        self.get_file_with_progress(remote, local, |_| {}).await
    }

    /// Downloads one remote file and reports progress synchronously through the callback.
    pub async fn get_file_with_progress<F>(
        &mut self,
        remote: impl AsRef<std::path::Path>,
        local: impl AsRef<std::path::Path>,
        mut on_progress: F,
    ) -> Result<()>
    where
        F: FnMut(TransferProgress),
    {
        let remote = remote.as_ref();
        let local = local.as_ref();
        if remote.as_os_str().is_empty() {
            return Err(ClientError::TransferTargetInvalid {
                reason: "remote path is empty",
            }
            .into());
        }
        if local.as_os_str().is_empty() {
            return Err(ClientError::TransferTargetInvalid {
                reason: "local path is empty",
            }
            .into());
        }

        let mut stream = self.open_transfer_stream("download unavailable").await?;

        write_get_request(
            &mut stream,
            &crate::transport::transfer::GetRequest {
                path: remote.display().to_string(),
            },
        )
        .await
        .map_err(TransportError::from)?;

        let (expected_size, expected_mode) = match read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::GetReady(ready) => (ready.size, ready.mode),
            TransferFrame::Error(details) => {
                return Err(ClientError::TransferRejected {
                    details: details.to_string(),
                }
                .into());
            }
            other => {
                return Err(ClientError::DownloadFailed {
                    details: format!("unexpected preflight frame: {other:?}"),
                }
                .into());
            }
        };
        on_progress(TransferProgress::new(0, expected_size));

        let temp_path = temp_transfer_path(local);
        let mut dest = tokio::fs::File::create(&temp_path)
            .await
            .map_err(|source| ClientError::FileIo {
                operation: "create temp download file",
                path: temp_path.clone(),
                source,
            })?;

        let mut received = 0u64;
        loop {
            match read_next_frame(&mut stream)
                .await
                .map_err(TransportError::from)?
            {
                TransferFrame::GetChunk(chunk) => {
                    received += chunk.len() as u64;
                    dest.write_all(&chunk)
                        .await
                        .map_err(|source| ClientError::FileIo {
                            operation: "write to temp download file",
                            path: temp_path.clone(),
                            source,
                        })?;
                    on_progress(TransferProgress::new(received, expected_size));
                }
                TransferFrame::GetComplete(complete) => {
                    if complete.size != expected_size || received != expected_size {
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        return Err(ClientError::DownloadFailed {
                            details: format!(
                                "expected {expected_size} bytes, received {received}, completion reported {}",
                                complete.size
                            ),
                        }
                        .into());
                    }
                    break;
                }
                TransferFrame::Error(details) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(ClientError::TransferRejected {
                        details: details.to_string(),
                    }
                    .into());
                }
                other => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(ClientError::DownloadFailed {
                        details: format!("unexpected data stream frame: {other:?}"),
                    }
                    .into());
                }
            }
        }

        dest.flush().await.map_err(|source| ClientError::FileIo {
            operation: "flush temp download file",
            path: temp_path.clone(),
            source,
        })?;
        drop(dest);

        persist_temp_file(&temp_path, local).await?;

        if let Some(mode) = expected_mode {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    tokio::fs::set_permissions(local, std::fs::Permissions::from_mode(mode)).await;
            }
        }

        Ok(())
    }
}

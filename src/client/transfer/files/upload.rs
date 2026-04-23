use tokio::io::AsyncReadExt;

use crate::client::{Session, TransferProgress};
use crate::error::{ClientError, Result, TransportError};
use crate::transport::transfer::{
    MAX_CHUNK_BYTES, TransferFrame, TransferReady, read_next_frame, write_put_chunk,
    write_put_complete, write_put_request,
};

impl Session {
    /// Uploads one local file or directory to the remote peer.
    ///
    /// If `local` is a directory, it will be uploaded recursively.
    pub async fn put(
        &mut self,
        local: impl AsRef<std::path::Path>,
        remote: impl AsRef<std::path::Path>,
        recursive: bool,
    ) -> Result<()> {
        self.put_with_progress(local, remote, recursive, |_| {})
            .await
    }

    /// Uploads one local file or directory with progress reporting.
    pub async fn put_with_progress<F>(
        &mut self,
        local: impl AsRef<std::path::Path>,
        remote: impl AsRef<std::path::Path>,
        recursive: bool,
        on_progress: F,
    ) -> Result<()>
    where
        F: FnMut(TransferProgress) + Clone + Send + 'static,
    {
        let local = local.as_ref();
        let remote = remote.as_ref();

        if local.is_dir() {
            if !recursive {
                return Err(ClientError::TransferTargetInvalid {
                    reason: "source is a directory, but recursive flag not set",
                }
                .into());
            }
            self.put_dir_with_progress(local, remote, on_progress).await
        } else {
            self.put_file_with_progress(local, remote, on_progress)
                .await
        }
    }

    /// Uploads one local file to the remote peer on a separate authenticated stream.
    pub async fn put_file(
        &mut self,
        local: impl AsRef<std::path::Path>,
        remote: impl AsRef<std::path::Path>,
    ) -> Result<()> {
        self.put_file_with_progress(local, remote, |_| {}).await
    }

    /// Uploads one local file and reports progress synchronously through the callback.
    pub async fn put_file_with_progress<F>(
        &mut self,
        local: impl AsRef<std::path::Path>,
        remote: impl AsRef<std::path::Path>,
        mut on_progress: F,
    ) -> Result<()>
    where
        F: FnMut(TransferProgress),
    {
        let local = local.as_ref();
        let remote = remote.as_ref();
        if remote.as_os_str().is_empty() {
            return Err(ClientError::TransferTargetInvalid {
                reason: "remote path is empty",
            }
            .into());
        }

        let mut stream = self.open_transfer_stream("upload unavailable").await?;

        let mut file =
            tokio::fs::File::open(local)
                .await
                .map_err(|source| ClientError::FileIo {
                    operation: "open local source file",
                    path: local.to_path_buf(),
                    source,
                })?;
        let metadata = file
            .metadata()
            .await
            .map_err(|source| ClientError::FileIo {
                operation: "read local source metadata",
                path: local.to_path_buf(),
                source,
            })?;
        let size = metadata.len();
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            Some(metadata.permissions().mode() & 0o777)
        };
        #[cfg(not(unix))]
        let mode = None;

        on_progress(TransferProgress::new(0, size));

        write_put_request(
            &mut stream,
            &crate::transport::transfer::PutRequest {
                path: remote.display().to_string(),
                size,
                mode,
            },
        )
        .await
        .map_err(TransportError::from)?;

        match read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::PutReady(TransferReady {
                size: remote_size, ..
            }) => {
                if remote_size != size {
                    return Err(ClientError::UploadFailed {
                        details: format!(
                            "remote acknowledged unexpected size {remote_size}, expected {size}"
                        ),
                    }
                    .into());
                }
            }
            TransferFrame::Error(details) => {
                return Err(ClientError::TransferRejected {
                    details: details.to_string(),
                }
                .into());
            }
            other => {
                return Err(ClientError::UploadFailed {
                    details: format!("unexpected preflight frame: {other:?}"),
                }
                .into());
            }
        }

        let mut sent = 0u64;
        let mut buffer = vec![0u8; MAX_CHUNK_BYTES];
        loop {
            let count = file
                .read(&mut buffer)
                .await
                .map_err(|source| ClientError::FileIo {
                    operation: "read local source file",
                    path: local.to_path_buf(),
                    source,
                })?;
            if count == 0 {
                break;
            }
            sent += count as u64;
            write_put_chunk(&mut stream, &buffer[..count])
                .await
                .map_err(TransportError::from)?;
            on_progress(TransferProgress::new(sent, size));
        }

        write_put_complete(
            &mut stream,
            &crate::transport::transfer::TransferComplete { size: sent },
        )
        .await
        .map_err(TransportError::from)?;

        match read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::PutComplete(complete) if complete.size == sent => Ok(()),
            TransferFrame::PutComplete(complete) => Err(ClientError::UploadFailed {
                details: format!(
                    "remote reported {} bytes saved, expected {sent}",
                    complete.size
                ),
            }
            .into()),
            TransferFrame::Error(details) => Err(ClientError::TransferRejected {
                details: details.to_string(),
            }
            .into()),
            other => Err(ClientError::UploadFailed {
                details: format!("unexpected completion frame: {other:?}"),
            }
            .into()),
        }
    }

    /// Uploads a directory recursively to the remote peer.
    pub async fn put_dir_with_progress<F>(
        &mut self,
        local: impl AsRef<std::path::Path>,
        remote: impl AsRef<std::path::Path>,
        on_progress: F,
    ) -> Result<()>
    where
        F: FnMut(TransferProgress) + Clone + Send + 'static,
    {
        let local_root = local.as_ref();
        let remote_root = remote.as_ref();

        let mut entries = Vec::new();
        let walk = walkdir::WalkDir::new(local_root);

        for entry in walk {
            let entry = entry.map_err(|e| ClientError::FileIo {
                operation: "walk local directory",
                path: local_root.to_path_buf(),
                source: e.into(),
            })?;
            if entry.file_type().is_file() {
                let relative = entry.path().strip_prefix(local_root).map_err(|_| {
                    ClientError::TransferTargetInvalid {
                        reason: "failed to resolve relative path during directory walk",
                    }
                })?;
                entries.push((entry.path().to_path_buf(), remote_root.join(relative)));
            }
        }

        // For now, we transfer them sequentially.
        for (local_path, remote_path) in entries {
            // We use a fresh progress callback for each file.
            self.put_file_with_progress(&local_path, &remote_path, on_progress.clone())
                .await?;
        }

        Ok(())
    }
}

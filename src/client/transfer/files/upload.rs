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
    ///
    /// This method manages the entire upload lifecycle:
    /// 1. Connects a dedicated P2P side-stream for the transfer.
    /// 2. Performs a handshake and determines if the remote target is valid.
    /// 3. Streams the local data (recursively if `recursive` is true).
    /// 4. Verifies completion with the remote peer.
    ///
    /// The `on_progress` closure is invoked periodically as bytes are written
    /// to the transport stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails, the remote target exists and
    /// cannot be overwritten, or if the transfer is interrupted.
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
                recursive: false,
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
        mut on_progress: F,
    ) -> Result<()>
    where
        F: FnMut(TransferProgress) + Clone + Send + 'static,
    {
        let local_root = local.as_ref();
        let remote_root = remote.as_ref();

        let mut stream = self
            .open_transfer_stream("recursive upload unavailable")
            .await?;

        // 1. Send recursive PutRequest
        write_put_request(
            &mut stream,
            &crate::transport::transfer::PutRequest {
                path: remote_root.display().to_string(),
                size: 0, // Size is cumulative in recursive mode
                mode: None,
                recursive: true,
            },
        )
        .await
        .map_err(TransportError::from)?;

        // 2. Expect PutReady
        match read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::PutReady(_) => {}
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

        let mut total_sent = 0u64;
        let walk = walkdir::WalkDir::new(local_root);

        for entry in walk {
            let entry = entry.map_err(|e| ClientError::FileIo {
                operation: "walk local directory",
                path: local_root.to_path_buf(),
                source: e.into(),
            })?;

            reject_recursive_symlink(&entry)?;

            let relative = entry.path().strip_prefix(local_root).map_err(|_| {
                ClientError::TransferTargetInvalid {
                    reason: "failed to resolve relative path during directory walk",
                }
            })?;

            // Skip the root itself in the walk if it's the first entry
            if relative.as_os_str().is_empty() {
                continue;
            }

            let is_dir = entry.file_type().is_dir();
            let metadata = entry.metadata().map_err(|e| ClientError::FileIo {
                operation: "read entry metadata",
                path: entry.path().to_path_buf(),
                source: e.into(),
            })?;

            let size = if is_dir { 0 } else { metadata.len() };
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                Some(metadata.permissions().mode() & 0o777)
            };
            #[cfg(not(unix))]
            let mode = None;

            // Send NewEntry frame
            crate::transport::transfer::write_new_entry(
                &mut stream,
                &crate::transport::transfer::EntryHeader {
                    path: relative.display().to_string(),
                    size,
                    mode,
                    is_dir,
                },
            )
            .await
            .map_err(TransportError::from)?;

            if !is_dir {
                let mut file =
                    tokio::fs::File::open(entry.path())
                        .await
                        .map_err(|e| ClientError::FileIo {
                            operation: "open file for streaming",
                            path: entry.path().to_path_buf(),
                            source: e,
                        })?;

                let mut buffer = vec![0u8; MAX_CHUNK_BYTES];
                loop {
                    let count = file
                        .read(&mut buffer)
                        .await
                        .map_err(|e| ClientError::FileIo {
                            operation: "read file chunk",
                            path: entry.path().to_path_buf(),
                            source: e,
                        })?;
                    if count == 0 {
                        break;
                    }
                    write_put_chunk(&mut stream, &buffer[..count])
                        .await
                        .map_err(TransportError::from)?;
                    total_sent += count as u64;
                    on_progress(TransferProgress::new(total_sent, 0)); // Total size unknown upfront
                }

                crate::transport::transfer::write_entry_complete(
                    &mut stream,
                    &crate::transport::transfer::EntryComplete,
                )
                .await
                .map_err(TransportError::from)?;
            }
        }

        // 3. Send final PutComplete
        write_put_complete(
            &mut stream,
            &crate::transport::transfer::TransferComplete { size: total_sent },
        )
        .await
        .map_err(TransportError::from)?;

        match read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::PutComplete(_) => Ok(()),
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
}

fn reject_recursive_symlink(entry: &walkdir::DirEntry) -> Result<()> {
    if entry.file_type().is_symlink() {
        return Err(ClientError::UploadFailed {
            details: format!(
                "recursive upload does not support symbolic links: {}",
                entry.path().display()
            ),
        }
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::reject_recursive_symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    #[test]
    fn recursive_upload_rejects_symlink_entries() {
        use std::os::unix::fs::symlink;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("irosh-symlink-upload-{unique}"));
        std::fs::create_dir_all(&root).expect("create temp root");
        let target = root.join("real-dir");
        std::fs::create_dir_all(&target).expect("create real dir");
        let link = root.join("link-dir");
        symlink(&target, &link).expect("create symlink");

        let mut walk = walkdir::WalkDir::new(&root).into_iter();
        let _root_entry = walk.next().expect("walk root").expect("root entry");
        let link_entry = walk
            .find_map(|entry| {
                let entry = entry.ok()?;
                (entry.path() == link).then_some(entry)
            })
            .expect("find symlink entry");

        let err = reject_recursive_symlink(&link_entry).expect_err("symlink should be rejected");
        assert!(
            err.to_string()
                .contains("recursive upload does not support symbolic links")
        );

        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir_all(&root);
    }
}

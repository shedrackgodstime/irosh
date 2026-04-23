use tokio::io::AsyncWriteExt;

use crate::client::{Session, TransferProgress};
use crate::error::{ClientError, Result, TransportError};
use crate::transport::transfer::{TransferFrame, read_next_frame, write_get_request};

use crate::client::transfer::store::{persist_temp_file, temp_transfer_path};

impl Session {
    /// Downloads one remote file or directory to a local path.
    ///
    /// If `remote` is a directory, it will be downloaded recursively.
    pub async fn get(
        &mut self,
        remote: impl AsRef<std::path::Path>,
        local: impl AsRef<std::path::Path>,
        recursive: bool,
    ) -> Result<()> {
        self.get_with_progress(remote, local, recursive, |_| {})
            .await
    }

    /// Downloads one remote file or directory with progress reporting.
    ///
    /// This method manages the entire download lifecycle:
    /// 1. Connects a dedicated P2P side-stream for the transfer.
    /// 2. Performs a handshake and determines the expected size and mode.
    /// 3. Streams the remote data (recursively if `recursive` is true).
    /// 4. Atomic persistence: Data is written to a temporary file first and
    ///    only moved to the final destination upon successful completion.
    ///
    /// The `on_progress` closure is invoked periodically as bytes are read
    /// from the transport stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails, the remote path is not found,
    /// or if the local filesystem prevents writing the data.
    pub async fn get_with_progress<F>(
        &mut self,
        remote: impl AsRef<std::path::Path>,
        local: impl AsRef<std::path::Path>,
        recursive: bool,
        on_progress: F,
    ) -> Result<()>
    where
        F: FnMut(TransferProgress) + Clone + Send + 'static,
    {
        let remote = remote.as_ref();
        let local = local.as_ref();

        // Check if remote is a directory.
        let is_dir = self.is_remote_dir(remote).await?;

        if is_dir {
            if !recursive {
                return Err(ClientError::TransferTargetInvalid {
                    reason: "remote is a directory, but recursive flag not set",
                }
                .into());
            }
            self.get_dir_with_progress(remote, local, on_progress).await
        } else {
            self.get_file_with_progress(remote, local, on_progress)
                .await
        }
    }

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
                recursive: false,
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
                    details: format!("remote rejected file {:?}: {}", remote, details),
                }
                .into());
            }
            other => {
                return Err(ClientError::DownloadFailed {
                    details: format!("unexpected preflight frame for {:?}: {:?}", remote, other),
                }
                .into());
            }
        };
        on_progress(TransferProgress::new(0, expected_size));

        let temp_path = temp_transfer_path(local);

        // Ensure parent directory exists for local path
        if let Some(parent) = local.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

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
                        details: format!(
                            "remote error during data stream for {:?}: {}",
                            remote, details
                        ),
                    }
                    .into());
                }
                other => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(ClientError::DownloadFailed {
                        details: format!(
                            "unexpected data stream frame for {:?}: {:?}",
                            remote, other
                        ),
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

    /// Downloads a remote directory recursively.
    pub async fn get_dir_with_progress<F>(
        &mut self,
        remote: impl AsRef<std::path::Path>,
        local: impl AsRef<std::path::Path>,
        mut on_progress: F,
    ) -> Result<()>
    where
        F: FnMut(TransferProgress) + Clone + Send + 'static,
    {
        let remote_root = remote.as_ref();
        let local_root = local.as_ref();

        let mut stream = self
            .open_transfer_stream("recursive download unavailable")
            .await?;

        // 1. Send recursive GetRequest
        crate::transport::transfer::write_get_request(
            &mut stream,
            &crate::transport::transfer::GetRequest {
                path: remote_root.display().to_string(),
                recursive: true,
            },
        )
        .await
        .map_err(TransportError::from)?;

        // 2. Expect GetReady
        match read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::GetReady(_) => {}
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
        }

        let mut total_received = 0u64;
        loop {
            match read_next_frame(&mut stream)
                .await
                .map_err(TransportError::from)?
            {
                TransferFrame::NewEntry(header) => {
                    let local_path = local_root.join(&header.path);
                    if header.is_dir {
                        tokio::fs::create_dir_all(&local_path).await.map_err(|e| {
                            ClientError::FileIo {
                                operation: "create local directory",
                                path: local_path.clone(),
                                source: e,
                            }
                        })?;
                    } else {
                        if let Some(parent) = local_path.parent() {
                            let _ = tokio::fs::create_dir_all(parent).await;
                        }

                        let temp_path = temp_transfer_path(&local_path);
                        let mut dest = tokio::fs::File::create(&temp_path).await.map_err(|e| {
                            ClientError::FileIo {
                                operation: "create temp download file",
                                path: temp_path.clone(),
                                source: e,
                            }
                        })?;

                        loop {
                            match read_next_frame(&mut stream)
                                .await
                                .map_err(TransportError::from)?
                            {
                                TransferFrame::GetChunk(chunk) => {
                                    dest.write_all(&chunk).await.map_err(|e| {
                                        ClientError::FileIo {
                                            operation: "write to temp download file",
                                            path: temp_path.clone(),
                                            source: e,
                                        }
                                    })?;
                                    total_received += chunk.len() as u64;
                                    on_progress(TransferProgress::new(total_received, 0));
                                }
                                TransferFrame::EntryComplete(_) => break,
                                other => {
                                    let _ = tokio::fs::remove_file(&temp_path).await;
                                    return Err(ClientError::DownloadFailed {
                                        details: format!(
                                            "unexpected frame during recursive download stream: {other:?}"
                                        ),
                                    }
                                    .into());
                                }
                            }
                        }
                        dest.flush().await.ok();
                        drop(dest);
                        persist_temp_file(&temp_path, &local_path).await?;

                        if let Some(mode) = header.mode {
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                let _ = tokio::fs::set_permissions(
                                    &local_path,
                                    std::fs::Permissions::from_mode(mode),
                                )
                                .await;
                            }
                        }
                    }
                }
                TransferFrame::GetComplete(complete) => {
                    let _ = complete;
                    return Ok(());
                }
                TransferFrame::Error(details) => {
                    return Err(ClientError::TransferRejected {
                        details: details.to_string(),
                    }
                    .into());
                }
                other => {
                    return Err(ClientError::DownloadFailed {
                        details: format!("unexpected frame during recursive download: {other:?}"),
                    }
                    .into());
                }
            }
        }
    }

    /// Best-effort check if a remote path is a directory.
    async fn is_remote_dir(&mut self, path: impl AsRef<std::path::Path>) -> Result<bool> {
        let path_str = path.as_ref().display().to_string();
        // Use an extremely robust check with unique results, making it the ONLY thing on the line.
        let check_cmd = format!(
            "if [ -d \"{}\" ]; then echo '___IROSH_IS_DIR_YES___'; else echo '___IROSH_IS_DIR_NO___'; fi",
            path_str
        );
        let output = self.capture_exec(&check_cmd).await?;
        let stdout_str = String::from_utf8_lossy(&output.stdout);

        // Search for exact line match to avoid MOTD issues
        let is_dir = stdout_str
            .lines()
            .any(|l| l.trim() == "___IROSH_IS_DIR_YES___");
        Ok(is_dir)
    }
}

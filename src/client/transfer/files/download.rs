//! Client file download implementation.
use tokio::io::AsyncWriteExt;
use tracing::warn;

use crate::client::{Session, TransferProgress};
use crate::error::{ClientError, Result, TransportError};
use crate::transport::transfer::{
    BlobGetRequest, TransferFrame, read_next_frame, write_blob_get_request, write_get_request,
};
use futures_util::StreamExt;
use iroh_blobs::{BlobFormat, Hash};
use std::str::FromStr;

use crate::client::transfer::store::{persist_temp_file, temp_transfer_path};

impl Session {
    /// Downloads one remote file or directory to a local path.
    ///
    /// If `remote` is a directory, it will be downloaded recursively.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or is rejected by the remote peer.
    pub async fn download(
        &mut self,
        remote: impl AsRef<std::path::Path>,
        local: impl AsRef<std::path::Path>,
        recursive: bool,
    ) -> Result<()> {
        self.download_with_progress(remote, local, recursive, |_| {})
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
    pub async fn download_with_progress<F>(
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
            self.download_dir_with_progress(remote, local, on_progress)
                .await
        } else {
            self.download_file_with_progress(remote, local, on_progress)
                .await
        }
    }

    /// Downloads a remote file using content-addressed blobs.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or is rejected by the remote peer.
    pub async fn download_blob<F>(
        &self,
        remote: impl AsRef<std::path::Path>,
        local: impl AsRef<std::path::Path>,
        mut on_progress: F,
    ) -> Result<iroh_blobs::Hash>
    where
        F: FnMut(TransferProgress) + Clone + Send + 'static,
    {
        let remote = remote.as_ref();
        let local = local.as_ref();

        // 1. Open transfer stream and negotiate capabilities
        let (mut stream, max_kind) = self
            .open_negotiated_stream("blob download unavailable")
            .await?;

        if max_kind < 18 {
            return Err(ClientError::DownloadFailed {
                details: "remote peer does not support blob transfers; use download() instead"
                    .to_string(),
            }
            .into());
        }

        write_blob_get_request(
            &mut stream,
            &BlobGetRequest {
                path: remote.display().to_string(),
            },
        )
        .await
        .map_err(TransportError::from)?;

        // 2. Expect BlobGetReady with the hash
        let (hash_str, format_str, expected_size) = match read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::BlobGetReady(ready) => (ready.hash, ready.format, ready.size),
            TransferFrame::Error(failure) => {
                return Err(ClientError::TransferRejected { failure }.into());
            }
            other => {
                return Err(ClientError::DownloadFailed {
                    details: format!("unexpected preflight frame: {other:?}"),
                }
                .into());
            }
        };

        let hash = Hash::from_str(&hash_str).map_err(|e| ClientError::DownloadFailed {
            details: format!("invalid hash from server: {e}"),
        })?;

        let format = if format_str == "hashseq" {
            BlobFormat::HashSeq
        } else {
            BlobFormat::Raw
        };

        // 3. Download via iroh-blobs
        let conn = self
            .connection
            .clone()
            .ok_or_else(|| ClientError::DownloadFailed {
                details: "not connected to any peer".to_string(),
            })?;

        let mut fetch_stream = self.blobs.remote().fetch(conn, hash).stream();

        while let Some(item) = fetch_stream.next().await {
            match item {
                iroh_blobs::api::remote::GetProgressItem::Progress(bytes) => {
                    // Note: fetch Progress item is cumulative payload bytes
                    on_progress(TransferProgress::new(bytes, expected_size));
                }
                iroh_blobs::api::remote::GetProgressItem::Error(e) => {
                    return Err(ClientError::DownloadFailed {
                        details: format!("blob download failed: {e}"),
                    }
                    .into());
                }
                iroh_blobs::api::remote::GetProgressItem::Done(_) => {}
            }
        }

        // 4. Export from blobs store to local path
        if format == BlobFormat::HashSeq {
            // Recreate directory
            tokio::fs::create_dir_all(local).await?;

            // Load collection
            let collection = iroh_blobs::format::collection::Collection::load(hash, &*self.blobs)
                .await
                .map_err(|e| ClientError::DownloadFailed {
                    details: format!("failed to parse collection {hash}: {e}"),
                })?;

            for (name, item_hash) in collection.iter() {
                let item_path = local.join(name);
                if let Some(parent) = item_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }

                if let Err(e) = self
                    .blobs
                    .blobs()
                    .export(*item_hash, item_path.clone())
                    .await
                {
                    return Err(ClientError::FileIo {
                        operation: "export collection item",
                        path: item_path.clone(),
                        source: std::io::Error::other(e.to_string()),
                    }
                    .into());
                }
            }
        } else if let Err(e) = self.blobs.blobs().export(hash, local).await {
            return Err(ClientError::FileIo {
                operation: "export blob to local file",
                path: local.to_path_buf(),
                source: std::io::Error::other(e.to_string()),
            }
            .into());
        }

        Ok(hash)
    }

    /// Downloads one remote file to a local path on a separate authenticated stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or is rejected by the remote peer.
    pub async fn download_file(
        &mut self,
        remote: impl AsRef<std::path::Path>,
        local: impl AsRef<std::path::Path>,
    ) -> Result<()> {
        self.download_file_with_progress(remote, local, |_| {})
            .await
    }

    /// Downloads one remote file and reports progress synchronously through the callback.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or is rejected by the remote peer.
    pub async fn download_file_with_progress<F>(
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

        let (mut stream, _max_kind) = self.open_negotiated_stream("download unavailable").await?;

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
            TransferFrame::Error(failure) => {
                return Err(ClientError::TransferRejected { failure }.into());
            }
            other => {
                return Err(ClientError::DownloadFailed {
                    details: format!(
                        "unexpected preflight frame for {}: {other:?}",
                        remote.display()
                    ),
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
                TransferFrame::Error(failure) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(ClientError::TransferRejected { failure }.into());
                }
                other => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(ClientError::DownloadFailed {
                        details: format!(
                            "unexpected data stream frame for {}: {other:?}",
                            remote.display()
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
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or is rejected by the remote peer.
    pub async fn download_dir_with_progress<F>(
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

        let (mut stream, _max_kind) = self
            .open_negotiated_stream("recursive download unavailable")
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
            TransferFrame::Error(failure) => {
                return Err(ClientError::TransferRejected { failure }.into());
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
                    let sanitized_rel =
                        crate::transport::transfer::sanitize_remote_path(&header.path)?;
                    let local_path = local_root.join(sanitized_rel);
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
                        dest.flush().await.unwrap_or_else(|e| {
                            warn!("Failed to flush temp file {:?}: {e}", temp_path);
                        });
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
                TransferFrame::Error(failure) => {
                    return Err(ClientError::TransferRejected { failure }.into());
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

        let mut stream = self
            .open_transfer_stream("directory check unavailable")
            .await?;
        crate::transport::transfer::write_exists_request(
            &mut stream,
            &crate::transport::transfer::ExistsRequest { path: path_str },
        )
        .await
        .map_err(crate::error::TransportError::from)?;

        match crate::transport::transfer::read_next_frame(&mut stream)
            .await
            .map_err(crate::error::TransportError::from)?
        {
            crate::transport::transfer::TransferFrame::ExistsResponse(res) => Ok(res.is_dir),
            crate::transport::transfer::TransferFrame::Error(failure) => {
                Err(crate::error::ClientError::TransferRejected { failure }.into())
            }
            other => Err(crate::error::ClientError::DownloadFailed {
                details: format!("unexpected frame during is_dir check: {other:?}"),
            }
            .into()),
        }
    }
}

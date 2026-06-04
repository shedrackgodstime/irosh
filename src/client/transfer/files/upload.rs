//! Client file upload implementation.
use tokio::io::AsyncReadExt;

use crate::client::{Session, TransferProgress};
use crate::error::{ClientError, Result, TransportError};
use crate::transport::transfer::{
    BlobPutRequest, MAX_CHUNK_BYTES, TransferComplete, TransferFrame, TransferReady,
    read_next_frame, write_blob_put_request, write_put_chunk, write_put_complete,
    write_put_request,
};
use futures_util::StreamExt;
use iroh_blobs::BlobFormat;

impl Session {
    /// Uploads one local file or directory to the remote peer.
    ///
    /// If `local` is a directory, it will be uploaded recursively.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or is rejected by the remote peer.
    pub async fn upload(
        &mut self,
        local: impl AsRef<std::path::Path>,
        remote: impl AsRef<std::path::Path>,
        recursive: bool,
    ) -> Result<()> {
        self.upload_with_progress(local, remote, recursive, |_| {})
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
    pub async fn upload_with_progress<F>(
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
            self.upload_dir_with_progress(local, remote, on_progress)
                .await
        } else {
            self.upload_file_with_progress(local, remote, on_progress)
                .await
        }
    }

    /// Uploads a file using content-addressed blobs with progress reporting.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or is rejected by the remote peer.
    pub async fn upload_blob<F>(
        &self,
        local: impl AsRef<std::path::Path>,
        remote: impl AsRef<std::path::Path>,
        mut on_progress: F,
    ) -> Result<iroh_blobs::Hash>
    where
        F: FnMut(TransferProgress) + Clone + Send + 'static,
    {
        let local = local.as_ref();
        let remote = remote.as_ref();

        if tokio::fs::metadata(local).await.is_ok_and(|m| m.is_dir()) {
            return self.upload_blob_dir(local, remote, on_progress).await;
        }

        // Single file: add to store, send data over SSH.
        let mut add_stream = self
            .blobs
            .blobs()
            .add_path_with_opts(iroh_blobs::api::blobs::AddPathOptions {
                path: local.to_path_buf(),
                format: BlobFormat::Raw,
                mode: iroh_blobs::api::blobs::ImportMode::Copy,
            })
            .stream()
            .await;

        let mut hash = None;
        let mut total_size = 0u64;

        while let Some(item) = add_stream.next().await {
            match item {
                iroh_blobs::api::blobs::AddProgressItem::Done(tag) => {
                    hash = Some(tag.hash());
                    let status = self.blobs.blobs().status(tag.hash()).await.map_err(|e| {
                        ClientError::UploadFailed {
                            details: format!("failed to get blob status: {e}"),
                        }
                    })?;
                    total_size = match status {
                        iroh_blobs::api::proto::BlobStatus::Complete { size } => size,
                        _ => 0,
                    };
                }
                iroh_blobs::api::blobs::AddProgressItem::Error(e) => {
                    return Err(ClientError::UploadFailed {
                        details: format!("failed to add file to blobs store: {e}"),
                    }
                    .into());
                }
                _ => {}
            }
        }

        let hash = hash.ok_or_else(|| ClientError::UploadFailed {
            details: "add_path finished without hash".to_string(),
        })?;

        let mut stream = self.open_transfer_stream("blob upload unavailable").await?;

        write_blob_put_request(
            &mut stream,
            &BlobPutRequest {
                path: remote.display().to_string(),
                hash: hash.to_string(),
                format: "raw".to_string(),
                size: total_size,
            },
        )
        .await
        .map_err(TransportError::from)?;

        // Wait for PutReady
        match read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::PutReady(ready) if ready.size == total_size => {}
            TransferFrame::PutReady(ready) => {
                return Err(ClientError::UploadFailed {
                    details: format!(
                        "server acknowledged size {} but we have {}",
                        ready.size, total_size
                    ),
                }
                .into());
            }
            TransferFrame::Error(failure) => {
                return Err(ClientError::TransferRejected { failure }.into());
            }
            other => {
                return Err(ClientError::UploadFailed {
                    details: format!("unexpected frame: {other:?}"),
                }
                .into());
            }
        }

        // Send blob data
        let mut reader = self.blobs.blobs().reader(hash);
        let mut buffer = vec![0u8; MAX_CHUNK_BYTES];
        let mut sent = 0u64;

        loop {
            let count = reader
                .read(&mut buffer)
                .await
                .map_err(|e| ClientError::UploadFailed {
                    details: format!("failed to read blob from store: {e}"),
                })?;
            if count == 0 {
                break;
            }
            sent += count as u64;
            write_put_chunk(&mut stream, &buffer[..count])
                .await
                .map_err(TransportError::from)?;
            on_progress(TransferProgress::new(sent, total_size));
        }

        write_put_complete(&mut stream, &TransferComplete { size: sent })
            .await
            .map_err(TransportError::from)?;

        // Wait for server confirmation
        match read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::PutComplete(complete) if complete.size == sent => Ok(hash),
            TransferFrame::PutComplete(complete) => Err(ClientError::UploadFailed {
                details: format!(
                    "server confirmed {} bytes but we sent {sent}",
                    complete.size
                ),
            }
            .into()),
            TransferFrame::Error(failure) => Err(ClientError::TransferRejected { failure }.into()),
            other => Err(ClientError::UploadFailed {
                details: format!("unexpected completion frame: {other:?}"),
            }
            .into()),
        }
    }

    /// Upload a directory as a content-addressed blob.
    ///
    /// Walks the directory, adds each file to the local FsStore, builds a
    /// [`iroh_blobs::format::collection::Collection`], stores it, then sends
    /// the files recursively over SSH. The server reconstructs the directory
    /// on disk; the returned hash is deterministic and will match the hash
    /// computed by the server when `download_blob` is called later.
    async fn upload_blob_dir<F>(
        &self,
        local: &std::path::Path,
        remote: &std::path::Path,
        mut on_progress: F,
    ) -> Result<iroh_blobs::Hash>
    where
        F: FnMut(TransferProgress),
    {
        // 1. Walk directory, add each file to store, build collection
        let mut collection = iroh_blobs::format::collection::Collection::default();
        let mut total_size = 0u64;

        let mut entries = Vec::new();
        collect_files_recursive(local, local, &mut entries).map_err(|e| ClientError::FileIo {
            operation: "read directory recursively",
            path: local.to_path_buf(),
            source: e,
        })?;
        entries.sort();

        for relative in &entries {
            let file_path = local.join(relative);
            let data = tokio::fs::read(&file_path)
                .await
                .map_err(|e| ClientError::FileIo {
                    operation: "read file",
                    path: file_path.clone(),
                    source: e,
                })?;
            let mut add_stream = self.blobs.blobs().add_bytes(data).stream().await;
            let mut file_hash = None;
            while let Some(item) = add_stream.next().await {
                match item {
                    iroh_blobs::api::blobs::AddProgressItem::Done(tag) => {
                        file_hash = Some(tag.hash());
                    }
                    iroh_blobs::api::blobs::AddProgressItem::Error(e) => {
                        return Err(ClientError::UploadFailed {
                            details: format!("failed to add file to store: {e}"),
                        }
                        .into());
                    }
                    _ => {}
                }
            }
            let file_hash = file_hash.ok_or_else(|| ClientError::UploadFailed {
                details: "add_bytes finished without hash".to_string(),
            })?;
            collection.push(relative.clone(), file_hash);
        }

        // 2. Store collection blobs; last one is the root hash
        let mut root_hash = None;
        for blob_data in collection.to_blobs() {
            let blob_len = blob_data.len() as u64;
            total_size += blob_len;
            let mut add_stream = self
                .blobs
                .blobs()
                .add_bytes(blob_data.to_vec())
                .stream()
                .await;
            while let Some(item) = add_stream.next().await {
                match item {
                    iroh_blobs::api::blobs::AddProgressItem::Done(tag) => {
                        root_hash = Some(tag.hash());
                    }
                    iroh_blobs::api::blobs::AddProgressItem::Error(e) => {
                        return Err(ClientError::UploadFailed {
                            details: format!("failed to add collection blob: {e}"),
                        }
                        .into());
                    }
                    _ => {}
                }
            }
        }

        let root_hash = root_hash.ok_or_else(|| ClientError::UploadFailed {
            details: "collection to_blobs produced no blobs".to_string(),
        })?;

        // 3. Upload files recursively over SSH (server reconstructs directory)
        let mut stream = self.open_transfer_stream("blob dir upload").await?;

        write_blob_put_request(
            &mut stream,
            &BlobPutRequest {
                path: remote.display().to_string(),
                hash: root_hash.to_string(),
                format: "hashseq".to_string(),
                size: total_size,
            },
        )
        .await
        .map_err(TransportError::from)?;

        // The server sends PutReady → ignore size check (size is the
        // combined blob size, not the raw file stream size)
        match read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::PutReady(_) => {}
            TransferFrame::Error(failure) => {
                return Err(ClientError::TransferRejected { failure }.into());
            }
            other => {
                return Err(ClientError::UploadFailed {
                    details: format!("unexpected frame: {other:?}"),
                }
                .into());
            }
        }

        // 4. Send each blob as length-prefixed data
        // Blobs are sent in the order: child files, then collection blobs
        // (metadata, links). The collection hash is the links blob hash.
        let mut sent = 0u64;

        // We need the blobs in the right order. Child file data first,
        // in the same order they were pushed to the collection.
        for (_, child_hash) in collection.iter() {
            sent += Self::send_blob_from_store(
                &self.blobs,
                &mut stream,
                *child_hash,
                &mut on_progress,
                total_size,
                sent,
            )
            .await?;
        }

        // Then collection blobs (from to_blobs: metadata, links)
        for blob_data in collection.to_blobs() {
            sent +=
                Self::send_raw_blob(&mut stream, &blob_data, &mut on_progress, total_size, sent)
                    .await?;
        }

        write_put_complete(&mut stream, &TransferComplete { size: sent })
            .await
            .map_err(TransportError::from)?;

        match read_next_frame(&mut stream)
            .await
            .map_err(TransportError::from)?
        {
            TransferFrame::PutComplete(complete) if complete.size == sent => Ok(root_hash),
            TransferFrame::PutComplete(complete) => Err(ClientError::UploadFailed {
                details: format!(
                    "server confirmed {} bytes but we sent {sent}",
                    complete.size
                ),
            }
            .into()),
            TransferFrame::Error(failure) => Err(ClientError::TransferRejected { failure }.into()),
            other => Err(ClientError::UploadFailed {
                details: format!("unexpected completion frame: {other:?}"),
            }
            .into()),
        }
    }

    /// Send a single blob from the store over the SSH stream.
    async fn send_blob_from_store<F>(
        blobs: &iroh_blobs::store::fs::FsStore,
        stream: &mut crate::transport::stream::IrohDuplex,
        hash: iroh_blobs::Hash,
        on_progress: &mut F,
        total_size: u64,
        cumulative_before: u64,
    ) -> Result<u64>
    where
        F: FnMut(TransferProgress),
    {
        let mut reader = blobs.blobs().reader(hash);
        let mut data = Vec::new();
        reader
            .read_to_end(&mut data)
            .await
            .map_err(|e| ClientError::UploadFailed {
                details: format!("failed to read blob from store: {e}"),
            })?;
        Self::send_raw_blob(stream, &data, on_progress, total_size, cumulative_before).await
    }

    /// Send raw bytes over the SSH stream with length prefix.
    async fn send_raw_blob<F>(
        stream: &mut crate::transport::stream::IrohDuplex,
        data: &[u8],
        on_progress: &mut F,
        total_size: u64,
        cumulative_before: u64,
    ) -> Result<u64>
    where
        F: FnMut(TransferProgress),
    {
        let len = data.len() as u64;
        // Send 8-byte length prefix (big-endian)
        let len_bytes = len.to_be_bytes();
        write_put_chunk(stream, &len_bytes)
            .await
            .map_err(TransportError::from)?;

        // Send blob data in chunks
        let mut offset = 0usize;
        while offset < data.len() {
            let end = (offset + MAX_CHUNK_BYTES).min(data.len());
            write_put_chunk(stream, &data[offset..end])
                .await
                .map_err(TransportError::from)?;
            offset = end;
        }
        let written = cumulative_before + 8 + len;
        on_progress(TransferProgress::new(written, total_size));
        Ok(8 + len)
    }

    /// Uploads one local file to the remote peer on a separate authenticated stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or is rejected by the remote peer.
    pub async fn upload_file(
        &mut self,
        local: impl AsRef<std::path::Path>,
        remote: impl AsRef<std::path::Path>,
    ) -> Result<()> {
        self.upload_file_with_progress(local, remote, |_| {}).await
    }

    /// Uploads one local file and reports progress synchronously through the callback.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or is rejected by the remote peer.
    pub async fn upload_file_with_progress<F>(
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
            TransferFrame::Error(failure) => {
                return Err(ClientError::TransferRejected { failure }.into());
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
            TransferFrame::Error(failure) => Err(ClientError::TransferRejected { failure }.into()),
            other => Err(ClientError::UploadFailed {
                details: format!("unexpected completion frame: {other:?}"),
            }
            .into()),
        }
    }

    /// Uploads a directory recursively to the remote peer.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or is rejected by the remote peer.
    pub async fn upload_dir_with_progress<F>(
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
            TransferFrame::Error(failure) => {
                return Err(ClientError::TransferRejected { failure }.into());
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
            TransferFrame::PutComplete(complete) if complete.size == total_sent => Ok(()),
            TransferFrame::PutComplete(complete) => Err(ClientError::UploadFailed {
                details: format!(
                    "remote reported {} bytes saved, expected {total_sent}",
                    complete.size
                ),
            }
            .into()),
            TransferFrame::Error(failure) => Err(ClientError::TransferRejected { failure }.into()),
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

/// Recursively collect all files under `dir`, returning their paths relative to `dir`.
fn collect_files_recursive(
    dir: &std::path::Path,
    base: &std::path::Path,
    entries: &mut Vec<String>,
) -> std::io::Result<()> {
    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;
        if entry.file_type().is_dir() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(base)
            .map_err(|_| std::io::Error::other("path prefix mismatch"))?;
        entries.push(relative.to_string_lossy().to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::reject_recursive_symlink;
    #[cfg(unix)]
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

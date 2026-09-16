//! Content-addressed blob upload protocol.
//!
//! Runs over the dedicated transfer side-stream: the client adds files to the
//! local [`iroh_blobs::store::fs::FsStore`], stores a
//! [`iroh_blobs::format::collection::Collection`] for directories, then sends
//! the blobs to the server using the blob-put request/response frames.

use tokio::io::AsyncReadExt;

use crate::client::{Session, TransferProgress};
use crate::error::{ClientError, Result, TransportError};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    BlobPutRequest, MAX_CHUNK_BYTES, TransferComplete, TransferFrame, read_next_frame,
    write_blob_put_request, write_put_chunk, write_put_complete,
};
use futures_util::StreamExt;
use iroh_blobs::BlobFormat;
use iroh_blobs::store::fs::FsStore;

use super::upload::collect_files_recursive;

impl Session {
    /// Uploads a file using content-addressed blobs with progress reporting.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or is rejected by the remote peer.
    #[must_use]
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
                path: crate::transport::transfer::normalize_path_separators(
                    &remote.display().to_string(),
                ),
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

        for relative in entries {
            let file_path = local.join(&relative);
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
            collection.push(relative, file_hash);
        }

        // 2. Store collection blobs; last one is the root hash
        let mut root_hash = None;
        for blob_data in collection.to_blobs() {
            let blob_len = blob_data.len() as u64;
            total_size += blob_len;
            let mut add_stream = self.blobs.blobs().add_bytes(blob_data).stream().await;
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
                path: crate::transport::transfer::normalize_path_separators(
                    &remote.display().to_string(),
                ),
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
        blobs: &FsStore,
        stream: &mut IrohDuplex,
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
        stream: &mut IrohDuplex,
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
}

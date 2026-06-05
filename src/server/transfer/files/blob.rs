//! Blob-based file transfer.
use iroh_blobs::{BlobFormat, Hash};
use std::str::FromStr;
use tracing::{debug, info};

use crate::error::{Result, ServerError};
use crate::server::transfer::state::{ConnectionShellState, ShellContext};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    BlobGetReady, BlobGetRequest, BlobPutRequest, TransferComplete, TransferFrame, TransferReady,
    read_next_frame, write_blob_get_ready, write_put_complete, write_put_ready,
};
use futures_util::StreamExt;
use tokio::io::AsyncReadExt;

#[must_use]
pub async fn handle_blob_put_request(
    stream: &mut IrohDuplex,
    _connection: iroh::endpoint::Connection,
    request: BlobPutRequest,
    context: ShellContext,
    shell_state: &ConnectionShellState,
) -> Result<()> {
    info!(
        "Handling BlobPutRequest: hash={}, path={}",
        request.hash, request.path
    );

    let expected_hash = Hash::from_str(&request.hash).map_err(|e| ServerError::TransferFailed {
        failure: crate::transport::transfer::TransferFailure::new(
            crate::transport::transfer::TransferFailureCode::PathInvalid,
            format!("invalid hash: {e}"),
        ),
    })?;

    let format = if request.format.to_lowercase() == "hashseq" {
        BlobFormat::HashSeq
    } else {
        BlobFormat::Raw
    };

    let target_path = context.resolve_path(&request.path, shell_state).await?;
    debug!("Resolved target path: {}", target_path.display());

    // 1. Send PutReady to acknowledge the request
    write_put_ready(
        stream,
        &TransferReady {
            size: request.size,
            mode: None,
        },
    )
    .await
    .map_err(|e| ServerError::TransferFailed {
        failure: crate::transport::transfer::TransferFailure::new(
            crate::transport::transfer::TransferFailureCode::Internal,
            format!("failed to send ready: {e}"),
        ),
    })?;

    // 2. Receive blob data.
    // For Raw format: all chunks form one blob → add_slice → verify hash → export.
    // For HashSeq format: length-prefixed blobs → each added to store individually.
    let mut received = 0u64;

    if format == BlobFormat::HashSeq {
        // Parse length-prefixed blobs from the data stream
        let mut expected_remaining = 0u64;
        let mut current_blob = Vec::new();
        let mut blob_count = 0u64;

        loop {
            match read_next_frame(stream)
                .await
                .map_err(|e| ServerError::TransferFailed {
                    failure: crate::transport::transfer::TransferFailure::new(
                        crate::transport::transfer::TransferFailureCode::Internal,
                        format!("failed to read frame: {e}"),
                    ),
                })? {
                TransferFrame::PutChunk(data) => {
                    if expected_remaining == 0 {
                        // No current blob — this chunk starts a new length prefix
                        // The first 8 bytes are the big-endian length of the next blob
                        if data.len() < 8 {
                            return Err(ServerError::TransferFailed {
                                failure: crate::transport::transfer::TransferFailure::new(
                                    crate::transport::transfer::TransferFailureCode::Internal,
                                    "truncated length prefix".to_string(),
                                ),
                            }
                            .into());
                        }
                        let (len_bytes, blob_start) = data.split_at(8);
                        expected_remaining = u64::from_be_bytes([
                            len_bytes[0],
                            len_bytes[1],
                            len_bytes[2],
                            len_bytes[3],
                            len_bytes[4],
                            len_bytes[5],
                            len_bytes[6],
                            len_bytes[7],
                        ]);
                        if let Ok(cap) = usize::try_from(expected_remaining) {
                            current_blob.reserve(cap);
                        }
                        current_blob.extend_from_slice(blob_start);
                        expected_remaining -= blob_start.len() as u64;
                    } else {
                        current_blob.extend_from_slice(&data);
                        expected_remaining -= data.len() as u64;
                    }
                    received += data.len() as u64;

                    if expected_remaining == 0 && !current_blob.is_empty() {
                        // Complete blob received — add to store
                        let mut add_stream = shell_state
                            .blobs
                            .blobs()
                            .add_bytes(std::mem::take(&mut current_blob))
                            .stream()
                            .await;
                        while let Some(item) = add_stream.next().await {
                            if let iroh_blobs::api::blobs::AddProgressItem::Error(e) = item {
                                return Err(ServerError::TransferFailed {
                                    failure: crate::transport::transfer::TransferFailure::new(
                                        crate::transport::transfer::TransferFailureCode::Internal,
                                        format!("failed to add blob to store: {e}"),
                                    ),
                                }
                                .into());
                            }
                        }
                        blob_count += 1;
                    }
                }
                TransferFrame::PutComplete(_) => break,
                TransferFrame::Error(failure) => {
                    return Err(ServerError::TransferFailed { failure }.into());
                }
                other => {
                    return Err(ServerError::TransferFailed {
                        failure: crate::transport::transfer::TransferFailure::new(
                            crate::transport::transfer::TransferFailureCode::UnexpectedFrame,
                            format!("unexpected frame during blob data: {other:?}"),
                        ),
                    }
                    .into());
                }
            }
        }

        if blob_count == 0 {
            return Err(ServerError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::Internal,
                    "no blobs received for hashseq".to_string(),
                ),
            }
            .into());
        }

        // Verify that the collection blob (last one stored) has the expected hash.
        // We don't have the hash directly, but we can verify by checking if the
        // expected hash exists in the store.
        let status = shell_state
            .blobs
            .blobs()
            .status(expected_hash)
            .await
            .map_err(|e| ServerError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::Internal,
                    format!("failed to check blob status: {e}"),
                ),
            })?;
        if let iroh_blobs::api::proto::BlobStatus::NotFound = status {
            return Err(ServerError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::Internal,
                    format!("collection hash {expected_hash} not found in store after upload"),
                ),
            }
            .into());
        }

        // Export the collection to the target path
        export_collection(shell_state, expected_hash, &target_path).await?;
    } else {
        // Raw format: all chunks form one blob
        let mut all_data = Vec::new();
        loop {
            match read_next_frame(stream)
                .await
                .map_err(|e| ServerError::TransferFailed {
                    failure: crate::transport::transfer::TransferFailure::new(
                        crate::transport::transfer::TransferFailureCode::Internal,
                        format!("failed to read frame: {e}"),
                    ),
                })? {
                TransferFrame::PutChunk(data) => {
                    all_data.extend_from_slice(&data);
                    received += data.len() as u64;
                }
                TransferFrame::PutComplete(_) => break,
                TransferFrame::Error(failure) => {
                    return Err(ServerError::TransferFailed { failure }.into());
                }
                other => {
                    return Err(ServerError::TransferFailed {
                        failure: crate::transport::transfer::TransferFailure::new(
                            crate::transport::transfer::TransferFailureCode::UnexpectedFrame,
                            format!("unexpected frame during blob data: {other:?}"),
                        ),
                    }
                    .into());
                }
            }
        }

        // Add to store and verify hash
        let mut add_stream = shell_state.blobs.blobs().add_bytes(all_data).stream().await;

        let mut actual_hash = None;
        while let Some(item) = add_stream.next().await {
            match item {
                iroh_blobs::api::blobs::AddProgressItem::Done(tag) => {
                    actual_hash = Some(tag.hash());
                }
                iroh_blobs::api::blobs::AddProgressItem::Error(e) => {
                    return Err(ServerError::TransferFailed {
                        failure: crate::transport::transfer::TransferFailure::new(
                            crate::transport::transfer::TransferFailureCode::Internal,
                            format!("failed to add blob to store: {e}"),
                        ),
                    }
                    .into());
                }
                _ => {}
            }
        }

        let actual_hash = actual_hash.ok_or_else(|| ServerError::TransferFailed {
            failure: crate::transport::transfer::TransferFailure::new(
                crate::transport::transfer::TransferFailureCode::Internal,
                "add_bytes finished without hash".to_string(),
            ),
        })?;

        if actual_hash != expected_hash {
            return Err(ServerError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::Internal,
                    format!("hash mismatch: expected {expected_hash}, got {actual_hash}"),
                ),
            }
            .into());
        }

        // Export single file
        if let Err(e) = shell_state
            .blobs
            .blobs()
            .export(actual_hash, target_path.clone())
            .await
        {
            return Err(ServerError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::Internal,
                    format!("failed to export blob to {}: {}", target_path.display(), e),
                ),
            }
            .into());
        }
    }

    // 5. Send completion frame
    write_put_complete(stream, &TransferComplete { size: received })
        .await
        .map_err(|e| {
            ServerError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::Internal,
                    format!("failed to send completion: {e}"),
                ),
            }
            .into()
        })
}

#[must_use]
pub async fn handle_blob_get_request(
    stream: &mut IrohDuplex,
    _connection: iroh::endpoint::Connection,
    request: BlobGetRequest,
    context: ShellContext,
    shell_state: &ConnectionShellState,
) -> Result<()> {
    info!("Handling BlobGetRequest: path={}", request.path);

    let target_path = context.resolve_path(&request.path, shell_state).await?;

    if !context.path_exists(&request.path).await? {
        return Err(ServerError::TransferFailed {
            failure: crate::transport::transfer::TransferFailure::new(
                crate::transport::transfer::TransferFailureCode::NotFound,
                format!("file not found: {}", request.path),
            ),
        }
        .into());
    }

    if context.is_dir(&request.path).await? {
        // Directory: walk, add each file to store, build collection, export
        let (hash, total_size) = add_directory_to_store(shell_state, &target_path).await?;
        write_blob_get_ready(
            stream,
            &BlobGetReady {
                hash: hash.to_string(),
                format: "hashseq".to_string(),
                size: total_size,
            },
        )
        .await
        .map_err(Into::into)
    } else {
        // Single file
        let mut add_stream = shell_state
            .blobs
            .blobs()
            .add_path_with_opts(iroh_blobs::api::blobs::AddPathOptions {
                path: target_path,
                format: BlobFormat::Raw,
                mode: iroh_blobs::api::blobs::ImportMode::Copy,
            })
            .stream()
            .await;

        let mut hash = None;
        let mut final_size = 0u64;

        while let Some(item) = add_stream.next().await {
            match item {
                iroh_blobs::api::blobs::AddProgressItem::Done(tag) => {
                    hash = Some(tag.hash());
                }
                iroh_blobs::api::blobs::AddProgressItem::Size(size) => {
                    final_size = size;
                }
                iroh_blobs::api::blobs::AddProgressItem::Error(e) => {
                    return Err(ServerError::TransferFailed {
                        failure: crate::transport::transfer::TransferFailure::new(
                            crate::transport::transfer::TransferFailureCode::Internal,
                            format!("failed to add file to server blobs: {e}"),
                        ),
                    }
                    .into());
                }
                _ => {}
            }
        }

        let hash = hash.ok_or_else(|| ServerError::TransferFailed {
            failure: crate::transport::transfer::TransferFailure::new(
                crate::transport::transfer::TransferFailureCode::Internal,
                "add_path finished without hash".to_string(),
            ),
        })?;

        write_blob_get_ready(
            stream,
            &BlobGetReady {
                hash: hash.to_string(),
                format: "raw".to_string(),
                size: final_size,
            },
        )
        .await
        .map_err(Into::into)
    }
}

/// Walk a directory, add each file to the store, build a [`Collection`],
/// store the collection blobs, and return the collection hash + total size.
async fn add_directory_to_store(
    shell_state: &ConnectionShellState,
    dir_path: &std::path::Path,
) -> Result<(iroh_blobs::Hash, u64)> {
    let mut collection = iroh_blobs::format::collection::Collection::default();
    let mut total_size = 0u64;

    let mut entries: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(dir_path) {
        let entry = entry.map_err(|e| ServerError::TransferFailed {
            failure: crate::transport::transfer::TransferFailure::new(
                crate::transport::transfer::TransferFailureCode::Internal,
                format!("failed to walk directory {}: {e}", dir_path.display()),
            ),
        })?;
        if entry.file_type().is_dir() {
            continue;
        }
        let relative =
            entry
                .path()
                .strip_prefix(dir_path)
                .map_err(|_| ServerError::TransferFailed {
                    failure: crate::transport::transfer::TransferFailure::new(
                        crate::transport::transfer::TransferFailureCode::Internal,
                        "path prefix mismatch".to_string(),
                    ),
                })?;
        entries.push(relative.to_string_lossy().to_string());
    }

    entries.sort();

    for relative in &entries {
        let file_path = dir_path.join(relative);
        let data = tokio::fs::read(&file_path)
            .await
            .map_err(|e| ServerError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::Internal,
                    format!("failed to read {}: {e}", file_path.display()),
                ),
            })?;
        let mut add_stream = shell_state.blobs.blobs().add_bytes(data).stream().await;
        let mut file_hash = None;
        while let Some(item) = add_stream.next().await {
            match item {
                iroh_blobs::api::blobs::AddProgressItem::Done(tag) => {
                    file_hash = Some(tag.hash());
                }
                iroh_blobs::api::blobs::AddProgressItem::Error(e) => {
                    return Err(ServerError::TransferFailed {
                        failure: crate::transport::transfer::TransferFailure::new(
                            crate::transport::transfer::TransferFailureCode::Internal,
                            format!("failed to add file to store: {e}"),
                        ),
                    }
                    .into());
                }
                _ => {}
            }
        }
        let file_hash = file_hash.ok_or_else(|| ServerError::TransferFailed {
            failure: crate::transport::transfer::TransferFailure::new(
                crate::transport::transfer::TransferFailureCode::Internal,
                "add_bytes finished without hash".to_string(),
            ),
        })?;
        collection.push(relative.clone(), file_hash);
    }

    // Store collection blobs; the last blob (links) is the collection root
    let mut root_hash = None;
    for data in collection.to_blobs() {
        let data_len = data.len() as u64;
        total_size += data_len;
        let mut add_stream = shell_state
            .blobs
            .blobs()
            .add_bytes(data.to_vec())
            .stream()
            .await;
        while let Some(item) = add_stream.next().await {
            match item {
                iroh_blobs::api::blobs::AddProgressItem::Done(tag) => {
                    root_hash = Some(tag.hash());
                }
                iroh_blobs::api::blobs::AddProgressItem::Error(e) => {
                    return Err(ServerError::TransferFailed {
                        failure: crate::transport::transfer::TransferFailure::new(
                            crate::transport::transfer::TransferFailureCode::Internal,
                            format!("failed to add collection blob: {e}"),
                        ),
                    }
                    .into());
                }
                _ => {}
            }
        }
    }

    let root_hash = root_hash.ok_or_else(|| ServerError::TransferFailed {
        failure: crate::transport::transfer::TransferFailure::new(
            crate::transport::transfer::TransferFailureCode::Internal,
            "collection to_blobs produced no blobs".to_string(),
        ),
    })?;

    Ok((root_hash, total_size))
}

async fn export_collection(
    shell_state: &ConnectionShellState,
    hash: Hash,
    target_path: &std::path::Path,
) -> Result<()> {
    // Recreate directory
    tokio::fs::create_dir_all(target_path).await?;

    // Load collection
    let mut reader = shell_state.blobs.blobs().reader(hash);
    let mut bytes = Vec::with_capacity(65536);
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| ServerError::TransferFailed {
            failure: crate::transport::transfer::TransferFailure::new(
                crate::transport::transfer::TransferFailureCode::Internal,
                format!("failed to read collection blob {hash}: {e}"),
            ),
        })?;

    // Load collection
    let collection = iroh_blobs::format::collection::Collection::load(hash, &*shell_state.blobs)
        .await
        .map_err(|e| ServerError::TransferFailed {
            failure: crate::transport::transfer::TransferFailure::new(
                crate::transport::transfer::TransferFailureCode::Internal,
                format!("failed to load collection {hash}: {e}"),
            ),
        })?;

    for (name, item_hash) in collection.iter() {
        let item_path = target_path.join(name);
        if let Some(parent) = item_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        if let Err(e) = shell_state
            .blobs
            .blobs()
            .export(*item_hash, item_path.clone())
            .await
        {
            return Err(ServerError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::Internal,
                    format!("failed to export collection item {name}: {e}"),
                ),
            }
            .into());
        }
    }

    Ok(())
}

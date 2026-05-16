use iroh_blobs::{BlobFormat, Hash};
use std::str::FromStr;
use tracing::{debug, info};

use crate::error::{Result, ServerError};
use crate::server::transfer::state::{ConnectionShellState, ShellContext};
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    BlobGetRequest, BlobPutRequest, TransferComplete, write_put_complete,
};

pub async fn handle_blob_put_request(
    stream: &mut IrohDuplex,
    connection: iroh::endpoint::Connection,
    request: BlobPutRequest,
    context: ShellContext,
    shell_state: &ConnectionShellState,
) -> Result<()> {
    info!(
        "Handling BlobPutRequest: hash={}, path={}",
        request.hash, request.path
    );

    let hash = Hash::from_str(&request.hash).map_err(|e| ServerError::TransferFailed {
        failure: crate::transport::transfer::TransferFailure::new(
            crate::transport::transfer::TransferFailureCode::PathInvalid,
            format!("invalid hash: {}", e),
        ),
    })?;

    let format = if request.format.to_lowercase() == "hashseq" {
        BlobFormat::HashSeq
    } else {
        BlobFormat::Raw
    };

    let target_path = context.resolve_path(&request.path, shell_state).await?;
    debug!("Resolved target path: {}", target_path.display());

    // 1. Fetch the blob(s) from the client.
    let stats = shell_state
        .blobs
        .remote()
        .fetch(
            connection.clone(),
            iroh_blobs::HashAndFormat { hash, format },
        )
        .await
        .map_err(|e| ServerError::TransferFailed {
            failure: crate::transport::transfer::TransferFailure::new(
                crate::transport::transfer::TransferFailureCode::Internal,
                format!("blob fetch failed: {}", e),
            ),
        })?;

    info!(
        "Blob download complete: {} bytes in {:?}",
        stats.payload_bytes_read, stats.elapsed
    );

    // 2. Export the blob(s) to the target file system path.
    if format == BlobFormat::HashSeq {
        // Handle collection export (directory)
        export_collection(shell_state, hash, &target_path).await?;
    } else {
        // Handle single file export
        if let Err(e) = shell_state
            .blobs
            .blobs()
            .export(hash, target_path.clone())
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

    // 3. Send completion frame
    let status =
        shell_state
            .blobs
            .blobs()
            .status(hash)
            .await
            .map_err(|e| ServerError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::Internal,
                    format!("failed to get blob status: {}", e),
                ),
            })?;

    let size = match status {
        iroh_blobs::api::proto::BlobStatus::Complete { size } => size,
        iroh_blobs::api::proto::BlobStatus::Partial { size } => size.unwrap_or(0),
        iroh_blobs::api::proto::BlobStatus::NotFound => 0,
    };

    write_put_complete(stream, &TransferComplete { size })
        .await
        .map_err(|e| {
            ServerError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::Internal,
                    format!("failed to send completion: {}", e),
                ),
            }
            .into()
        })
}

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
        return Err(ServerError::TransferFailed {
            failure: crate::transport::transfer::TransferFailure::new(
                crate::transport::transfer::TransferFailureCode::IsDirectory,
                format!("path is a directory: {}", request.path),
            ),
        }
        .into());
    }

    // 1. Add file or directory to server's local blobs store so it can be served
    use n0_future::StreamExt;
    let is_dir = context.is_dir(&request.path).await.unwrap_or(false);
    let format = if is_dir {
        BlobFormat::HashSeq
    } else {
        BlobFormat::Raw
    };

    let mut add_stream = shell_state
        .blobs
        .blobs()
        .add_path_with_opts(iroh_blobs::api::blobs::AddPathOptions {
            path: target_path,
            format,
            mode: iroh_blobs::api::blobs::ImportMode::Copy,
        })
        .stream()
        .await;

    let mut hash = None;
    let mut final_format = format;
    let mut final_size = 0u64;

    while let Some(item) = add_stream.next().await {
        match item {
            iroh_blobs::api::blobs::AddProgressItem::Done(tag) => {
                hash = Some(tag.hash());
                final_format = tag.format();
            }
            iroh_blobs::api::blobs::AddProgressItem::Size(size) => {
                final_size = size;
            }
            iroh_blobs::api::blobs::AddProgressItem::Error(e) => {
                return Err(ServerError::TransferFailed {
                    failure: crate::transport::transfer::TransferFailure::new(
                        crate::transport::transfer::TransferFailureCode::Internal,
                        format!("failed to add file to server blobs: {}", e),
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

    // 2. Respond with the hash, format, and size
    use crate::transport::transfer::{BlobGetReady, write_blob_get_ready};
    write_blob_get_ready(
        stream,
        &BlobGetReady {
            hash: hash.to_string(),
            format: if final_format == BlobFormat::HashSeq {
                "hashseq".to_string()
            } else {
                "raw".to_string()
            },
            size: final_size,
        },
    )
    .await
    .map_err(Into::into)
}

async fn export_collection(
    shell_state: &ConnectionShellState,
    hash: Hash,
    target_path: &std::path::Path,
) -> Result<()> {
    // Recreate directory
    tokio::fs::create_dir_all(target_path).await?;

    // Load collection
    use tokio::io::AsyncReadExt;
    let mut reader = shell_state.blobs.blobs().reader(hash);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| ServerError::TransferFailed {
            failure: crate::transport::transfer::TransferFailure::new(
                crate::transport::transfer::TransferFailureCode::Internal,
                format!("failed to read collection blob {}: {}", hash, e),
            ),
        })?;

    // Load collection
    let collection = iroh_blobs::format::collection::Collection::load(hash, &*shell_state.blobs)
        .await
        .map_err(|e| ServerError::TransferFailed {
            failure: crate::transport::transfer::TransferFailure::new(
                crate::transport::transfer::TransferFailureCode::Internal,
                format!("failed to load collection {}: {}", hash, e),
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
                    format!("failed to export collection item {}: {}", name, e),
                ),
            }
            .into());
        }
    }

    Ok(())
}

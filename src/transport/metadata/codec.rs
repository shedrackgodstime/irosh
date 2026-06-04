//! Metadata codec/framing.
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::types::{MetadataError, PeerMetadata};

/// Magic header for metadata frames.
#[cfg(test)]
pub(crate) const MAGIC: [u8; 4] = *b"IRMD";
#[cfg(not(test))]
pub(crate) const MAGIC: [u8; 4] = *b"IRMD";
/// Current metadata protocol version.
#[cfg(test)]
pub(crate) const VERSION: u8 = 2;
#[cfg(not(test))]
const VERSION: u8 = 2;
/// Client request asking the peer to send metadata.
const KIND_METADATA_REQUEST: u8 = 1;
/// Server response containing peer metadata.
#[cfg(test)]
pub(crate) const KIND_PEER_METADATA: u8 = 2;
#[cfg(not(test))]
const KIND_PEER_METADATA: u8 = 2;
/// Maximum bytes allowed for a metadata payload.
#[cfg(test)]
pub(crate) const MAX_METADATA_BYTES: usize = 8 * 1024;
#[cfg(not(test))]
const MAX_METADATA_BYTES: usize = 8 * 1024;

#[tracing::instrument(skip(writer))]
async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    kind: u8,
    payload: &[u8],
) -> Result<(), MetadataError> {
    tracing::trace!(len = payload.len(), kind, "Writing metadata frame");
    if payload.len() > MAX_METADATA_BYTES {
        return Err(MetadataError::PayloadTooLarge(payload.len()));
    }

    writer.write_all(&MAGIC).await?;
    writer.write_u8(VERSION).await?;
    writer.write_u8(kind).await?;
    // Reason: payload length is validated against a limit before this point.
    #[allow(clippy::cast_possible_truncation)]
    let len = payload.len() as u32;
    writer.write_u32(len).await?;
    writer.write_all(payload).await?;
    writer.flush().await?;

    Ok(())
}

#[tracing::instrument(skip(reader))]
async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<(u8, Vec<u8>), MetadataError> {
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic).await?;
    if magic != MAGIC {
        return Err(MetadataError::InvalidMagic);
    }

    let version = reader.read_u8().await?;
    if version != VERSION {
        return Err(MetadataError::UnsupportedVersion(version));
    }

    let kind = reader.read_u8().await?;
    if kind != KIND_PEER_METADATA && kind != KIND_METADATA_REQUEST {
        return Err(MetadataError::UnsupportedKind(kind));
    }

    let length = reader.read_u32().await? as usize;
    if length > MAX_METADATA_BYTES {
        return Err(MetadataError::PayloadTooLarge(length));
    }

    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload).await?;
    tracing::trace!(len = length, kind, "Read metadata frame");
    Ok((kind, payload))
}

/// Writes a metadata request frame to the provided stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
pub async fn write_metadata_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
) -> Result<(), MetadataError> {
    write_frame(writer, KIND_METADATA_REQUEST, &[]).await
}

/// Reads and validates a metadata request frame from the provided stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
pub async fn read_metadata_request<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<(), MetadataError> {
    let (kind, payload) = read_frame(reader).await?;
    if kind != KIND_METADATA_REQUEST {
        return Err(MetadataError::UnexpectedKind {
            expected: KIND_METADATA_REQUEST,
            actual: kind,
        });
    }
    if !payload.is_empty() {
        return Err(MetadataError::PayloadTooLarge(payload.len()));
    }
    Ok(())
}

/// Writes one metadata response frame to the provided stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[tracing::instrument(skip(writer))]
pub async fn write_metadata<W: AsyncWrite + Unpin>(
    writer: &mut W,
    metadata: &PeerMetadata,
) -> Result<(), MetadataError> {
    let payload = serde_json::to_vec(metadata)?;
    write_frame(writer, KIND_PEER_METADATA, &payload).await
}

/// Reads one metadata response frame from the provided stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[tracing::instrument(skip(reader))]
pub async fn read_metadata<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<PeerMetadata, MetadataError> {
    let (kind, payload) = read_frame(reader).await?;
    if kind != KIND_PEER_METADATA {
        return Err(MetadataError::UnexpectedKind {
            expected: KIND_PEER_METADATA,
            actual: kind,
        });
    }
    let raw: PeerMetadata = serde_json::from_slice(&payload)?;
    // Sanitize peer-supplied fields to prevent injection attacks.
    Ok(PeerMetadata::new(raw.hostname, raw.user, raw.os))
}

//! Decoders for transfer protocol frames.
#![allow(missing_docs)]

use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncReadExt};

use super::{
    KIND_BLOB_GET_READY, KIND_BLOB_GET_REQUEST, KIND_BLOB_PUT_REQUEST, KIND_COMPLETION_REQUEST,
    KIND_COMPLETION_RESPONSE, KIND_CWD_REQUEST, KIND_CWD_RESPONSE, KIND_ENTRY_COMPLETE, KIND_ERROR,
    KIND_EXISTS_REQUEST, KIND_EXISTS_RESPONSE, KIND_GET_CHUNK, KIND_GET_COMPLETE, KIND_GET_READY,
    KIND_GET_REQUEST, KIND_NEW_ENTRY, KIND_PUT_CHUNK, KIND_PUT_COMPLETE, KIND_PUT_READY,
    KIND_PUT_REQUEST, MAGIC, VERSION, validate_payload_limit,
};
use crate::transport::transfer::types::{
    ExistsRequest, ExistsResponse, GetRequest, PutRequest, TransferComplete, TransferError,
    TransferFailure, TransferFrame, TransferReady,
};

#[tracing::instrument(skip(reader))]
async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<(u8, Vec<u8>), TransferError> {
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic).await?;
    if magic != MAGIC {
        return Err(TransferError::InvalidMagic);
    }

    let version = reader.read_u8().await?;
    if version != VERSION {
        return Err(TransferError::UnsupportedVersion(version));
    }

    let kind = reader.read_u8().await?;
    if !matches!(
        kind,
        KIND_PUT_REQUEST
            | KIND_PUT_READY
            | KIND_PUT_CHUNK
            | KIND_PUT_COMPLETE
            | KIND_GET_REQUEST
            | KIND_GET_READY
            | KIND_GET_CHUNK
            | KIND_GET_COMPLETE
            | KIND_ERROR
            | KIND_CWD_REQUEST
            | KIND_CWD_RESPONSE
            | KIND_EXISTS_REQUEST
            | KIND_EXISTS_RESPONSE
            | KIND_NEW_ENTRY
            | KIND_ENTRY_COMPLETE
            | KIND_COMPLETION_REQUEST
            | KIND_COMPLETION_RESPONSE
            | KIND_BLOB_PUT_REQUEST
            | KIND_BLOB_GET_REQUEST
            | KIND_BLOB_GET_READY
    ) {
        return Err(TransferError::UnsupportedKind(kind));
    }

    let length = reader.read_u32().await? as usize;
    validate_payload_limit(kind, length)?;

    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload).await?;
    tracing::trace!(len = length, kind, "Read transfer frame");
    Ok((kind, payload))
}

/// Like [`read_frame`] but reuses a caller-provided buffer.
///
/// The buffer keeps its allocation across calls, avoiding a fresh
/// allocation + zero-fill per 64 KiB chunk on hot transfer paths.
async fn read_frame_into<'a, R: AsyncRead + Unpin>(
    reader: &mut R,
    buf: &'a mut Vec<u8>,
) -> Result<(u8, &'a [u8]), TransferError> {
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic).await?;
    if magic != MAGIC {
        return Err(TransferError::InvalidMagic);
    }

    let version = reader.read_u8().await?;
    if version != VERSION {
        return Err(TransferError::UnsupportedVersion(version));
    }

    let kind = reader.read_u8().await?;
    if !matches!(
        kind,
        KIND_PUT_REQUEST
            | KIND_PUT_READY
            | KIND_PUT_CHUNK
            | KIND_PUT_COMPLETE
            | KIND_GET_REQUEST
            | KIND_GET_READY
            | KIND_GET_CHUNK
            | KIND_GET_COMPLETE
            | KIND_ERROR
            | KIND_CWD_REQUEST
            | KIND_CWD_RESPONSE
            | KIND_EXISTS_REQUEST
            | KIND_EXISTS_RESPONSE
            | KIND_NEW_ENTRY
            | KIND_ENTRY_COMPLETE
            | KIND_COMPLETION_REQUEST
            | KIND_COMPLETION_RESPONSE
            | KIND_BLOB_PUT_REQUEST
            | KIND_BLOB_GET_REQUEST
            | KIND_BLOB_GET_READY
    ) {
        return Err(TransferError::UnsupportedKind(kind));
    }

    let length = reader.read_u32().await? as usize;
    validate_payload_limit(kind, length)?;

    buf.resize(length, 0);
    reader.read_exact(&mut buf[..]).await?;
    tracing::trace!(len = length, kind, "Read transfer frame (buffered)");
    Ok((kind, &buf[..]))
}

async fn read_json_frame<R: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(
    reader: &mut R,
    expected_kind: u8,
) -> Result<T, TransferError> {
    let (kind, payload) = read_frame(reader).await?;
    if kind != expected_kind {
        return Err(TransferError::UnexpectedKind {
            expected: expected_kind,
            actual: kind,
        });
    }
    Ok(serde_json::from_slice(&payload)?)
}

/// A transfer frame whose chunk payload borrows a caller-provided buffer.
///
/// Control frames are returned owned; only `PutChunk`/`GetChunk` payloads are
/// borrowed so hot transfer loops can reuse a single allocation.
pub(crate) enum TransferFrameBorrowed<'a> {
    PutChunk(&'a [u8]),
    GetChunk(&'a [u8]),
    Owned(TransferFrame),
}

/// Decodes a raw `(kind, payload)` pair into a [`TransferFrame`].
fn decode_frame(kind: u8, payload: &[u8]) -> Result<TransferFrame, TransferError> {
    match kind {
        KIND_PUT_REQUEST => Ok(TransferFrame::PutRequest(serde_json::from_slice(payload)?)),
        KIND_PUT_READY => Ok(TransferFrame::PutReady(serde_json::from_slice(payload)?)),
        KIND_PUT_COMPLETE => Ok(TransferFrame::PutComplete(serde_json::from_slice(payload)?)),
        KIND_GET_REQUEST => Ok(TransferFrame::GetRequest(serde_json::from_slice(payload)?)),
        KIND_GET_READY => Ok(TransferFrame::GetReady(serde_json::from_slice(payload)?)),
        KIND_GET_COMPLETE => Ok(TransferFrame::GetComplete(serde_json::from_slice(payload)?)),
        KIND_CWD_REQUEST => Ok(TransferFrame::CwdRequest(serde_json::from_slice(payload)?)),
        KIND_CWD_RESPONSE => Ok(TransferFrame::CwdResponse(serde_json::from_slice(payload)?)),
        KIND_EXISTS_REQUEST => Ok(TransferFrame::ExistsRequest(serde_json::from_slice(
            payload,
        )?)),
        KIND_EXISTS_RESPONSE => Ok(TransferFrame::ExistsResponse(serde_json::from_slice(
            payload,
        )?)),
        KIND_COMPLETION_REQUEST => Ok(TransferFrame::CompletionRequest(serde_json::from_slice(
            payload,
        )?)),
        KIND_COMPLETION_RESPONSE => Ok(TransferFrame::CompletionResponse(serde_json::from_slice(
            payload,
        )?)),
        KIND_BLOB_PUT_REQUEST => Ok(TransferFrame::BlobPutRequest(serde_json::from_slice(
            payload,
        )?)),
        KIND_BLOB_GET_REQUEST => Ok(TransferFrame::BlobGetRequest(serde_json::from_slice(
            payload,
        )?)),
        KIND_BLOB_GET_READY => Ok(TransferFrame::BlobGetReady(serde_json::from_slice(
            payload,
        )?)),
        KIND_NEW_ENTRY => Ok(TransferFrame::NewEntry(serde_json::from_slice(payload)?)),
        KIND_ENTRY_COMPLETE => Ok(TransferFrame::EntryComplete(serde_json::from_slice(
            payload,
        )?)),
        KIND_ERROR => Ok(TransferFrame::Error(serde_json::from_slice(payload)?)),
        _ => Err(TransferError::UnsupportedKind(kind)),
    }
}

/// Reads and decodes the next transfer frame, reusing the caller's buffer.
///
/// Hot transfer loops should prefer this over [`read_next_frame`] to avoid a
/// fresh allocation + zero-fill per data chunk.
pub(crate) async fn read_next_frame_into<'a, R: AsyncRead + Unpin>(
    reader: &mut R,
    buf: &'a mut Vec<u8>,
) -> Result<TransferFrameBorrowed<'a>, TransferError> {
    let (kind, payload) = read_frame_into(reader, buf).await?;
    match kind {
        KIND_PUT_CHUNK => Ok(TransferFrameBorrowed::PutChunk(payload)),
        KIND_GET_CHUNK => Ok(TransferFrameBorrowed::GetChunk(payload)),
        _ => Ok(TransferFrameBorrowed::Owned(decode_frame(kind, payload)?)),
    }
}

/// Reads and decodes the next transfer frame from the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[must_use]
#[tracing::instrument(skip(reader))]
pub async fn read_next_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<TransferFrame, TransferError> {
    let mut buf = Vec::new();
    match read_next_frame_into(reader, &mut buf).await? {
        TransferFrameBorrowed::PutChunk(payload) => Ok(TransferFrame::PutChunk(payload.to_vec())),
        TransferFrameBorrowed::GetChunk(payload) => Ok(TransferFrame::GetChunk(payload.to_vec())),
        TransferFrameBorrowed::Owned(frame) => Ok(frame),
    }
}

/// Reads and decodes a put request frame from the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn read_put_request<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<PutRequest, TransferError> {
    read_json_frame(reader, KIND_PUT_REQUEST).await
}

/// Reads and decodes a put ready frame from the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn read_put_ready<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<TransferReady, TransferError> {
    read_json_frame(reader, KIND_PUT_READY).await
}

/// Reads and decodes a put chunk frame from the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[must_use]
pub async fn read_put_chunk<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Vec<u8>, TransferError> {
    let (kind, payload) = read_frame(reader).await?;
    if kind != KIND_PUT_CHUNK {
        return Err(TransferError::UnexpectedKind {
            expected: KIND_PUT_CHUNK,
            actual: kind,
        });
    }
    Ok(payload)
}

/// Reads and decodes a put complete frame from the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn read_put_complete<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<TransferComplete, TransferError> {
    read_json_frame(reader, KIND_PUT_COMPLETE).await
}

/// Reads and decodes a get request frame from the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn read_get_request<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<GetRequest, TransferError> {
    read_json_frame(reader, KIND_GET_REQUEST).await
}

/// Reads and decodes a get ready frame from the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn read_get_ready<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<TransferReady, TransferError> {
    read_json_frame(reader, KIND_GET_READY).await
}

/// Reads and decodes a get chunk frame from the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[must_use]
pub async fn read_get_chunk<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Vec<u8>, TransferError> {
    let (kind, payload) = read_frame(reader).await?;
    if kind != KIND_GET_CHUNK {
        return Err(TransferError::UnexpectedKind {
            expected: KIND_GET_CHUNK,
            actual: kind,
        });
    }
    Ok(payload)
}

/// Reads and decodes a get complete frame from the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn read_get_complete<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<TransferComplete, TransferError> {
    read_json_frame(reader, KIND_GET_COMPLETE).await
}

/// Reads and decodes a transfer error frame from the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn read_transfer_error<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<TransferFailure, TransferError> {
    read_json_frame(reader, KIND_ERROR).await
}

/// Reads and decodes an exists request frame from the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn read_exists_request<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<ExistsRequest, TransferError> {
    read_json_frame(reader, KIND_EXISTS_REQUEST).await
}

/// Reads and decodes an exists response frame from the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be deserialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn read_exists_response<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<ExistsResponse, TransferError> {
    read_json_frame(reader, KIND_EXISTS_RESPONSE).await
}

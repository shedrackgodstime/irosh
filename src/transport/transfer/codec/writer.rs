//! Encoders for transfer protocol frames.
#![allow(missing_docs)]

use serde::Serialize;
use tokio::io::AsyncWrite;

use super::{
    KIND_BLOB_GET_READY, KIND_BLOB_GET_REQUEST, KIND_BLOB_PUT_REQUEST, KIND_COMPLETION_REQUEST,
    KIND_COMPLETION_RESPONSE, KIND_CWD_REQUEST, KIND_CWD_RESPONSE, KIND_ENTRY_COMPLETE, KIND_ERROR,
    KIND_EXISTS_REQUEST, KIND_EXISTS_RESPONSE, KIND_GET_CHUNK, KIND_GET_COMPLETE, KIND_GET_READY,
    KIND_GET_REQUEST, KIND_NEW_ENTRY, KIND_PUT_CHUNK, KIND_PUT_COMPLETE, KIND_PUT_READY,
    KIND_PUT_REQUEST, write_frame,
};
use crate::transport::transfer::types::{
    BlobGetReady, BlobGetRequest, BlobPutRequest, CwdRequest, CwdResponse, ExistsRequest,
    ExistsResponse, GetRequest, PutRequest, TransferComplete, TransferError, TransferFailure,
    TransferReady,
};
use crate::transport::transfer::{
    CompletionRequest, CompletionResponse, EntryComplete, EntryHeader,
};

async fn write_json_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    kind: u8,
    value: &T,
) -> Result<(), TransferError> {
    let payload = serde_json::to_vec(value)?;
    write_frame(writer, kind, &payload).await
}

/// Writes a put request frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_put_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    request: &PutRequest,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_PUT_REQUEST, request).await
}

/// Writes a put ready frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_put_ready<W: AsyncWrite + Unpin>(
    writer: &mut W,
    ready: &TransferReady,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_PUT_READY, ready).await
}

/// Writes a put chunk frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
pub async fn write_put_chunk<W: AsyncWrite + Unpin>(
    writer: &mut W,
    chunk: &[u8],
) -> Result<(), TransferError> {
    write_frame(writer, KIND_PUT_CHUNK, chunk).await
}

/// Writes a put complete frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_put_complete<W: AsyncWrite + Unpin>(
    writer: &mut W,
    complete: &TransferComplete,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_PUT_COMPLETE, complete).await
}

/// Writes a get request frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_get_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    request: &GetRequest,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_GET_REQUEST, request).await
}

/// Writes a get ready frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_get_ready<W: AsyncWrite + Unpin>(
    writer: &mut W,
    ready: &TransferReady,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_GET_READY, ready).await
}

/// Writes a get chunk frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
pub async fn write_get_chunk<W: AsyncWrite + Unpin>(
    writer: &mut W,
    chunk: &[u8],
) -> Result<(), TransferError> {
    write_frame(writer, KIND_GET_CHUNK, chunk).await
}

/// Writes a get complete frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_get_complete<W: AsyncWrite + Unpin>(
    writer: &mut W,
    complete: &TransferComplete,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_GET_COMPLETE, complete).await
}

/// Writes a transfer error frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_transfer_error<W: AsyncWrite + Unpin>(
    writer: &mut W,
    error: &TransferFailure,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_ERROR, error).await
}

/// Writes a cwd request frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_cwd_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    request: &CwdRequest,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_CWD_REQUEST, request).await
}

/// Writes a cwd response frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_cwd_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    response: &CwdResponse,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_CWD_RESPONSE, response).await
}

/// Writes an exists request frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_exists_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    req: &ExistsRequest,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_EXISTS_REQUEST, req).await
}

/// Writes an exists response frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_exists_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    res: &ExistsResponse,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_EXISTS_RESPONSE, res).await
}

/// Writes a new entry frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_new_entry<W: AsyncWrite + Unpin>(
    writer: &mut W,
    header: &EntryHeader,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_NEW_ENTRY, header).await
}

/// Writes an entry complete frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_entry_complete<W: AsyncWrite + Unpin>(
    writer: &mut W,
    complete: &EntryComplete,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_ENTRY_COMPLETE, complete).await
}

/// Writes a completion request frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_completion_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    req: &CompletionRequest,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_COMPLETION_REQUEST, req).await
}

/// Writes a completion response frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_completion_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    res: &CompletionResponse,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_COMPLETION_RESPONSE, res).await
}

/// Writes a blob put request frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_blob_put_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    req: &BlobPutRequest,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_BLOB_PUT_REQUEST, req).await
}

/// Writes a blob get request frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
#[inline]
pub async fn write_blob_get_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    req: &BlobGetRequest,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_BLOB_GET_REQUEST, req).await
}

/// Writes a blob get ready frame to the stream.
///
/// # Errors
///
/// Returns an error if the data cannot be serialized or if the underlying channel encounters an I/O error.
#[must_use]
pub async fn write_blob_get_ready<W: AsyncWrite + Unpin>(
    writer: &mut W,
    ready: &BlobGetReady,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_BLOB_GET_READY, ready).await
}

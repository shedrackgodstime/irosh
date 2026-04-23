use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::types::{
    CwdRequest, CwdResponse, ExistsRequest, ExistsResponse, GetRequest, MAX_CHUNK_BYTES,
    MAX_CONTROL_BYTES, PutRequest, TransferComplete, TransferError, TransferFailure, TransferFrame,
    TransferReady,
};

/// Magic header for transfer frames.
pub(crate) const MAGIC: [u8; 4] = *b"IRFT";
/// Current transfer protocol version.
pub(crate) const VERSION: u8 = 1;

#[cfg(test)]
pub(crate) const KIND_PUT_REQUEST: u8 = 1;
#[cfg(not(test))]
const KIND_PUT_REQUEST: u8 = 1;
const KIND_PUT_READY: u8 = 2;
const KIND_PUT_CHUNK: u8 = 3;
const KIND_PUT_COMPLETE: u8 = 4;
const KIND_GET_REQUEST: u8 = 5;
const KIND_GET_READY: u8 = 6;
pub(crate) const KIND_GET_CHUNK: u8 = 7;
const KIND_GET_COMPLETE: u8 = 8;
const KIND_ERROR: u8 = 9;
const KIND_CWD_REQUEST: u8 = 10;
const KIND_CWD_RESPONSE: u8 = 11;
const KIND_EXISTS_REQUEST: u8 = 12;
const KIND_EXISTS_RESPONSE: u8 = 13;
const KIND_NEW_ENTRY: u8 = 14;
const KIND_ENTRY_COMPLETE: u8 = 15;

fn validate_payload_limit(kind: u8, payload_len: usize) -> Result<(), TransferError> {
    let max_len = match kind {
        KIND_PUT_CHUNK | KIND_GET_CHUNK => MAX_CHUNK_BYTES,
        _ => MAX_CONTROL_BYTES,
    };

    if payload_len > max_len {
        return Err(TransferError::PayloadTooLarge(payload_len));
    }
    Ok(())
}

async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    kind: u8,
    payload: &[u8],
) -> Result<(), TransferError> {
    validate_payload_limit(kind, payload.len())?;

    writer.write_all(&MAGIC).await?;
    writer.write_u8(VERSION).await?;
    writer.write_u8(kind).await?;
    writer.write_u32(payload.len() as u32).await?;
    writer.write_all(payload).await?;
    writer.flush().await?;
    Ok(())
}

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
    ) {
        return Err(TransferError::UnsupportedKind(kind));
    }

    let length = reader.read_u32().await? as usize;
    validate_payload_limit(kind, length)?;

    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload).await?;
    Ok((kind, payload))
}

async fn write_json_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    kind: u8,
    value: &T,
) -> Result<(), TransferError> {
    let payload = serde_json::to_vec(value)?;
    write_frame(writer, kind, &payload).await
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

pub async fn write_put_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    request: &PutRequest,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_PUT_REQUEST, request).await
}

pub async fn read_put_request<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<PutRequest, TransferError> {
    read_json_frame(reader, KIND_PUT_REQUEST).await
}

pub async fn write_put_ready<W: AsyncWrite + Unpin>(
    writer: &mut W,
    ready: &TransferReady,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_PUT_READY, ready).await
}

pub async fn read_put_ready<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<TransferReady, TransferError> {
    read_json_frame(reader, KIND_PUT_READY).await
}

pub async fn write_put_chunk<W: AsyncWrite + Unpin>(
    writer: &mut W,
    chunk: &[u8],
) -> Result<(), TransferError> {
    write_frame(writer, KIND_PUT_CHUNK, chunk).await
}

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

pub async fn write_put_complete<W: AsyncWrite + Unpin>(
    writer: &mut W,
    complete: &TransferComplete,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_PUT_COMPLETE, complete).await
}

pub async fn read_put_complete<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<TransferComplete, TransferError> {
    read_json_frame(reader, KIND_PUT_COMPLETE).await
}

pub async fn write_get_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    request: &GetRequest,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_GET_REQUEST, request).await
}

pub async fn read_get_request<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<GetRequest, TransferError> {
    read_json_frame(reader, KIND_GET_REQUEST).await
}

pub async fn write_get_ready<W: AsyncWrite + Unpin>(
    writer: &mut W,
    ready: &TransferReady,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_GET_READY, ready).await
}

pub async fn read_get_ready<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<TransferReady, TransferError> {
    read_json_frame(reader, KIND_GET_READY).await
}

pub async fn write_get_chunk<W: AsyncWrite + Unpin>(
    writer: &mut W,
    chunk: &[u8],
) -> Result<(), TransferError> {
    write_frame(writer, KIND_GET_CHUNK, chunk).await
}

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

pub async fn write_get_complete<W: AsyncWrite + Unpin>(
    writer: &mut W,
    complete: &TransferComplete,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_GET_COMPLETE, complete).await
}

pub async fn read_get_complete<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<TransferComplete, TransferError> {
    read_json_frame(reader, KIND_GET_COMPLETE).await
}

pub async fn write_transfer_error<W: AsyncWrite + Unpin>(
    writer: &mut W,
    error: &TransferFailure,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_ERROR, error).await
}

pub async fn read_transfer_error<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<TransferFailure, TransferError> {
    read_json_frame(reader, KIND_ERROR).await
}

pub async fn write_cwd_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    request: &CwdRequest,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_CWD_REQUEST, request).await
}

pub async fn write_cwd_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    response: &CwdResponse,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_CWD_RESPONSE, response).await
}

pub async fn write_exists_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    req: &ExistsRequest,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_EXISTS_REQUEST, req).await
}

pub async fn read_exists_request<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<ExistsRequest, TransferError> {
    read_json_frame(reader, KIND_EXISTS_REQUEST).await
}

pub async fn write_exists_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    res: &ExistsResponse,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_EXISTS_RESPONSE, res).await
}

pub async fn read_exists_response<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<ExistsResponse, TransferError> {
    read_json_frame(reader, KIND_EXISTS_RESPONSE).await
}

pub async fn write_new_entry<W: AsyncWrite + Unpin>(
    writer: &mut W,
    header: &crate::transport::transfer::EntryHeader,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_NEW_ENTRY, header).await
}

pub async fn write_entry_complete<W: AsyncWrite + Unpin>(
    writer: &mut W,
    complete: &crate::transport::transfer::EntryComplete,
) -> Result<(), TransferError> {
    write_json_frame(writer, KIND_ENTRY_COMPLETE, complete).await
}

/// Reads and decodes the next transfer frame from the stream.
pub async fn read_next_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<TransferFrame, TransferError> {
    let (kind, payload) = read_frame(reader).await?;
    match kind {
        KIND_PUT_REQUEST => Ok(TransferFrame::PutRequest(serde_json::from_slice(&payload)?)),
        KIND_PUT_READY => Ok(TransferFrame::PutReady(serde_json::from_slice(&payload)?)),
        KIND_PUT_CHUNK => Ok(TransferFrame::PutChunk(payload)),
        KIND_PUT_COMPLETE => Ok(TransferFrame::PutComplete(serde_json::from_slice(
            &payload,
        )?)),
        KIND_GET_REQUEST => Ok(TransferFrame::GetRequest(serde_json::from_slice(&payload)?)),
        KIND_GET_READY => Ok(TransferFrame::GetReady(serde_json::from_slice(&payload)?)),
        KIND_GET_CHUNK => Ok(TransferFrame::GetChunk(payload)),
        KIND_GET_COMPLETE => Ok(TransferFrame::GetComplete(serde_json::from_slice(
            &payload,
        )?)),
        KIND_CWD_REQUEST => Ok(TransferFrame::CwdRequest(serde_json::from_slice(&payload)?)),
        KIND_CWD_RESPONSE => Ok(TransferFrame::CwdResponse(serde_json::from_slice(
            &payload,
        )?)),
        KIND_EXISTS_REQUEST => Ok(TransferFrame::ExistsRequest(serde_json::from_slice(
            &payload,
        )?)),
        KIND_EXISTS_RESPONSE => Ok(TransferFrame::ExistsResponse(serde_json::from_slice(
            &payload,
        )?)),
        KIND_NEW_ENTRY => Ok(TransferFrame::NewEntry(serde_json::from_slice(&payload)?)),
        KIND_ENTRY_COMPLETE => Ok(TransferFrame::EntryComplete(serde_json::from_slice(
            &payload,
        )?)),
        KIND_ERROR => Ok(TransferFrame::Error(serde_json::from_slice(&payload)?)),
        _ => Err(TransferError::UnsupportedKind(kind)),
    }
}

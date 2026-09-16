//! Wire format encoders and decoders for the transfer protocol.
//!
//! All public functions in this module follow the naming convention
//! `{direction}_{frame}` (e.g. `write_put_request`, `read_get_chunk`)
//! and are paired as writer/reader for each transfer frame type.
#![allow(missing_docs)]

use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::types::{MAX_CHUNK_BYTES, MAX_CONTROL_BYTES, TransferError};

mod reader;
mod writer;

pub use reader::*;
pub use writer::*;

/// Magic header for transfer frames.
pub(crate) const MAGIC: [u8; 4] = *b"IRFT";
/// Current transfer protocol version.
pub(crate) const VERSION: u8 = 2;

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
const KIND_COMPLETION_REQUEST: u8 = 16;
const KIND_COMPLETION_RESPONSE: u8 = 17;
const KIND_BLOB_PUT_REQUEST: u8 = 18;
const KIND_BLOB_GET_REQUEST: u8 = 19;
const KIND_BLOB_GET_READY: u8 = 20;

#[inline]
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

#[tracing::instrument(skip(writer))]
pub(crate) async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    kind: u8,
    payload: &[u8],
) -> Result<(), TransferError> {
    tracing::trace!(len = payload.len(), kind, "Writing transfer frame");
    validate_payload_limit(kind, payload.len())?;

    writer.write_all(&MAGIC).await?;
    writer.write_u8(VERSION).await?;
    writer.write_u8(kind).await?;
    // Reason: payload length is validated against MAX_CONTROL_BYTES / MAX_CHUNK_BYTES before this point.
    #[allow(clippy::cast_possible_truncation)]
    let len = payload.len() as u32;
    writer.write_u32(len).await?;
    writer.write_all(payload).await?;
    writer.flush().await?;
    Ok(())
}

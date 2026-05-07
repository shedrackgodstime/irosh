//! Framed single-file transfer protocol exchanged on a separate Iroh stream.

mod codec;
mod helpers;
#[cfg(test)]
mod tests;
mod types;

#[cfg(test)]
pub(crate) use codec::{KIND_GET_CHUNK, KIND_PUT_REQUEST, MAGIC, VERSION};
pub use codec::{
    read_exists_request, read_exists_response, read_get_chunk, read_get_complete, read_get_ready,
    read_get_request, read_next_frame, read_put_chunk, read_put_complete, read_put_ready,
    read_put_request, read_transfer_error, write_completion_request, write_completion_response,
    write_cwd_request, write_cwd_response, write_entry_complete, write_exists_request,
    write_exists_response, write_get_chunk, write_get_complete, write_get_ready, write_get_request,
    write_new_entry, write_put_chunk, write_put_complete, write_put_ready, write_put_request,
    write_transfer_error,
};
pub use helpers::sanitize_remote_path;
#[cfg(test)]
pub(crate) use types::MAX_CONTROL_BYTES;
pub use types::{
    CompletionRequest, CompletionResponse, CwdRequest, CwdResponse, EntryComplete, EntryHeader,
    ExistsRequest, ExistsResponse, GetRequest, MAX_CHUNK_BYTES, PutRequest, TransferComplete,
    TransferError, TransferFailure, TransferFailureCode, TransferFrame, TransferReady,
};

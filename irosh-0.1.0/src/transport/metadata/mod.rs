//! Optional framed peer metadata exchanged on a separate Iroh stream.

mod codec;
#[cfg(test)]
mod tests;
mod types;

#[cfg(test)]
pub(crate) use codec::{KIND_PEER_METADATA, MAGIC, MAX_METADATA_BYTES, VERSION};
pub use codec::{read_metadata, read_metadata_request, write_metadata, write_metadata_request};
pub use types::{MetadataError, PeerMetadata};

//! Transport-layer errors.

/// Transport-layer errors.
#[cfg(feature = "transport")]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TransportError {
    /// Binding a local endpoint failed.
    #[error("failed to bind transport endpoint")]
    EndpointBind {
        /// The underlying iroh bind error.
        #[source]
        source: iroh::endpoint::BindError,
    },

    /// The P2P connection was lost or refused.
    #[error("transport connection lost: {source}")]
    ConnectionLost {
        /// The underlying iroh connection error.
        #[source]
        source: iroh::endpoint::ConnectionError,
    },

    /// Metadata framing or parsing failed.
    #[error(transparent)]
    Metadata(#[from] crate::transport::metadata::MetadataError),

    /// Transfer framing or parsing failed.
    #[error(transparent)]
    Transfer(#[from] crate::transport::transfer::TransferError),

    /// The provided connection ticket has an invalid format.
    #[error("invalid connection ticket format")]
    TicketFormatInvalid,

    /// The provided relay URL is invalid.
    #[error("invalid relay URL: {url}")]
    InvalidRelayUrl {
        /// The invalid relay URL.
        url: String,
    },

    /// A general protocol violation or unexpected message sequence.
    #[error("protocol violation: {details}")]
    ProtocolError {
        /// A description of the violation.
        details: String,
    },
}

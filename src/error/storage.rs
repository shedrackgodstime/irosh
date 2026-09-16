//! Storage and persistence errors.

use std::path::PathBuf;

/// Storage and persistence errors.
#[cfg(feature = "storage")]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StorageError {
    /// Failed to create a directory at the given path.
    #[error("failed to create directory at {path}")]
    DirectoryCreate {
        /// The path that could not be created.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to read the contents of a directory.
    #[error("failed to read directory at {path}")]
    DirectoryRead {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to read an entry within a directory.
    #[error("failed to read entry in directory {path}")]
    DirectoryEntryRead {
        /// The directory being enumerated.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to read a file from disk.
    #[error("failed to read file at {path}")]
    FileRead {
        /// The file path that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to write a file to disk.
    #[error("failed to write file at {path}")]
    FileWrite {
        /// The file path that could not be written to.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to delete a file.
    #[error("failed to delete file at {path}")]
    FileDelete {
        /// The file path that could not be deleted.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The requested peer alias was not found in the local storage.
    #[error("peer '{alias}' not found in storage")]
    PeerNotFound {
        /// The alias that was searched for.
        alias: String,
    },

    /// Failed to parse or decode a connection ticket.
    #[error("failed to parse connection ticket")]
    TicketParse {
        /// The underlying ticket parsing error.
        #[source]
        source: crate::transport::ticket::TicketError,
    },

    /// Failed to load or generate the local P2P identity.
    #[error("failed to load or generate local identity")]
    IdentityLoad {
        /// The underlying transport error from iroh.
        #[source]
        source: iroh::endpoint::TransportError,
    },

    /// Failed to parse an SSH public key.
    #[error("failed to parse SSH public key")]
    PublicKeyParse {
        /// The underlying SSH key error.
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    /// Failed to read an SSH public key file from disk.
    #[error("failed to read SSH public key file at {path}")]
    PublicKeyRead {
        /// The path to the key file.
        path: PathBuf,
        /// The underlying SSH key error.
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    /// Failed to write an SSH public key to disk.
    #[error("failed to write SSH public key")]
    PublicKeyWrite {
        /// The path to the key file.
        path: PathBuf,
        /// The underlying SSH key error.
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    /// Failed to format an SSH public key for display or export.
    #[error("failed to format public key")]
    PublicKeyFormat {
        /// The underlying SSH key error.
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    /// A blocking storage task (e.g. key generation) failed.
    #[error("blocking storage task failed during {operation}")]
    BlockingTaskFailed {
        /// A description of the operation that was in progress.
        operation: &'static str,
        /// The Tokio join error.
        #[source]
        source: tokio::task::JoinError,
    },

    /// The endpoint secret file is invalid or corrupt.
    #[error("invalid endpoint secret at {path}: {details}")]
    EndpointSecretInvalid {
        /// Path to the invalid secret file.
        path: PathBuf,
        /// Details about why the secret is invalid.
        details: String,
        /// The underlying key-parsing error.
        #[source]
        source: iroh::KeyParsingError,
    },

    /// The provided peer name is invalid (e.g. contains path separators).
    #[error("invalid peer name: {name}")]
    PeerNameInvalid {
        /// The invalid name that was provided.
        name: String,
    },

    /// Failed to serialize a peer profile to JSON.
    #[error("failed to serialize peer profile")]
    PeerProfileSerialize {
        /// The underlying serialization error.
        #[source]
        source: serde_json::Error,
    },

    /// Failed to parse a peer profile from JSON.
    #[error("failed to parse peer profile")]
    PeerProfileParse {
        /// The underlying parsing error.
        #[source]
        source: serde_json::Error,
    },

    /// Password hashing failed (argon2).
    #[error("failed to hash password: {reason}")]
    PasswordHash {
        /// The underlying error from the argon2 crate.
        ///
        /// NOTE: This does not use `#[source]` because `argon2::password_hash::Error`
        /// does not currently implement `std::error::Error`.
        reason: argon2::password_hash::Error,
    },
}

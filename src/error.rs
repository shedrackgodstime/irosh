//! Top-level and subsystem error types for the irosh library.

use std::path::PathBuf;

/// Transport-layer errors.
#[cfg(feature = "transport")]
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// Binding a local Iroh endpoint failed.
    #[error("failed to bind Iroh endpoint")]
    EndpointBind {
        #[source]
        source: iroh::endpoint::BindError,
    },

    /// A ticket string could not be parsed.
    #[error("irosh ticket format is invalid")]
    TicketFormatInvalid,

    /// Metadata framing or parsing failed.
    #[error(transparent)]
    Metadata(#[from] crate::transport::metadata::MetadataError),

    /// Transfer framing or parsing failed.
    #[error(transparent)]
    Transfer(#[from] crate::transport::transfer::TransferError),
}

/// Storage and persistence errors.
#[cfg(feature = "storage")]
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("failed to create directory at {path}")]
    DirectoryCreate {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read file at {path}")]
    FileRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write file at {path}")]
    FileWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to delete file at {path}")]
    FileDelete {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read directory at {path}")]
    DirectoryRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read directory entry")]
    DirectoryEntryRead {
        #[source]
        source: std::io::Error,
    },

    #[error("failed to serialize peer profile")]
    PeerProfileSerialize {
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to parse peer profile")]
    PeerProfileParse {
        #[source]
        source: serde_json::Error,
    },

    #[error("node secret at {path} is invalid: {details}")]
    NodeSecretInvalid { path: PathBuf, details: String },

    #[error("peer name is invalid: {name}")]
    PeerNameInvalid { name: String },

    #[error("failed to read public key at {path}")]
    PublicKeyRead {
        path: PathBuf,
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    #[error("failed to write public key at {path}")]
    PublicKeyWrite {
        path: PathBuf,
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    #[error("failed to format public key")]
    PublicKeyFormat {
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    #[error("blocking storage task failed during {operation}")]
    BlockingTaskFailed {
        operation: &'static str,
        #[source]
        source: tokio::task::JoinError,
    },
}

/// Client-side session and lifecycle errors.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The P2P connection to the target peer failed.
    #[error("failed to connect to P2P endpoint")]
    ConnectFailed {
        #[source]
        source: iroh::endpoint::ConnectError,
    },

    /// Opening a bi-directional stream for SSH failed.
    #[error("failed to open SSH transport stream")]
    StreamOpenFailed {
        #[source]
        source: iroh::endpoint::ConnectionError,
    },

    /// A metadata-related operation failed.
    #[error("metadata request failed: {detail}")]
    MetadataFailed { detail: String },

    /// Negotiating the SSH protocol failed.
    #[error("failed to negotiate SSH protocol")]
    SshNegotiationFailed {
        #[source]
        source: russh::Error,
    },

    /// The SSH session channel could not be opened.
    #[error("failed to open SSH session channel")]
    ChannelOpenFailed {
        #[source]
        source: russh::Error,
    },

    /// Requesting a PTY failed.
    #[error("failed to request PTY")]
    PtyRequestFailed {
        #[source]
        source: russh::Error,
    },

    /// Requesting an interactive shell failed.
    #[error("failed to request shell")]
    ShellRequestFailed {
        #[source]
        source: russh::Error,
    },

    /// Executing a remote command failed.
    #[error("failed to execute remote command")]
    ExecFailed {
        #[source]
        source: russh::Error,
    },

    /// Sending data over the SSH channel failed.
    #[error("failed to send data")]
    DataSendFailed {
        #[source]
        source: russh::Error,
    },

    /// Sending EOF over the SSH channel failed.
    #[error("failed to send EOF")]
    EofSendFailed {
        #[source]
        source: russh::Error,
    },

    /// Resizing the PTY window failed.
    #[error("failed to resize PTY window")]
    WindowChangeFailed {
        #[source]
        source: russh::Error,
    },

    /// Disconnecting the SSH session failed.
    #[error("failed to disconnect SSH session")]
    DisconnectFailed {
        #[source]
        source: russh::Error,
    },

    /// Standard I/O failure on the local terminal.

    #[error("terminal I/O error")]
    TerminalIo {
        #[source]
        source: std::io::Error,
    },

    /// The SSH peer disconnected abruptly during the initial handshake.
    #[error("SSH peer disconnected during handshake")]
    SshHandshakeDisconnected { detail: Option<String> },

    /// A file upload operation failed.
    #[error("upload failed: {details}")]
    UploadFailed { details: String },

    /// A file download operation failed.
    #[error("download failed: {details}")]
    DownloadFailed { details: String },

    /// A file-level I/O operation failed.
    #[error("failed to {operation} at {path}")]
    FileIo {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The transfer target identifier or path is invalid.
    #[error("invalid transfer target: {reason}")]
    TransferTargetInvalid { reason: &'static str },

    /// The remote peer rejected the transfer request.
    #[error("transfer rejected by remote: {details}")]
    TransferRejected { details: String },

    /// A transfer-related control operation failed.
    #[error("transfer control operation failed: {details}")]
    TransferFailed { details: String },

    /// The session transport is not available (disconnected or not initialized).
    #[error("transport unavailable: {details}")]
    TransportUnavailable { details: &'static str },
}

/// Server-side orchestration errors.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// Starting the background P2P listener failed.
    #[error("failed to start Iroh background service")]
    ServerStart {
        #[source]
        source: iroh::endpoint::BindError,
    },

    /// Formatting the SSH host key failed.
    #[error("failed to format host key")]
    FormatHostKey {
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    /// A file transfer operation in the server context failed.
    #[error("transfer operation failed: {details}")]
    TransferFailed { details: String },

    /// A file-level I/O operation failed.
    #[error("failed to {operation} at {path}")]
    FileIo {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The transfer request was rejected.
    #[error("transfer rejected: {details}")]
    TransferRejected { details: String },

    /// Spawning a shell or command process failed.
    #[error("failed to spawn remote process")]
    ProcessSpawnFailed {
        #[source]
        source: std::io::Error,
    },

    /// A blocking task failed to complete.
    #[error("blocking server task failed during {operation}")]
    BlockingTaskFailed {
        operation: &'static str,
        #[source]
        source: tokio::task::JoinError,
    },

    /// A query about a process (e.g., CWD) failed.
    #[error("failed to query process {pid}: {details}")]
    ProcessQueryFailed { pid: u32, details: String },

    /// An SSH channel operation failed.
    #[error("SSH channel error during {operation}: {details}")]
    ChannelError {
        operation: &'static str,
        details: String,
    },

    /// A shell-related operation failed.
    #[error("shell error: {details}")]
    ShellError { details: String },
}

/// The comprehensive error type for all library operations.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum IroshError {
    /// A transport-level error, such as endpoint binding or ticket parsing failure.
    #[cfg(feature = "transport")]
    #[error("transport error")]
    Transport(#[from] TransportError),

    /// A storage-level error, such as failing to read/write keys, trust, or peers.
    #[cfg(feature = "storage")]
    #[error("storage error")]
    Storage(#[from] StorageError),

    /// A server-level error, such as failing to accept or manage an SSH session.
    #[error("server error")]
    Server(#[from] ServerError),

    /// A client-level error, such as connection or session lifecycle failure.
    #[error("client error")]
    Client(#[from] ClientError),

    #[cfg(any(feature = "server", feature = "client"))]
    /// An underlying SSH protocol error from the `russh` crate.
    #[error("SSH error: {0}")]
    Russh(#[from] russh::Error),

    /// SSH authentication failed.
    #[error("SSH authentication failed")]
    AuthenticationFailed,

    /// The remote server's host key did not match the expected key.
    #[error("server key mismatch: expected {expected}, got {actual}")]
    ServerKeyMismatch { expected: String, actual: String },

    /// The specified peer name was not found in the storage layer.
    #[error("unknown peer: {name}")]
    UnknownPeer { name: String },

    /// The provided connection target is invalid or unparseable.
    #[error("invalid connection target: {raw}")]
    InvalidTarget { raw: String },
}

/// A specialized `Result` type for irosh library operations.
pub type Result<T> = std::result::Result<T, IroshError>;

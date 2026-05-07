//! Top-level and subsystem error types for the irosh library.

use std::path::PathBuf;

/// Transport-layer errors.
#[cfg(feature = "transport")]
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// Binding a local endpoint failed.
    #[error("failed to bind transport endpoint")]
    EndpointBind {
        #[source]
        source: iroh::endpoint::BindError,
    },

    /// The P2P connection was lost or refused.
    #[error("transport connection lost: {source}")]
    ConnectionLost {
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
    InvalidRelayUrl { url: String },

    /// A general protocol violation or unexpected message sequence.
    #[error("protocol violation: {details}")]
    ProtocolError { details: String },
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

    #[error("failed to read directory at {path}")]
    DirectoryRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read entry in directory {path}")]
    DirectoryEntryRead {
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

    #[error("peer '{alias}' not found in storage")]
    PeerNotFound { alias: String },

    #[error("failed to parse connection ticket")]
    TicketParse {
        #[source]
        source: crate::transport::ticket::TicketError,
    },

    #[error("failed to load or generate local identity")]
    IdentityLoad {
        #[source]
        source: iroh::endpoint::TransportError,
    },

    #[error("failed to parse SSH public key")]
    PublicKeyParse {
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    #[error("failed to read SSH public key file at {path}")]
    PublicKeyRead {
        path: PathBuf,
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    #[error("failed to write SSH public key")]
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

    #[error("invalid node secret at {path}: {details}")]
    NodeSecretInvalid {
        path: PathBuf,
        details: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("invalid peer name: {name}")]
    PeerNameInvalid { name: String },

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

    #[error("failed to hash password: {reason}")]
    PasswordHash {
        /// The underlying error from the argon2 crate.
        ///
        /// NOTE: This does not use `#[source]` because `argon2::password_hash::Error`
        /// does not currently implement `std::error::Error`.
        reason: argon2::password_hash::Error,
    },
}

/// Authentication and credential errors.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Password verification failed due to an incorrect password.
    #[error("invalid password provided")]
    InvalidPassword,

    /// Password verification failed due to a cryptographic or format error.
    #[error("password verification failed: {reason}")]
    VerificationFailed {
        /// The underlying error from the argon2 crate.
        ///
        /// NOTE: This does not use `#[source]` because `argon2::password_hash::Error`
        /// does not currently implement `std::error::Error`.
        reason: argon2::password_hash::Error,
    },

    /// The required authentication method is not supported by the client or server.
    #[error("unsupported authentication method: {0}")]
    UnsupportedMethod(String),

    /// A required credential (like a password) was not provided.
    #[error("missing required credential: {0}")]
    MissingCredential(String),
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

    /// Requesting a shell session failed.
    #[error("failed to request shell")]
    ShellRequestFailed {
        #[source]
        source: russh::Error,
    },

    /// A command failed to execute.
    #[error("remote command execution failed")]
    ExecFailed {
        #[source]
        source: russh::Error,
    },

    /// Sending data over the SSH channel failed.
    #[error("failed to send data to remote channel")]
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

    /// A port forwarding tunnel failed.
    #[error("tunnel failed: {details}")]
    TunnelFailed { details: String },
}

/// Server-side orchestration errors.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// The Iroh endpoint failed to bind.
    #[error("failed to bind server endpoint")]
    EndpointBind {
        #[source]
        source: iroh::endpoint::BindError,
    },

    /// Identity loading or generation failed.
    #[error("failed to load server identity")]
    IdentityLoad {
        #[source]
        source: iroh::endpoint::TransportError,
    },

    /// SSH server configuration failed.
    #[error("failed to configure SSH server")]
    SshConfig {
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    /// A shell process failed to start or manage.
    #[error("remote shell error: {details}")]
    ShellError { details: String },

    /// A channel-level SSH operation failed.
    #[error("channel error during {operation}: {details}")]
    ChannelError {
        operation: &'static str,
        details: String,
    },

    /// A file transfer operation failed on the server.
    #[error("server transfer error: {details}")]
    TransferFailed { details: String },

    /// The remote peer provided an invalid transfer path.
    #[error("invalid transfer path: {details}")]
    InvalidPath { details: String },

    #[error("failed to format host key")]
    FormatHostKey {
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    #[error("blocking storage task failed during {operation}")]
    BlockingTaskFailed {
        operation: &'static str,
        #[source]
        source: tokio::task::JoinError,
    },

    #[error("failed to query process information for PID {pid}: {details}")]
    ProcessQueryFailed {
        pid: u32,
        details: String,
        #[source]
        source: std::io::Error,
    },

    /// Failure during OS service management (install/start/stop).
    #[error("Service management failure: {details}")]
    ServiceManagement { details: String },
}

/// Top-level crate error unifying all subsystem failures.
#[derive(Debug, thiserror::Error)]
pub enum IroshError {
    #[error("platform not supported: {0}")]
    PlatformNotSupported(String),

    /// Errors originating from the Iroh transport layer.
    #[cfg(feature = "transport")]
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),

    /// Errors originating from the storage or persistence layer.
    #[cfg(feature = "storage")]
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    /// Errors originating from the SSH client session.
    #[error("client error: {0}")]
    Client(#[from] ClientError),

    /// Errors originating from the SSH server orchestration.
    #[error("server error: {0}")]
    Server(#[from] ServerError),

    /// Direct SSH protocol errors from the underlying library.
    #[error("ssh protocol error: {0}")]
    Russh(#[from] russh::Error),

    /// Errors related to connection tickets.
    #[error("ticket error: {0}")]
    Ticket(#[from] crate::transport::ticket::TicketError),

    /// Errors originating from the authentication subsystem.
    #[error("authentication error: {0}")]
    Auth(#[from] AuthError),

    /// Generic I/O failures.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Authentication with the remote peer failed.
    #[error("authentication failed")]
    AuthenticationFailed,

    /// The remote server identity does not match the pinned trust record.
    #[error("server host key mismatch (expected {expected}, got {actual})")]
    ServerKeyMismatch { expected: String, actual: String },

    /// The requested connection target is invalid or unparseable.
    #[error("invalid connection target: {raw}")]
    InvalidTarget { raw: String },
}

/// A specialized `Result` type for irosh library operations.
pub type Result<T> = std::result::Result<T, IroshError>;

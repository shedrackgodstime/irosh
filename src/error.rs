//! Top-level and subsystem error types for the irosh library.

use std::path::PathBuf;

/// Transport-layer errors.
#[cfg(feature = "transport")]
#[derive(Debug, thiserror::Error)]
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

/// Storage and persistence errors.
#[cfg(feature = "storage")]
#[derive(Debug, thiserror::Error)]
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
        /// The underlying error, if available.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
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
        /// The underlying iroh connection error.
        #[source]
        source: iroh::endpoint::ConnectError,
    },

    /// Opening a bi-directional stream for SSH failed.
    #[error("failed to open SSH transport stream")]
    StreamOpenFailed {
        /// The underlying iroh connection error.
        #[source]
        source: iroh::endpoint::ConnectionError,
    },

    /// A metadata-related operation failed.
    #[error("metadata request failed: {detail}")]
    MetadataFailed {
        /// A description of the metadata failure.
        detail: String,
    },

    /// Negotiating the SSH protocol failed.
    #[error("failed to negotiate SSH protocol")]
    SshNegotiationFailed {
        /// The underlying SSH error.
        #[source]
        source: russh::Error,
    },

    /// The SSH session channel could not be opened.
    #[error("failed to open SSH session channel")]
    ChannelOpenFailed {
        /// The underlying SSH error.
        #[source]
        source: russh::Error,
    },

    /// Requesting a PTY failed.
    #[error("failed to request PTY")]
    PtyRequestFailed {
        /// The underlying SSH error.
        #[source]
        source: russh::Error,
    },

    /// Requesting a shell session failed.
    #[error("failed to request shell")]
    ShellRequestFailed {
        /// The underlying SSH error.
        #[source]
        source: russh::Error,
    },

    /// A command failed to execute.
    #[error("remote command execution failed")]
    ExecFailed {
        /// The underlying SSH error.
        #[source]
        source: russh::Error,
    },

    /// Sending data over the SSH channel failed.
    #[error("failed to send data to remote channel")]
    DataSendFailed {
        /// The underlying SSH error.
        #[source]
        source: russh::Error,
    },

    /// Sending EOF over the SSH channel failed.
    #[error("failed to send EOF")]
    EofSendFailed {
        /// The underlying SSH error.
        #[source]
        source: russh::Error,
    },

    /// Resizing the PTY window failed.
    #[error("failed to resize PTY window")]
    WindowChangeFailed {
        /// The underlying SSH error.
        #[source]
        source: russh::Error,
    },

    /// Disconnecting the SSH session failed.
    #[error("failed to disconnect SSH session")]
    DisconnectFailed {
        /// The underlying SSH error.
        #[source]
        source: russh::Error,
    },

    /// Standard I/O failure on the local terminal.
    #[error("terminal I/O error")]
    TerminalIo {
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The SSH peer disconnected abruptly during the initial handshake.
    #[error("ssh peer disconnected during handshake")]
    SshHandshakeDisconnected {
        /// Optional details about the disconnection.
        detail: Option<String>,
    },

    /// A file upload operation failed.
    #[error("upload failed: {details}")]
    UploadFailed {
        /// A description of what went wrong.
        details: String,
    },

    /// A file download operation failed.
    #[error("download failed: {details}")]
    DownloadFailed {
        /// A description of what went wrong.
        details: String,
    },

    /// A file-level I/O operation failed.
    #[error("failed to {operation} at {path}")]
    FileIo {
        /// A description of the operation that failed.
        operation: &'static str,
        /// The file path involved in the operation.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The transfer target identifier or path is invalid.
    #[error("invalid transfer target: {reason}")]
    TransferTargetInvalid {
        /// The reason the target is invalid.
        reason: &'static str,
    },

    /// The remote peer rejected the transfer request.
    #[error("transfer rejected by remote: {failure}")]
    TransferRejected {
        /// Details of the transfer rejection from the peer.
        failure: crate::transport::transfer::TransferFailure,
    },

    /// A transfer-related control operation failed.
    #[error("transfer control operation failed: {failure}")]
    TransferFailed {
        /// Details of the transfer failure.
        failure: crate::transport::transfer::TransferFailure,
    },

    /// The session transport is not available (disconnected or not initialized).
    #[error("transport unavailable: {details}")]
    TransportUnavailable {
        /// The reason transport is unavailable.
        details: &'static str,
    },

    /// A port forwarding tunnel failed.
    #[error("tunnel failed: {details}")]
    TunnelFailed {
        /// A description of what went wrong.
        details: String,
    },
}

/// Server-side orchestration errors.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// The Iroh endpoint failed to bind.
    #[error("failed to bind server endpoint")]
    EndpointBind {
        /// The underlying iroh bind error.
        #[source]
        source: iroh::endpoint::BindError,
    },

    /// Invalid authentication configuration.
    #[error("invalid auth configuration: {reason}")]
    AuthConfiguration {
        /// The reason the configuration is invalid.
        reason: String,
    },

    /// Identity loading or generation failed.
    #[error("failed to load server identity")]
    IdentityLoad {
        /// The underlying transport error.
        #[source]
        source: iroh::endpoint::TransportError,
    },

    /// SSH server configuration failed.
    #[error("failed to configure SSH server")]
    SshConfig {
        /// The underlying SSH key error.
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    /// A shell process failed to start or manage.
    #[error("remote shell error: {details}")]
    ShellError {
        /// Details about the shell failure.
        details: String,
    },

    /// A channel-level SSH operation failed.
    #[error("channel error during {operation}: {details}")]
    ChannelError {
        /// Description of the operation that was in progress.
        operation: &'static str,
        /// More details about the error.
        details: String,
    },

    /// A file transfer operation failed on the server.
    #[error("server transfer error: {failure}")]
    TransferFailed {
        /// Details of the transfer failure.
        failure: crate::transport::transfer::TransferFailure,
    },

    /// The remote peer provided an invalid transfer path.
    #[error("invalid transfer path: {details}")]
    InvalidPath {
        /// Explanation of why the path is invalid.
        details: String,
    },

    /// Failed to format an SSH host key for display.
    #[error("failed to format host key")]
    FormatHostKey {
        /// The underlying SSH key error.
        #[source]
        source: russh::keys::ssh_key::Error,
    },

    /// A blocking storage task (e.g. key generation) failed.
    #[error("blocking storage task failed during {operation}")]
    BlockingTaskFailed {
        /// Description of the operation that was in progress.
        operation: &'static str,
        /// The Tokio join error.
        #[source]
        source: tokio::task::JoinError,
    },

    /// Failed to query OS process information by PID.
    #[error("failed to query process information for PID {pid}: {details}")]
    ProcessQueryFailed {
        /// The process ID that was queried.
        pid: u32,
        /// Details about the failure.
        details: String,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failure during OS service management (install/start/stop).
    #[error("service management failure: {details}")]
    ServiceManagement {
        /// Details about the service failure.
        details: String,
    },
}

/// Top-level crate error unifying all subsystem failures.
#[derive(Debug, thiserror::Error)]
pub enum IroshError {
    /// The current platform is not supported by this feature.
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
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// Authentication with the remote peer failed.
    #[error("authentication failed")]
    AuthenticationFailed,

    /// The remote server identity does not match the pinned trust record.
    #[error("server host key mismatch (expected {expected}, got {actual})")]
    ServerKeyMismatch {
        /// The expected host key fingerprint.
        expected: String,
        /// The actual host key fingerprint presented by the server.
        actual: String,
    },

    /// The requested connection target is invalid or unparseable.
    #[error("invalid connection target: {raw}")]
    InvalidTarget {
        /// The raw target string that could not be parsed.
        raw: String,
    },
    /// RPC errors from iroh-blobs.
    #[error("rpc error: {0}")]
    Rpc(String),
}

/// A specialized `Result` type for irosh library operations.
pub type Result<T> = std::result::Result<T, IroshError>;
impl From<crate::transport::transfer::TransferError> for IroshError {
    fn from(e: crate::transport::transfer::TransferError) -> Self {
        Self::Transport(TransportError::Transfer(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_error_display_invalid_password() {
        let err = AuthError::InvalidPassword;
        assert_eq!(err.to_string(), "invalid password provided");
    }

    #[test]
    fn auth_error_display_unsupported_method() {
        let err = AuthError::UnsupportedMethod("gssapi".into());
        assert_eq!(err.to_string(), "unsupported authentication method: gssapi");
    }

    #[test]
    fn auth_error_display_missing_credential() {
        let err = AuthError::MissingCredential("password".into());
        assert_eq!(err.to_string(), "missing required credential: password");
    }

    #[test]
    fn client_error_display_transport_unavailable() {
        let err = ClientError::TransportUnavailable {
            details: "not connected",
        };
        assert_eq!(err.to_string(), "transport unavailable: not connected");
    }

    #[test]
    fn client_error_display_transfer_target_invalid() {
        let err = ClientError::TransferTargetInvalid {
            reason: "empty path",
        };
        assert_eq!(err.to_string(), "invalid transfer target: empty path");
    }

    #[test]
    fn client_error_display_tunnel_failed() {
        let err = ClientError::TunnelFailed {
            details: "port in use".into(),
        };
        assert_eq!(err.to_string(), "tunnel failed: port in use");
    }

    #[test]
    fn server_error_display_shell_error() {
        let err = ServerError::ShellError {
            details: "exec failed".into(),
        };
        assert_eq!(err.to_string(), "remote shell error: exec failed");
    }

    #[test]
    fn server_error_display_service_management() {
        let err = ServerError::ServiceManagement {
            details: "permission denied".into(),
        };
        assert_eq!(
            err.to_string(),
            "service management failure: permission denied"
        );
    }

    #[test]
    fn irosh_error_display_platform_not_supported() {
        let err = IroshError::PlatformNotSupported("windows 95".into());
        assert_eq!(err.to_string(), "platform not supported: windows 95");
    }

    #[test]
    fn irosh_error_display_authentication_failed() {
        let err = IroshError::AuthenticationFailed;
        assert_eq!(err.to_string(), "authentication failed");
    }

    #[test]
    fn irosh_error_display_server_key_mismatch() {
        let err = IroshError::ServerKeyMismatch {
            expected: "abc".into(),
            actual: "def".into(),
        };
        assert_eq!(
            err.to_string(),
            "server host key mismatch (expected abc, got def)"
        );
    }

    #[test]
    fn irosh_error_display_invalid_target() {
        let err = IroshError::InvalidTarget {
            raw: "not-a-ticket".into(),
        };
        assert_eq!(err.to_string(), "invalid connection target: not-a-ticket");
    }

    #[test]
    fn irosh_error_display_rpc() {
        let err = IroshError::Rpc("timeout".into());
        assert_eq!(err.to_string(), "rpc error: timeout");
    }

    #[test]
    fn irosh_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err = IroshError::from(io_err);
        assert!(err.to_string().contains("i/o error"));
    }

    #[test]
    fn irosh_error_from_auth_error() {
        let auth_err = AuthError::InvalidPassword;
        let err = IroshError::from(auth_err);
        assert!(err.to_string().contains("authentication error"));
    }

    #[cfg(feature = "storage")]
    #[test]
    fn storage_error_display_directory_create() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err = StorageError::DirectoryCreate {
            path: "/tmp/irosh".into(),
            source: io_err,
        };
        assert!(err.to_string().contains("failed to create directory at"));
    }

    #[cfg(feature = "storage")]
    #[test]
    fn storage_error_display_peer_name_invalid() {
        let err = StorageError::PeerNameInvalid {
            name: "../foo".into(),
        };
        assert_eq!(err.to_string(), "invalid peer name: ../foo");
    }

    #[cfg(feature = "storage")]
    #[test]
    fn irosh_error_from_storage_error() {
        let err = StorageError::PeerNameInvalid { name: "bad".into() };
        let irosh_err = IroshError::from(err);
        assert!(irosh_err.to_string().contains("storage error"));
    }

    #[cfg(feature = "transport")]
    #[test]
    fn transport_error_display_ticket_format() {
        let err = TransportError::TicketFormatInvalid;
        assert_eq!(err.to_string(), "invalid connection ticket format");
    }

    #[cfg(feature = "transport")]
    #[test]
    fn transport_error_display_invalid_relay_url() {
        let err = TransportError::InvalidRelayUrl {
            url: "bad://url".into(),
        };
        assert_eq!(err.to_string(), "invalid relay URL: bad://url");
    }

    #[cfg(feature = "transport")]
    #[test]
    fn transport_error_display_protocol_error() {
        let err = TransportError::ProtocolError {
            details: "unexpected message".into(),
        };
        assert_eq!(err.to_string(), "protocol violation: unexpected message");
    }
}

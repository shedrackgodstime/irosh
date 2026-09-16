//! Client-side session and lifecycle errors.

use std::path::PathBuf;

/// Client-side session and lifecycle errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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

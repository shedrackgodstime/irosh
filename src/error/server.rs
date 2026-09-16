//! Server-side orchestration errors.

/// Server-side orchestration errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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

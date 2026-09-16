//! Top-level and subsystem error types for the irosh library.
//!
//! Each subsystem (`auth`, `client`, `server`, `storage`, `transport`) owns its
//! error enum here; [`IroshError`] unifies them and converts automatically via
//! `?` given the `From` impls on the [`IroshError`] variants.

mod auth;
mod client;
mod server;
#[cfg(feature = "storage")]
mod storage;
#[cfg(feature = "transport")]
mod transport;

pub use auth::AuthError;
pub use client::ClientError;
pub use server::ServerError;
#[cfg(feature = "storage")]
pub use storage::StorageError;
#[cfg(feature = "transport")]
pub use transport::TransportError;

/// Top-level crate error unifying all subsystem failures.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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

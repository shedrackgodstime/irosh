//! CLI-level error classification — typed matching on library errors.
//!
//! The library exposes structured `thiserror` types. This module walks
//! the error chain to classify failures without string matching.

use crate::ui::messages;
use irosh::error::{AuthError, ClientError, IroshError, StorageError, TransportError};

/// Classified error variants for CLI-level error reporting.
///
/// This enum is never propagated — it's only used to determine the
/// right user-facing tip for a given `anyhow::Error` from the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CliError {
    ConnectionRefused,
    ConnectionTimeout,
    AuthWrongPassword,
    AuthKeyRejected,
    WormholeTimeout,
    PeerNotFound,
    InvalidTarget,
    DaemonConflict,
    PermissionDenied,
    FileNotFound,
    BlobStore,
    Unknown,
}

impl CliError {
    /// Classify an [`anyhow::Error`] by walking its source chain and
    /// matching against the library's typed error enums.
    pub(crate) fn classify(e: &anyhow::Error) -> Self {
        use std::error::Error;

        // Walk the chain: top-level error → source → source ...
        let mut current: Option<&(dyn Error + 'static)> = Some(e.as_ref());
        while let Some(err) = current {
            // IroshError — top-level unifying enum
            if let Some(ie) = err.downcast_ref::<IroshError>() {
                match ie {
                    IroshError::Auth(a) => match a {
                        AuthError::InvalidPassword => return Self::AuthWrongPassword,
                        _ => return Self::AuthKeyRejected,
                    },
                    IroshError::AuthenticationFailed => return Self::AuthWrongPassword,
                    IroshError::ServerKeyMismatch { .. } => return Self::AuthKeyRejected,
                    IroshError::Client(ClientError::ConnectFailed { .. }) => {
                        return Self::ConnectionRefused;
                    }
                    IroshError::InvalidTarget { .. } => return Self::InvalidTarget,
                    _ => {}
                }
            }

            // TransportError — connection / wire
            if let Some(te) = err.downcast_ref::<TransportError>() {
                match te {
                    TransportError::ConnectionLost { .. } => return Self::ConnectionTimeout,
                    TransportError::TicketFormatInvalid => return Self::InvalidTarget,
                    _ => {}
                }
            }

            // StorageError — persistence
            if let Some(StorageError::PeerNotFound { .. }) = err.downcast_ref::<StorageError>() {
                return Self::PeerNotFound;
            }

            // Service-specific errors that don't go through IroshError
            if let Some(ClientError::ConnectFailed { .. }) = err.downcast_ref::<ClientError>() {
                return Self::ConnectionRefused;
            }

            // std::io::Error — direct I/O
            if let Some(ioe) = err.downcast_ref::<std::io::Error>() {
                match ioe.kind() {
                    std::io::ErrorKind::ConnectionRefused => return Self::ConnectionRefused,
                    std::io::ErrorKind::TimedOut => return Self::ConnectionTimeout,
                    std::io::ErrorKind::PermissionDenied => return Self::PermissionDenied,
                    std::io::ErrorKind::NotFound => return Self::FileNotFound,
                    _ => {}
                }
            }

            // Fallback: string content for anyhow! / bail! errors that
            // don't wrap a typed library error.
            let msg = err.to_string().to_lowercase();
            if msg.contains("wormhole") && (msg.contains("not found") || msg.contains("no peer")) {
                return Self::WormholeTimeout;
            }
            if msg.contains("identity conflict") || msg.contains("already running") {
                return Self::DaemonConflict;
            }
            if msg.contains("blobs") || msg.contains("store") {
                return Self::BlobStore;
            }
            if msg.contains("connection refused")
                || msg.contains("connect failed")
                || msg.contains("timed out")
            {
                return Self::ConnectionRefused;
            }
            if msg.contains("password")
                && (msg.contains("incorrect") || msg.contains("wrong") || msg.contains("invalid"))
            {
                return Self::AuthWrongPassword;
            }
            if msg.contains("no such file") || msg.contains("not found") {
                return Self::FileNotFound;
            }
            if msg.contains("permission denied") {
                return Self::PermissionDenied;
            }

            current = err.source();
        }

        Self::Unknown
    }

    /// Return the user-facing tip constant for this error variant.
    pub(crate) fn tip(&self) -> &'static str {
        match self {
            Self::ConnectionRefused | Self::ConnectionTimeout => messages::TIP_CONNECTION_REFUSED,
            Self::AuthWrongPassword => messages::TIP_AUTH_WRONG_PASSWORD,
            Self::AuthKeyRejected => messages::TIP_AUTH_KEY_REJECTED,
            Self::WormholeTimeout => messages::TIP_WORMHOLE_TIMEOUT,
            Self::PeerNotFound | Self::InvalidTarget => messages::TIP_PEER_LIST,
            Self::DaemonConflict => messages::TIP_DAEMON_STATUS,
            Self::PermissionDenied => messages::TIP_CHECK_DIAGNOSTIC,
            Self::FileNotFound => messages::TIP_VERIFY_PATH,
            Self::BlobStore => messages::TIP_BLOB_STORE,
            Self::Unknown => messages::TIP_FALLBACK,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_wrong_password_from_invalid_password() {
        let ae = AuthError::InvalidPassword;
        let ie = IroshError::Auth(ae);
        let e = anyhow::Error::from(ie);
        assert_eq!(CliError::classify(&e), CliError::AuthWrongPassword);
    }

    #[test]
    fn auth_key_rejected_from_server_key_mismatch() {
        let ie = IroshError::ServerKeyMismatch {
            expected: "abc".into(),
            actual: "def".into(),
        };
        let e = anyhow::Error::from(ie);
        assert_eq!(CliError::classify(&e), CliError::AuthKeyRejected);
    }

    #[test]
    fn connection_refused_from_connect_failed() {
        let ce = ClientError::ConnectFailed {
            source: irosh::iroh::endpoint::ConnectError::from(
                irosh::iroh::endpoint::ConnectionError::Reset,
            ),
        };
        let ie = IroshError::Client(ce);
        let e = anyhow::Error::from(ie);
        assert_eq!(CliError::classify(&e), CliError::ConnectionRefused);
    }

    #[test]
    fn wormhole_timeout_from_string() {
        let e = anyhow::anyhow!("wormhole code not found: whale-jungle-8");
        assert_eq!(CliError::classify(&e), CliError::WormholeTimeout);
    }

    #[test]
    fn daemon_conflict_from_string() {
        let e = anyhow::anyhow!("identity conflict: daemon already running");
        assert_eq!(CliError::classify(&e), CliError::DaemonConflict);
    }

    #[test]
    fn invalid_target_returns_peer_list_tip() {
        let ie = IroshError::InvalidTarget {
            raw: "not-a-ticket".into(),
        };
        let e = anyhow::Error::from(ie);
        assert_eq!(CliError::classify(&e), CliError::InvalidTarget);
    }

    #[test]
    fn fallback_for_unknown_error() {
        let e = anyhow::anyhow!("something completely unexpected happened");
        assert_eq!(CliError::classify(&e), CliError::Unknown);
    }

    #[test]
    fn tip_returns_right_constant() {
        assert_eq!(
            CliError::AuthWrongPassword.tip(),
            messages::TIP_AUTH_WRONG_PASSWORD
        );
        assert_eq!(CliError::Unknown.tip(), messages::TIP_FALLBACK);
    }
}

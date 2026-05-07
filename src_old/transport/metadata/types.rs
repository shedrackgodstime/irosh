use std::fmt;

use serde::{Deserialize, Serialize};

/// Connection metadata optionally exchanged on a separate control stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerMetadata {
    /// The remote machine's hostname.
    pub hostname: String,
    /// The remote machine's user.
    pub user: String,
    /// The remote machine's operating system.
    pub os: String,
}

impl PeerMetadata {
    /// Generates a friendly default alias like "kristency-linux".
    pub fn default_alias(&self) -> String {
        let clean_user = self.user.replace(' ', "-").to_lowercase();
        let clean_os = self.os.replace(' ', "-").to_lowercase();
        format!("{}-{}", clean_user, clean_os)
    }

    /// Collects the current system's metadata to send to a connecting peer.
    pub fn current() -> Self {
        Self {
            hostname: std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown-host".to_string()),
            user: std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "unknown-user".to_string()),
            os: std::env::consts::OS.to_string(),
        }
    }
}

/// Error type for metadata framing and I/O.
#[derive(Debug)]
pub enum MetadataError {
    Io(std::io::Error),
    InvalidMagic,
    UnsupportedVersion(u8),
    UnsupportedKind(u8),
    UnexpectedKind { expected: u8, actual: u8 },
    PayloadTooLarge(usize),
    Json(serde_json::Error),
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetadataError::Io(err) => write!(f, "metadata I/O error: {}", err),
            MetadataError::InvalidMagic => write!(f, "invalid metadata magic header"),
            MetadataError::UnsupportedVersion(version) => {
                write!(f, "unsupported metadata version: {}", version)
            }
            MetadataError::UnsupportedKind(kind) => {
                write!(f, "unsupported metadata frame kind: {}", kind)
            }
            MetadataError::UnexpectedKind { expected, actual } => {
                write!(
                    f,
                    "unexpected metadata frame kind: expected {}, got {}",
                    expected, actual
                )
            }
            MetadataError::PayloadTooLarge(size) => {
                write!(f, "metadata payload too large: {} bytes", size)
            }
            MetadataError::Json(err) => write!(f, "invalid metadata payload: {}", err),
        }
    }
}

impl std::error::Error for MetadataError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MetadataError::Io(err) => Some(err),
            MetadataError::Json(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for MetadataError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for MetadataError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

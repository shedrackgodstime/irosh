use std::fmt;

use serde::{Deserialize, Serialize};

/// Maximum control payload size.
pub(crate) const MAX_CONTROL_BYTES: usize = 8 * 1024;
/// Maximum chunk payload size.
pub const MAX_CHUNK_BYTES: usize = 64 * 1024;

/// An upload request from client to server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PutRequest {
    pub path: String,
    pub size: u64,
    pub mode: Option<u32>,
}

/// A download request from client to server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GetRequest {
    pub path: String,
}

/// A ready response that includes the expected file size.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferReady {
    pub size: u64,
    pub mode: Option<u32>,
}

/// A transfer completion marker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferComplete {
    pub size: u64,
}

/// A terminal transfer error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransferFailureCode {
    RemoteShellUnavailable,
    TargetAlreadyExists,
    PathInvalid,
    CreateDirectoryFailed,
    SizeMismatch,
    UnexpectedFrame,
    HelperFailed,
    AtomicRenameFailed,
    Rejected,
    Internal,
}

/// A terminal transfer error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferFailure {
    pub code: TransferFailureCode,
    pub detail: String,
}

impl TransferFailure {
    pub fn new(code: TransferFailureCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }

    fn label(&self) -> &'static str {
        match self.code {
            TransferFailureCode::RemoteShellUnavailable => "remote shell unavailable",
            TransferFailureCode::TargetAlreadyExists => "target already exists",
            TransferFailureCode::PathInvalid => "transfer path invalid",
            TransferFailureCode::CreateDirectoryFailed => "creating remote directory failed",
            TransferFailureCode::SizeMismatch => "transfer size mismatch",
            TransferFailureCode::UnexpectedFrame => "unexpected transfer frame",
            TransferFailureCode::HelperFailed => "transfer helper failed",
            TransferFailureCode::AtomicRenameFailed => "atomic rename failed",
            TransferFailureCode::Rejected => "transfer rejected",
            TransferFailureCode::Internal => "transfer internal error",
        }
    }
}

impl fmt::Display for TransferFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.detail.is_empty() {
            write!(f, "{}", self.label())
        } else {
            write!(f, "{}: {}", self.label(), self.detail)
        }
    }
}

impl std::error::Error for TransferFailure {}

/// An empty request for the current working directory of the live remote shell.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CwdRequest;

/// A response containing the current working directory of the live remote shell.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CwdResponse {
    pub path: String,
}

/// A request to check if a remote path exists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExistsRequest {
    pub path: String,
}

/// A response indicating whether a remote path exists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExistsResponse {
    pub exists: bool,
}

/// A decoded transfer frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferFrame {
    PutRequest(PutRequest),
    PutReady(TransferReady),
    PutChunk(Vec<u8>),
    PutComplete(TransferComplete),
    GetRequest(GetRequest),
    GetReady(TransferReady),
    GetChunk(Vec<u8>),
    GetComplete(TransferComplete),
    CwdRequest(CwdRequest),
    CwdResponse(CwdResponse),
    ExistsRequest(ExistsRequest),
    ExistsResponse(ExistsResponse),
    Error(TransferFailure),
}

/// Error type for transfer framing and parsing.
#[derive(Debug)]
pub enum TransferError {
    Io(std::io::Error),
    InvalidMagic,
    UnsupportedVersion(u8),
    UnsupportedKind(u8),
    UnexpectedKind { expected: u8, actual: u8 },
    PayloadTooLarge(usize),
    Json(serde_json::Error),
}

impl fmt::Display for TransferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransferError::Io(err) => write!(f, "transfer I/O error: {}", err),
            TransferError::InvalidMagic => write!(f, "invalid transfer magic header"),
            TransferError::UnsupportedVersion(version) => {
                write!(f, "unsupported transfer version: {}", version)
            }
            TransferError::UnsupportedKind(kind) => {
                write!(f, "unsupported transfer frame kind: {}", kind)
            }
            TransferError::UnexpectedKind { expected, actual } => {
                write!(
                    f,
                    "unexpected transfer frame kind: expected {}, got {}",
                    expected, actual
                )
            }
            TransferError::PayloadTooLarge(size) => {
                write!(f, "transfer payload too large: {} bytes", size)
            }
            TransferError::Json(err) => write!(f, "invalid transfer payload: {}", err),
        }
    }
}

impl std::error::Error for TransferError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TransferError::Io(err) => Some(err),
            TransferError::Json(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for TransferError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for TransferError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

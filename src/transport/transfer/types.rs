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
    #[serde(default)]
    pub recursive: bool,
}

/// A download request from client to server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GetRequest {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
}

/// A ready response that includes the expected file size.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferReady {
    pub size: u64,
    pub mode: Option<u32>,
}

/// A header for a new entry in a recursive transfer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryHeader {
    pub path: String,
    pub size: u64,
    pub mode: Option<u32>,
    pub is_dir: bool,
}

/// A marker for the end of an entry's data in a recursive transfer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EntryComplete;

/// A transfer completion marker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferComplete {
    pub size: u64,
}

/// Specific codes indicating why a file transfer was terminated or rejected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransferFailureCode {
    /// The remote machine does not have a live shell available to determine CWD.
    RemoteShellUnavailable,
    /// The upload target already exists on the remote filesystem.
    TargetAlreadyExists,
    /// The provided transfer path is invalid or malformed.
    PathInvalid,
    /// Failed to create a directory on the remote filesystem.
    CreateDirectoryFailed,
    /// The received byte count does not match the expected file size.
    SizeMismatch,
    /// An unexpected protocol frame kind was received.
    UnexpectedFrame,
    /// An external helper (like `tar`) failed during the transfer.
    HelperFailed,
    /// Failed to atomically move the temporary file to its final destination.
    AtomicRenameFailed,
    /// The remote server explicitly rejected the transfer request.
    Rejected,
    /// An unrecoverable internal error occurred in the transfer engine.
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

/// An request to check if a remote path exists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExistsRequest {
    pub path: String,
}

/// A response indicating whether a remote path exists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExistsResponse {
    pub exists: bool,
    #[serde(default)]
    pub is_dir: bool,
}

/// A request for tab completion from the remote server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionRequest {
    pub path: String,
}

/// A response containing possible completion matches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionResponse {
    pub matches: Vec<String>,
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
    CompletionRequest(CompletionRequest),
    CompletionResponse(CompletionResponse),
    NewEntry(EntryHeader),
    EntryComplete(EntryComplete),
    Error(TransferFailure),
}

/// Low-level errors occurring during transfer framing, parsing, or transport I/O.
#[derive(Debug)]
pub enum TransferError {
    /// A standard library I/O error.
    Io(std::io::Error),
    /// The stream header does not match the expected magic bytes.
    InvalidMagic,
    /// The remote peer is using an incompatible protocol version.
    UnsupportedVersion(u8),
    /// An unknown or unhandled frame kind was received.
    UnsupportedKind(u8),
    /// Received a frame kind that was invalid for the current state.
    UnexpectedKind { expected: u8, actual: u8 },
    /// The received control payload exceeds the maximum allowed size.
    PayloadTooLarge(usize),
    /// Failed to parse or serialize a JSON control payload.
    Json(serde_json::Error),
    /// The received path is invalid or contains forbidden components.
    InvalidPath(String),
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
            TransferError::InvalidPath(details) => write!(f, "invalid transfer path: {}", details),
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

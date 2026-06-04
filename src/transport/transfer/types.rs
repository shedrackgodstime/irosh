//! Transfer frame types.
use std::fmt;

use serde::{Deserialize, Serialize};

/// Maximum control payload size.
pub(crate) const MAX_CONTROL_BYTES: usize = 8 * 1024;
/// Maximum chunk payload size.
pub const MAX_CHUNK_BYTES: usize = 64 * 1024;

/// Highest kind supported by v0.3.0 and earlier (pre-blob era).
pub(crate) const LEGACY_MAX_KIND: u8 = 17;
/// Highest kind supported by the current version.
pub(crate) const CURRENT_MAX_KIND: u8 = 20;

/// An upload request from client to server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PutRequest {
    /// The destination path on the remote filesystem.
    pub path: String,
    /// The total size of the upload in bytes.
    pub size: u64,
    /// Optional file permissions (Unix mode bits).
    pub mode: Option<u32>,
    /// If `true`, upload the directory tree rooted at [`path`](Self::path)
    /// instead of a single file.
    #[serde(default)]
    pub recursive: bool,
}

/// A download request from client to server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GetRequest {
    /// The remote path to download from.
    pub path: String,
    /// If `true`, download the directory tree rooted at [`path`](Self::path).
    #[serde(default)]
    pub recursive: bool,
}

/// A request to push a content-addressed blob.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlobPutRequest {
    /// The local path of the blob to upload.
    pub path: String,
    /// The content hash of the blob.
    pub hash: String,
    /// The blob format identifier.
    pub format: String,
}

/// A request to pull a content-addressed blob.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlobGetRequest {
    /// The destination path on the remote filesystem.
    pub path: String,
}

/// A response indicating the blob is ready to be downloaded via iroh-blobs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlobGetReady {
    /// The content hash of the blob.
    pub hash: String,
    /// The blob format identifier.
    pub format: String,
    /// Total size of the blob in bytes.
    pub size: u64,
}

/// A ready response that includes the expected file size.
///
/// # Invariants
///
/// - `size` must equal the total bytes that will be transferred across all
///   subsequent [`TransferFrame::Data`] chunks for this entry.
/// - Individual chunk payloads must not exceed [`MAX_CHUNK_BYTES`] (64 KiB).
/// - For recursive transfers, each [`EntryHeader`] is followed by its data
///   chunks and an [`EntryComplete`] marker before the next entry begins.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferReady {
    /// Total expected size in bytes.
    pub size: u64,
    /// Optional file permissions (Unix mode bits).
    pub mode: Option<u32>,
}

/// A header for a new entry in a recursive transfer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryHeader {
    /// Relative path of the entry within the transfer.
    pub path: String,
    /// Size of the entry in bytes.
    pub size: u64,
    /// Optional file permissions (Unix mode bits).
    pub mode: Option<u32>,
    /// Whether this entry is a directory.
    pub is_dir: bool,
}

/// A marker for the end of an entry's data in a recursive transfer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EntryComplete;

/// A transfer completion marker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferComplete {
    /// Total bytes transferred.
    pub size: u64,
}

/// Specific codes indicating why a file transfer was terminated or rejected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
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
    /// The requested path was not found on the filesystem.
    NotFound,
    /// The requested path is a directory but the recursive flag was not set.
    IsDirectory,
    /// An unrecoverable internal error occurred in the transfer engine.
    Internal,
}

/// A terminal transfer error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferFailure {
    /// The high-level failure category.
    pub code: TransferFailureCode,
    /// Human-readable explanation of the failure.
    pub detail: String,
}

impl TransferFailure {
    /// Creates a new transfer failure with the given code and detail message.
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
            TransferFailureCode::NotFound => "path not found",
            TransferFailureCode::IsDirectory => "path is a directory",
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
    /// The absolute path of the remote working directory.
    pub path: String,
}

/// An request to check if a remote path exists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExistsRequest {
    /// The remote path to check.
    pub path: String,
}

/// A response indicating whether a remote path exists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExistsResponse {
    /// Whether the path exists on the remote filesystem.
    pub exists: bool,
    /// Whether the path is a directory (if it exists).
    #[serde(default)]
    pub is_dir: bool,
}

/// A request for tab completion from the remote server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionRequest {
    /// The path prefix to complete.
    pub path: String,
}

/// A response containing possible completion matches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionResponse {
    /// The list of matching paths or filenames.
    pub matches: Vec<String>,
}

/// Capability advertisement sent as the first frame on a new transfer stream.
///
/// Used to negotiate the set of frame kinds both peers support.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capability {
    /// The highest frame kind this peer supports.
    pub max_kind: u8,
}

/// A decoded transfer frame.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransferFrame {
    /// Capability negotiation frame (sent first on new streams).
    Capability(Capability),
    /// The client wishes to upload a file.
    PutRequest(PutRequest),
    /// The server is ready to accept upload data.
    PutReady(TransferReady),
    /// A chunk of upload payload data.
    PutChunk(Vec<u8>),
    /// The upload is complete.
    PutComplete(TransferComplete),
    /// The client wishes to download a file.
    GetRequest(GetRequest),
    /// The server is ready to begin sending download data.
    GetReady(TransferReady),
    /// A chunk of download payload data.
    GetChunk(Vec<u8>),
    /// The download is complete.
    GetComplete(TransferComplete),
    /// Query the remote shell's current working directory.
    CwdRequest(CwdRequest),
    /// Response containing the remote shell's current working directory.
    CwdResponse(CwdResponse),
    /// Check whether a path exists on the remote filesystem.
    ExistsRequest(ExistsRequest),
    /// Response indicating whether a remote path exists.
    ExistsResponse(ExistsResponse),
    /// Request tab-completion suggestions from the remote server.
    CompletionRequest(CompletionRequest),
    /// Response containing tab-completion matches.
    CompletionResponse(CompletionResponse),
    /// Request to upload a content-addressed blob.
    BlobPutRequest(BlobPutRequest),
    /// Request to download a content-addressed blob.
    BlobGetRequest(BlobGetRequest),
    /// Notification that a blob is ready for download via iroh-blobs.
    BlobGetReady(BlobGetReady),
    /// Header for a new entry in a recursive transfer.
    NewEntry(EntryHeader),
    /// Marker that an entry's data is complete (recursive transfer).
    EntryComplete(EntryComplete),
    /// A terminal transfer error.
    Error(TransferFailure),
}

/// Low-level errors occurring during transfer framing, parsing, or transport I/O.
#[derive(Debug)]
#[non_exhaustive]
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
    UnexpectedKind {
        /// The frame kind the receiver was expecting.
        expected: u8,
        /// The frame kind that was actually received.
        actual: u8,
    },
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
            TransferError::Io(err) => write!(f, "transfer I/O error: {err}"),
            TransferError::InvalidMagic => write!(f, "invalid transfer magic header"),
            TransferError::UnsupportedVersion(version) => {
                write!(f, "unsupported transfer version: {version}")
            }
            TransferError::UnsupportedKind(kind) => {
                write!(f, "unsupported transfer frame kind: {kind}")
            }
            TransferError::UnexpectedKind { expected, actual } => {
                write!(
                    f,
                    "unexpected transfer frame kind: expected {expected}, got {actual}"
                )
            }
            TransferError::PayloadTooLarge(size) => {
                write!(f, "transfer payload too large: {size} bytes")
            }
            TransferError::Json(err) => write!(f, "invalid transfer payload: {err}"),
            TransferError::InvalidPath(details) => write!(f, "invalid transfer path: {details}"),
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

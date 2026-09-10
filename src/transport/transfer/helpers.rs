//! Transfer path sanitization.
use crate::error::{Result, TransportError};
use std::path::{Component, Path, PathBuf};

/// Sanitizes a path received over the network to prevent path traversal.
///
/// This ensures the path is relative and does not contain components that would
/// escape the base directory (like `..` or absolute roots).
///
/// # Errors
///
/// Returns an error if the path is absolute, contains null bytes, attempts
/// path traversal via `..`, or resolves to an empty path.
#[must_use]
pub fn sanitize_remote_path(raw: &str) -> Result<PathBuf> {
    if raw.contains('\0') {
        return Err(crate::error::IroshError::Transport(
            TransportError::Transfer(crate::transport::transfer::TransferError::InvalidPath(
                "path contains null byte".to_string(),
            )),
        ));
    }

    let raw_path = Path::new(raw);

    // We strictly forbid absolute paths from the network.
    if raw_path.is_absolute() {
        return Err(crate::error::IroshError::Transport(
            TransportError::Transfer(crate::transport::transfer::TransferError::InvalidPath(
                format!("absolute path not allowed: {raw}"),
            )),
        ));
    }

    let mut sanitized = PathBuf::new();
    for component in raw_path.components() {
        match component {
            Component::Normal(c) => {
                #[cfg(windows)]
                validate_windows_component(c)?;
                sanitized.push(c);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                // We do not allow '..' to pop above the current sanitized root.
                // This prevents "root/../../etc/passwd" from becoming "/etc/passwd".
                if !sanitized.pop() {
                    return Err(crate::error::IroshError::Transport(
                        TransportError::Transfer(
                            crate::transport::transfer::TransferError::InvalidPath(format!(
                                "path traversal attempt detected: {raw}"
                            )),
                        ),
                    ));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(crate::error::IroshError::Transport(
                    TransportError::Transfer(
                        crate::transport::transfer::TransferError::InvalidPath(format!(
                            "root or prefix components not allowed: {raw}"
                        )),
                    ),
                ));
            }
        }
    }

    if sanitized.as_os_str().is_empty() {
        return Err(crate::error::IroshError::Transport(
            TransportError::Transfer(crate::transport::transfer::TransferError::InvalidPath(
                "sanitized path is empty".to_string(),
            )),
        ));
    }

    Ok(sanitized)
}

/// Normalizes a path to forward-slash separators for wire transmission.
///
/// Path separators are platform-specific, but the transfer protocol always uses
/// `/` between components regardless of the sender's OS. Without this, a
/// Windows peer serializes `dir\file` (backslash), which a Unix receiver would
/// treat as a single literal filename containing a backslash.
#[must_use]
pub fn normalize_path_separators(raw: &str) -> String {
    if cfg!(windows) {
        raw.replace('\\', "/")
    } else {
        raw.to_string()
    }
}

/// Windows reserved device filenames (compared case-insensitively against the
/// stem before the first `.`).
#[cfg(windows)]
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Rejects path components that are invalid or dangerous on the Windows
/// filesystem: reserved device names (`NUL`, `CON`, ...) and characters that
/// are illegal in file names (`< > " | ? * :`).
#[cfg(windows)]
fn validate_windows_component(component: &std::ffi::OsStr) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const ILLEGAL: [u16; 7] = [
        b'<' as u16,
        b'>' as u16,
        b'"' as u16,
        b'|' as u16,
        b'?' as u16,
        b'*' as u16,
        b':' as u16,
    ];

    if component.encode_wide().any(|unit| ILLEGAL.contains(&unit)) {
        return Err(crate::error::IroshError::Transport(
            TransportError::Transfer(crate::transport::transfer::TransferError::InvalidPath(
                format!("component {component:?} contains characters illegal on Windows"),
            )),
        ));
    }

    let text = component.to_string_lossy();
    let stem = text.split('.').next().unwrap_or_default();
    if WINDOWS_RESERVED_NAMES.contains(&stem.to_ascii_lowercase().as_str()) {
        return Err(crate::error::IroshError::Transport(
            TransportError::Transfer(crate::transport::transfer::TransferError::InvalidPath(
                format!("component {text:?} is a reserved Windows filename"),
            )),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_remote_path() {
        // Valid relative paths
        assert_eq!(
            sanitize_remote_path("file.txt").unwrap(),
            PathBuf::from("file.txt")
        );
        assert_eq!(
            sanitize_remote_path("dir/file.txt").unwrap(),
            PathBuf::from("dir/file.txt")
        );
        assert_eq!(
            sanitize_remote_path("./file.txt").unwrap(),
            PathBuf::from("file.txt")
        );

        // Block absolute
        assert!(sanitize_remote_path("/etc/passwd").is_err());

        // Block traversal
        assert!(sanitize_remote_path("../file.txt").is_err());
        assert!(sanitize_remote_path("dir/../../file.txt").is_err());

        // Allow internal .. as long as it doesn't escape
        assert_eq!(
            sanitize_remote_path("dir/subdir/../file.txt").unwrap(),
            PathBuf::from("dir/file.txt")
        );

        // Block null bytes
        assert!(sanitize_remote_path("file\0.txt").is_err());
    }
}

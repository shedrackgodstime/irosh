use crate::error::{Result, TransportError};
use std::path::{Component, Path, PathBuf};

/// Sanitizes a path received over the network to prevent path traversal.
///
/// This ensures the path is relative and does not contain components that would
/// escape the base directory (like `..` or absolute roots).
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
                format!("absolute path not allowed: {}", raw),
            )),
        ));
    }

    let mut sanitized = PathBuf::new();
    for component in raw_path.components() {
        match component {
            Component::Normal(c) => sanitized.push(c),
            Component::CurDir => {}
            Component::ParentDir => {
                // We do not allow '..' to pop above the current sanitized root.
                // This prevents "root/../../etc/passwd" from becoming "/etc/passwd".
                if !sanitized.pop() {
                    return Err(crate::error::IroshError::Transport(
                        TransportError::Transfer(
                            crate::transport::transfer::TransferError::InvalidPath(format!(
                                "path traversal attempt detected: {}",
                                raw
                            )),
                        ),
                    ));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(crate::error::IroshError::Transport(
                    TransportError::Transfer(
                        crate::transport::transfer::TransferError::InvalidPath(format!(
                            "root or prefix components not allowed: {}",
                            raw
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

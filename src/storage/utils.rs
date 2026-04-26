//! Common utilities for secure and atomic storage operations.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::error::{Result, StorageError};

/// Atomically writes data to a file by writing to a temporary file first
/// and then performing an OS-level rename.
///
/// This also ensures the file has strict permissions (0600) on Unix-like systems.
pub fn atomic_write_secure(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| StorageError::DirectoryCreate {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
    })?;

    // 1. Ensure parent directory exists and has strict permissions
    ensure_dir_secure(parent)?;

    // 2. Create a temporary file in the same directory
    let tmp_path = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp_path).map_err(|source| StorageError::FileWrite {
        path: tmp_path.clone(),
        source,
    })?;

    // 3. Set strict permissions (0600) on the temp file before writing data
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|source| StorageError::FileWrite {
                path: tmp_path.clone(),
                source,
            })?;
    }

    // 4. Write data and sync to disk
    file.write_all(data)
        .map_err(|source| StorageError::FileWrite {
            path: tmp_path.clone(),
            source,
        })?;
    file.sync_all().map_err(|source| StorageError::FileWrite {
        path: tmp_path.clone(),
        source,
    })?;

    // 5. Atomic rename
    fs::rename(&tmp_path, path).map_err(|source| StorageError::FileWrite {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(())
}

/// Ensures a directory exists and has strict permissions (0700) on Unix.
pub fn ensure_dir_secure(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path).map_err(|source| StorageError::DirectoryCreate {
            path: path.to_path_buf(),
            source,
        })?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(path).map_err(|source| StorageError::DirectoryRead {
            path: path.to_path_buf(),
            source,
        })?;
        let mut perms = metadata.permissions();
        if perms.mode() & 0o777 != 0o700 {
            perms.set_mode(0o700);
            fs::set_permissions(path, perms).map_err(|source| StorageError::FileWrite {
                path: path.to_path_buf(),
                source,
            })?;
        }
    }

    Ok(())
}

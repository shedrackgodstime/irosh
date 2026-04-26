//! Secure storage for the server's password hashes (shadow file).

use crate::config::StateConfig;
use crate::error::{Result, StorageError};
use crate::storage::utils::atomic_write_secure;
use std::path::PathBuf;

/// Returns the path to the server's shadow file.
pub fn shadow_file_path(state: &StateConfig) -> PathBuf {
    state.root().join("shadow")
}

/// Saves a hashed password to the server's shadow file atomically and securely.
pub fn write_shadow_file(state: &StateConfig, password_hash: &str) -> Result<()> {
    let path = shadow_file_path(state);
    atomic_write_secure(&path, password_hash.as_bytes())
}

/// Loads the hashed password from the server's shadow file.
///
/// Returns `Ok(None)` if the shadow file does not exist.
pub fn load_shadow_file(state: &StateConfig) -> Result<Option<String>> {
    let path = shadow_file_path(state);
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path).map_err(|source| StorageError::FileRead {
        path: path.clone(),
        source,
    })?;

    Ok(Some(content.trim().to_string()))
}

/// Removes the shadow file, effectively disabling password authentication.
pub fn delete_shadow_file(state: &StateConfig) -> Result<bool> {
    let path = shadow_file_path(state);
    if !path.exists() {
        return Ok(false);
    }

    std::fs::remove_file(&path).map_err(|source| StorageError::FileDelete {
        path: path.clone(),
        source,
    })?;

    Ok(true)
}

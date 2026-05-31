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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StateConfig;

    fn temp_state(label: &str) -> StateConfig {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "irosh-shadow-test-{}-{}",
            label,
            rand::random::<u32>()
        ));
        StateConfig::new(path)
    }

    #[test]
    fn load_shadow_file_returns_none_when_missing() {
        let state = temp_state("missing");
        assert!(load_shadow_file(&state).unwrap().is_none());
    }

    #[test]
    fn write_and_load_shadow_file_round_trip() {
        let state = temp_state("roundtrip");
        write_shadow_file(&state, "my-hashed-password").unwrap();
        let loaded = load_shadow_file(&state).unwrap().unwrap();
        assert_eq!(loaded, "my-hashed-password");
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn delete_shadow_file_returns_true_when_exists() {
        let state = temp_state("delete-exists");
        write_shadow_file(&state, "delete-me").unwrap();
        assert!(delete_shadow_file(&state).unwrap());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn delete_shadow_file_returns_false_when_missing() {
        let state = temp_state("delete-missing");
        assert!(!delete_shadow_file(&state).unwrap());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn write_shadow_file_trims_whitespace_on_load() {
        let state = temp_state("trim");
        write_shadow_file(&state, "  spaced-hash  ").unwrap();
        let loaded = load_shadow_file(&state).unwrap().unwrap();
        assert_eq!(loaded, "spaced-hash");
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn shadow_file_path_ends_with_shadow() {
        let state = StateConfig::new("/tmp/test-shadow".into());
        assert!(shadow_file_path(&state).ends_with("shadow"));
    }
}

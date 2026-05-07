//! Persistent application configuration storage.

use crate::config::{AppConfig, StateConfig};
use crate::error::{Result, StorageError};
use crate::storage::utils::atomic_write_secure;
use std::fs;

const CONFIG_FILE: &str = "irosh.json";

/// Loads the persistent application configuration from disk.
///
/// If the configuration file does not exist, returns the default configuration.
pub fn load_config(state: &StateConfig) -> Result<AppConfig> {
    let path = state.root().join(CONFIG_FILE);
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let data = fs::read_to_string(&path).map_err(|source| StorageError::FileRead {
        path: path.clone(),
        source,
    })?;

    serde_json::from_str(&data)
        .map_err(|source| StorageError::PeerProfileParse { source })
        .map_err(Into::into)
}

/// Saves the persistent application configuration to disk atomically.
pub fn save_config(state: &StateConfig, config: &AppConfig) -> Result<()> {
    let path = state.root().join(CONFIG_FILE);
    let data = serde_json::to_vec_pretty(config)
        .map_err(|source| StorageError::PeerProfileSerialize { source })?;

    atomic_write_secure(&path, &data)
}

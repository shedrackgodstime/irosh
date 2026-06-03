//! Persistent application configuration storage.

use crate::config::{AppConfig, StateConfig};
use crate::error::{Result, StorageError};
use crate::storage::utils::atomic_write_secure;
use std::fs;

const CONFIG_FILE: &str = "irosh.json";

/// Loads the persistent application configuration from disk.
///
/// If the configuration file does not exist, returns the default configuration.
///
/// # Errors
///
/// Returns an error if the configuration file exists but cannot be read,
/// or if its contents cannot be parsed as valid JSON.
#[must_use]
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
///
/// # Errors
///
/// Returns an error if the configuration data cannot be serialized to JSON,
/// or if the atomic write to disk fails.
#[must_use]
pub fn save_config(state: &StateConfig, config: &AppConfig) -> Result<()> {
    let path = state.root().join(CONFIG_FILE);
    let data = serde_json::to_vec_pretty(config)
        .map_err(|source| StorageError::PeerProfileSerialize { source })?;

    atomic_write_secure(&path, &data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(label: &str) -> StateConfig {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "irosh-config-test-{}-{}",
            label,
            rand::random::<u32>()
        ));
        StateConfig::new(path)
    }

    #[test]
    fn load_config_returns_default_when_missing() {
        let state = temp_state("missing");
        let config = load_config(&state).unwrap();
        assert_eq!(config, AppConfig::default());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn save_and_load_config_round_trip() {
        let state = temp_state("roundtrip");
        let config = AppConfig {
            stealth_secret: Some("my-secret".into()),
            relay_url: Some("https://relay.example.com".into()),
            log_level: "debug".into(),
            wormhole_timeout: 7200,
            default_user: Some("admin".into()),
        };
        save_config(&state, &config).unwrap();
        let loaded = load_config(&state).unwrap();
        assert_eq!(loaded, config);
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn save_and_load_config_default_values() {
        let state = temp_state("defaults");
        let config = AppConfig::default();
        save_config(&state, &config).unwrap();
        let loaded = load_config(&state).unwrap();
        assert_eq!(loaded, config);
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn load_config_returns_default_on_empty_directory() {
        let state = temp_state("empty");
        // No file written — load should return default
        let config = load_config(&state).unwrap();
        assert_eq!(config, AppConfig::default());
        let _ = std::fs::remove_dir_all(state.root());
    }
}

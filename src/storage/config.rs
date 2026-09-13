//! Persistent application configuration storage.

use crate::config::{AppConfig, StateConfig};
use crate::error::{Result, StorageError};
use crate::storage::utils::atomic_write_secure;
use std::fs;
use std::path::Path;

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

/// Exports the current configuration to `dest` as pretty JSON with
/// strict secure permissions.
///
/// The export may contain the stealth shared secret, so the destination is
/// written atomically with file permissions restricted to the current user.
///
/// # Errors
///
/// Returns an error if the configuration cannot be serialized or written.
#[must_use]
pub fn export_config(state: &StateConfig, dest: &Path) -> Result<()> {
    let config = load_config(state)?;
    let data = serde_json::to_vec_pretty(&config)
        .map_err(|source| StorageError::PeerProfileSerialize { source })?;
    atomic_write_secure(dest, &data)
}

/// Imports configuration from a JSON file produced by [`export_config`]
/// (or an `irosh.json`), persisting it into the state directory.
///
/// Unknown or missing fields fall back to the file's own values as-is;
/// the file is parsed with the current schema's defaults for absent keys.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed as valid JSON,
/// or if the configuration cannot be saved.
#[must_use]
pub fn import_config(state: &StateConfig, src: &Path) -> Result<()> {
    let data = fs::read(src).map_err(|source| StorageError::FileRead {
        path: src.to_path_buf(),
        source,
    })?;
    let imported: AppConfig = serde_json::from_slice(&data)
        .map_err(|source| StorageError::PeerProfileParse { source })?;
    save_config(state, &imported)
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

    #[test]
    fn export_and_import_config_round_trip() {
        let state = temp_state("export-src");
        let imported_state = temp_state("export-dst");
        let dest = std::env::temp_dir().join(format!(
            "irosh-config-export-{}.json",
            rand::random::<u32>()
        ));

        let config = AppConfig {
            stealth_secret: Some("my-secret".into()),
            relay_url: Some("https://relay.example.com".into()),
            log_level: "debug".into(),
            wormhole_timeout: 7200,
            default_user: Some("admin".into()),
        };
        save_config(&state, &config).unwrap();

        export_config(&state, &dest).unwrap();
        assert!(dest.exists());

        import_config(&imported_state, &dest).unwrap();
        let loaded = load_config(&imported_state).unwrap();
        assert_eq!(loaded, config);

        let _ = std::fs::remove_file(&dest);
        let _ = std::fs::remove_dir_all(state.root());
        let _ = std::fs::remove_dir_all(imported_state.root());
    }

    #[test]
    fn import_config_rejects_invalid_json() {
        let state = temp_state("import-invalid");
        let bad =
            std::env::temp_dir().join(format!("irosh-config-bad-{}.json", rand::random::<u32>()));
        std::fs::write(&bad, b"not json{").unwrap();

        assert!(import_config(&state, &bad).is_err());
        // Nothing should have been persisted.
        assert_eq!(load_config(&state).unwrap(), AppConfig::default());

        let _ = std::fs::remove_file(&bad);
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn import_config_accepts_partial_exports() {
        let state = temp_state("import-partial");
        let partial = std::env::temp_dir().join(format!(
            "irosh-config-partial-{}.json",
            rand::random::<u32>()
        ));
        // Only one field present — missing fields fall back to defaults.
        std::fs::write(
            &partial,
            br#"{"log_level":"trace","relay_url":"https://r.example"}"#,
        )
        .unwrap();

        import_config(&state, &partial).unwrap();
        let loaded = load_config(&state).unwrap();
        assert_eq!(loaded.log_level, "trace");
        assert_eq!(loaded.relay_url.as_deref(), Some("https://r.example"));
        // Container-level `#[serde(default)]` keeps the file loadable even if
        // it omits non-Option fields entirely.
        assert_eq!(loaded.stealth_secret, None);

        let _ = std::fs::remove_file(&partial);
        let _ = std::fs::remove_dir_all(state.root());
    }
}

/// Async variants that offload blocking I/O to the blocking pool.
#[cfg(feature = "storage")]
pub mod async_storage {
    use super::{export_config, import_config, load_config, save_config};
    use crate::config::{AppConfig, StateConfig};
    use crate::error::{IroshError, Result};
    use std::io;
    use std::path::Path;

    /// Async variant of [`load_config`] — offloads to blocking pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the blocking task panics or the underlying
    /// synchronous `load_config` fails.
    pub async fn load_config_async(state: &StateConfig) -> Result<AppConfig> {
        let state = state.clone();
        tokio::task::spawn_blocking(move || load_config(&state))
            .await
            .map_err(|e| IroshError::Io(io::Error::other(e)))?
    }

    /// Async variant of [`save_config`] — offloads to blocking pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the blocking task panics or the underlying
    /// synchronous `save_config` fails.
    pub async fn save_config_async(state: &StateConfig, config: &AppConfig) -> Result<()> {
        let state = state.clone();
        let config = config.clone();
        tokio::task::spawn_blocking(move || save_config(&state, &config))
            .await
            .map_err(|e| IroshError::Io(io::Error::other(e)))?
    }

    /// Async variant of [`export_config`] — offloads to blocking pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the blocking task panics or the underlying
    /// synchronous `export_config` fails.
    pub async fn export_config_async(state: &StateConfig, dest: &Path) -> Result<()> {
        let state = state.clone();
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || export_config(&state, &dest))
            .await
            .map_err(|e| IroshError::Io(io::Error::other(e)))?
    }

    /// Async variant of [`import_config`] — offloads to blocking pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the blocking task panics or the underlying
    /// synchronous `import_config` fails.
    pub async fn import_config_async(state: &StateConfig, src: &Path) -> Result<()> {
        let state = state.clone();
        let src = src.to_path_buf();
        tokio::task::spawn_blocking(move || import_config(&state, &src))
            .await
            .map_err(|e| IroshError::Io(io::Error::other(e)))?
    }
}

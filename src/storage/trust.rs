//! Trust on First Use (TOFU) and peer authorization storage.

use std::fs;
use std::path::{Path, PathBuf};

use russh::keys::ssh_key::PublicKey;

use crate::config::StateConfig;
use crate::error::{Result, StorageError};

use serde::{Deserialize, Serialize};

/// The specific occurrence in the trust layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TrustEventKind {
    /// A new server host key was permanently saved.
    ServerKeyLearned,
    /// A client identity was permanently authorized.
    ClientKeyAuthorized,
}

/// Indicates the result of a trust action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustEvent {
    /// The kind of trust event that occurred.
    pub kind: TrustEventKind,
    /// The file path where the trust record was recorded.
    pub path: PathBuf,
}

/// Represents the state of a single trust record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustRecord {
    /// True if the record is present on disk.
    pub exists: bool,
    /// The path monitored for the record.
    pub path: PathBuf,
    /// The raw OpenSSH formatted string of the key, if it exists.
    pub public_key_openssh: Option<String>,
}

/// A comprehensive view of the current trust store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustSummary {
    /// The state of all known server trust records.
    pub known_servers: Vec<TrustRecord>,
    /// The state of all authorized client trust records.
    pub authorized_clients: Vec<TrustRecord>,
}

/// Ensures the required trust subdirectories exist.
fn ensure_trust_dirs(state: &StateConfig) -> Result<()> {
    let trust_dir = state.root().join("trust");
    let servers_dir = trust_dir.join("servers");
    let clients_dir = trust_dir.join("clients");
    for dir in [&trust_dir, &servers_dir, &clients_dir] {
        crate::storage::utils::ensure_dir_secure(dir)?;
    }

    // Legacy migration: if the old single files exist, we don't know the Node ID yet,
    // so we leave them intact. In the future, we could attempt to migrate them.
    Ok(())
}

fn server_key_path(state: &StateConfig, node_id: &str) -> PathBuf {
    let safe_id = node_id.replace(['/', '\\', '.', ':'], "_");
    state
        .root()
        .join("trust")
        .join("servers")
        .join(format!("{safe_id}.pub"))
}

fn client_key_path(state: &StateConfig, node_id: &str) -> PathBuf {
    let safe_id = node_id.replace(['/', '\\', '.', ':'], "_");
    state
        .root()
        .join("trust")
        .join("clients")
        .join(format!("{safe_id}.pub"))
}

fn write_public_key(path: &Path, key: &PublicKey) -> Result<()> {
    let content = key
        .to_openssh()
        .map_err(|source| StorageError::PublicKeyFormat { source })?;
    crate::storage::utils::atomic_write_secure(path, content.as_bytes())
}

/// Loads the pinned server public key for a specific node ID, if one has been trusted in the past.
///
/// This function also checks the legacy single-tenant trust file for backward
/// compatibility.
///
/// # Errors
///
/// Returns an error if a trust file exists but cannot be read or parsed.
#[must_use]
pub fn load_known_server(state: &StateConfig, node_id: &str) -> Result<Option<PublicKey>> {
    let path = server_key_path(state, node_id);
    if path.exists() {
        PublicKey::read_openssh_file(&path)
            .map(Some)
            .map_err(|source| {
                StorageError::PublicKeyRead {
                    path: path.clone(),
                    source,
                }
                .into()
            })
    } else {
        // Fallback for legacy single-tenant file
        let legacy_path = state.root().join("trust/known_server.pub");
        if legacy_path.exists() {
            PublicKey::read_openssh_file(&legacy_path)
                .map(Some)
                .map_err(|source| {
                    StorageError::PublicKeyRead {
                        path: legacy_path.clone(),
                        source,
                    }
                    .into()
                })
        } else {
            Ok(None)
        }
    }
}

/// Loads the authorized client public key for a specific node ID, if one has been trusted in the past.
///
/// This function also checks the legacy single-tenant trust file for backward
/// compatibility.
///
/// # Errors
///
/// Returns an error if a trust file exists but cannot be read or parsed.
#[must_use]
pub fn load_authorized_client(state: &StateConfig, node_id: &str) -> Result<Option<PublicKey>> {
    let path = client_key_path(state, node_id);
    if path.exists() {
        PublicKey::read_openssh_file(&path)
            .map(Some)
            .map_err(|source| {
                StorageError::PublicKeyRead {
                    path: path.clone(),
                    source,
                }
                .into()
            })
    } else {
        // Fallback for legacy single-tenant file
        let legacy_path = state.root().join("trust/authorized_client.pub");
        if legacy_path.exists() {
            PublicKey::read_openssh_file(&legacy_path)
                .map(Some)
                .map_err(|source| {
                    StorageError::PublicKeyRead {
                        path: legacy_path.clone(),
                        source,
                    }
                    .into()
                })
        } else {
            Ok(None)
        }
    }
}

/// Loads all previously authorized client public keys.
///
/// Invalid individual records are skipped when reading the directory.
///
/// # Errors
///
/// Returns an error if the trust directory itself cannot be read.
/// Loads all previously authorized client public keys along with their Node IDs.
#[must_use]
pub fn load_all_authorized_clients(state: &StateConfig) -> Result<Vec<(String, PublicKey)>> {
    let mut keys = Vec::new();

    let clients_dir = state.root().join("trust").join("clients");
    if clients_dir.exists() {
        let entries = fs::read_dir(&clients_dir).map_err(|source| StorageError::DirectoryRead {
            path: clients_dir.clone(),
            source,
        })?;

        for entry in entries {
            let entry = entry.map_err(|source| StorageError::DirectoryEntryRead {
                path: clients_dir.clone(),
                source,
            })?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "pub") {
                if let Some(stem) = path.file_stem() {
                    let node_id = stem.to_string_lossy().to_string();
                    if let Ok(key) = PublicKey::read_openssh_file(&path) {
                        keys.push((node_id, key));
                    }
                }
            }
        }
    }

    // Fallback for legacy single-tenant file
    let legacy_path = state.root().join("trust/authorized_client.pub");
    if legacy_path.exists() {
        if let Ok(key) = PublicKey::read_openssh_file(&legacy_path) {
            keys.push(("legacy".to_string(), key));
        }
    }

    Ok(keys)
}

/// Saves a server public key permanently, implicitly trusting it for future connections.
///
/// # Errors
///
/// Returns an error if the trust directories cannot be created or if the key
/// cannot be written in OpenSSH format.
#[must_use]
pub fn write_known_server(
    state: &StateConfig,
    node_id: &str,
    key: &PublicKey,
) -> Result<TrustEvent> {
    ensure_trust_dirs(state)?;
    let path = server_key_path(state, node_id);
    write_public_key(&path, key)?;
    Ok(TrustEvent {
        kind: TrustEventKind::ServerKeyLearned,
        path,
    })
}

/// Saves a client public key permanently, permitting it to connect.
///
/// # Errors
///
/// Returns an error if the trust directories cannot be created or if the key
/// cannot be written in OpenSSH format.
#[must_use]
pub fn write_authorized_client(
    state: &StateConfig,
    node_id: &str,
    key: &PublicKey,
) -> Result<TrustEvent> {
    ensure_trust_dirs(state)?;
    let path = client_key_path(state, node_id);
    write_public_key(&path, key)?;
    Ok(TrustEvent {
        kind: TrustEventKind::ClientKeyAuthorized,
        path,
    })
}

/// Clears the known server trust record for a specific node ID.
///
/// Returns `Ok(false)` if no such record exists.
///
/// # Errors
///
/// Returns an error if removing an existing record fails.
#[must_use]
pub fn reset_known_server(state: &StateConfig, node_id: &str) -> Result<bool> {
    remove_if_exists(&server_key_path(state, node_id))
}

/// Clears the authorized client trust record for a specific node ID.
///
/// Returns `Ok(false)` if no such record exists.
///
/// # Errors
///
/// Returns an error if removing an existing record fails.
#[must_use]
pub fn reset_authorized_client(state: &StateConfig, node_id: &str) -> Result<bool> {
    remove_if_exists(&client_key_path(state, node_id))
}

fn remove_if_exists(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(StorageError::FileDelete {
            path: path.to_path_buf(),
            source,
        }
        .into()),
    }
}

/// Analyzes and returns the state of all current trust records on disk.
///
/// Both per-node trust files and legacy single-tenant files are included when
/// present.
///
/// # Errors
///
/// Returns an error if trust directories or trust files cannot be inspected.
#[must_use]
pub fn inspect_trust(state: &StateConfig) -> Result<TrustSummary> {
    let servers_dir = state.root().join("trust").join("servers");
    let clients_dir = state.root().join("trust").join("clients");

    let mut known_servers = read_trust_dir(&servers_dir)?;
    let mut authorized_clients = read_trust_dir(&clients_dir)?;

    // Also inspect legacy files if they exist
    let legacy_server = state.root().join("trust/known_server.pub");
    if legacy_server.exists() {
        known_servers.push(inspect_public_key_record(legacy_server)?);
    }

    let legacy_client = state.root().join("trust/authorized_client.pub");
    if legacy_client.exists() {
        authorized_clients.push(inspect_public_key_record(legacy_client)?);
    }

    Ok(TrustSummary {
        known_servers,
        authorized_clients,
    })
}

fn read_trust_dir(dir: &Path) -> Result<Vec<TrustRecord>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    let entries = fs::read_dir(dir).map_err(|source| StorageError::DirectoryRead {
        path: dir.to_path_buf(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| StorageError::DirectoryEntryRead {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "pub") {
            records.push(inspect_public_key_record(path)?);
        }
    }

    Ok(records)
}

fn inspect_public_key_record(path: PathBuf) -> Result<TrustRecord> {
    if !path.exists() {
        return Ok(TrustRecord {
            exists: false,
            path,
            public_key_openssh: None,
        });
    }

    let key =
        PublicKey::read_openssh_file(&path).map_err(|source| StorageError::PublicKeyRead {
            path: path.clone(),
            source,
        })?;

    Ok(TrustRecord {
        exists: true,
        path,
        public_key_openssh: Some(
            key.to_openssh()
                .map_err(|source| StorageError::PublicKeyFormat { source })?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(label: &str) -> StateConfig {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "irosh-trust-test-{}-{}",
            label,
            rand::random::<u32>()
        ));
        StateConfig::new(path)
    }

    fn make_key(seed_byte: u8) -> PublicKey {
        use russh::keys::ssh_key::PrivateKey;
        use russh::keys::ssh_key::private::Ed25519Keypair;
        let mut seed = [0u8; 32];
        seed[0] = seed_byte;
        PrivateKey::from(Ed25519Keypair::from_seed(&seed))
            .public_key()
            .clone()
    }

    #[test]
    fn load_known_server_returns_none_when_missing() {
        let state = temp_state("known-missing");
        let result = load_known_server(&state, "node-1").unwrap();
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn write_and_load_known_server_round_trip() {
        let state = temp_state("known-rtt");
        let key = make_key(1);
        let event = write_known_server(&state, "node-1", &key).unwrap();
        assert_eq!(event.kind, TrustEventKind::ServerKeyLearned);

        let loaded = load_known_server(&state, "node-1").unwrap().unwrap();
        assert_eq!(loaded.to_openssh().unwrap(), key.to_openssh().unwrap());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn write_and_load_authorized_client_round_trip() {
        let state = temp_state("client-rtt");
        let key = make_key(2);
        let event = write_authorized_client(&state, "client-1", &key).unwrap();
        assert_eq!(event.kind, TrustEventKind::ClientKeyAuthorized);

        let loaded = load_authorized_client(&state, "client-1").unwrap().unwrap();
        assert_eq!(loaded.to_openssh().unwrap(), key.to_openssh().unwrap());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn reset_known_server_removes_record() {
        let state = temp_state("known-reset");
        let key = make_key(3);
        write_known_server(&state, "node-rm", &key).unwrap();
        assert!(reset_known_server(&state, "node-rm").unwrap());
        assert!(load_known_server(&state, "node-rm").unwrap().is_none());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn reset_known_server_returns_false_when_missing() {
        let state = temp_state("known-reset-missing");
        assert!(!reset_known_server(&state, "ghost").unwrap());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn reset_authorized_client_returns_false_when_missing() {
        let state = temp_state("client-reset-missing");
        assert!(!reset_authorized_client(&state, "ghost").unwrap());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn load_all_authorized_clients_returns_all() {
        let state = temp_state("all-clients");
        let key_a = make_key(10);
        let key_b = make_key(11);
        write_authorized_client(&state, "user-a", &key_a).unwrap();
        write_authorized_client(&state, "user-b", &key_b).unwrap();

        let clients = load_all_authorized_clients(&state).unwrap();
        assert_eq!(clients.len(), 2);
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn load_all_authorized_clients_returns_empty_when_none() {
        let state = temp_state("no-clients");
        let clients = load_all_authorized_clients(&state).unwrap();
        assert!(clients.is_empty());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn inspect_trust_returns_summary() {
        let state = temp_state("inspect");
        let key = make_key(5);
        write_known_server(&state, "server-1", &key).unwrap();
        write_authorized_client(&state, "client-1", &key).unwrap();

        let summary = inspect_trust(&state).unwrap();
        assert_eq!(summary.known_servers.len(), 1);
        assert_eq!(summary.authorized_clients.len(), 1);
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn inspect_trust_returns_empty_when_no_trust_data() {
        let state = temp_state("inspect-empty");
        let summary = inspect_trust(&state).unwrap();
        assert!(summary.known_servers.is_empty());
        assert!(summary.authorized_clients.is_empty());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn trust_event_kind_variants_distinct() {
        assert_ne!(
            TrustEventKind::ServerKeyLearned,
            TrustEventKind::ClientKeyAuthorized
        );
    }

    #[test]
    fn trust_record_serde_round_trip() {
        let record = TrustRecord {
            exists: true,
            path: "/tmp/key.pub".into(),
            public_key_openssh: Some("ssh-ed25519 AAA...".into()),
        };
        let json = serde_json::to_string(&record).unwrap();
        let deserialized: TrustRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record, deserialized);
    }
}

//! Trust on First Use (TOFU) and peer authorization storage.

use std::fs;
use std::path::{Path, PathBuf};

use russh::keys::ssh_key::PublicKey;

use crate::config::StateConfig;
use crate::error::{Result, StorageError};

use serde::{Deserialize, Serialize};

/// The specific occurrence in the trust layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
        if !dir.exists() {
            fs::create_dir_all(dir).map_err(|source| StorageError::DirectoryCreate {
                path: dir.clone(),
                source,
            })?;
        }
    }

    // Legacy migration: if the old single files exist, we don't know the Node ID yet,
    // so we leave them intact. In the future, we could attempt to migrate them.
    Ok(())
}

fn server_key_path(state: &StateConfig, node_id: &str) -> PathBuf {
    let safe_id = node_id.replace(['/', '\\', '.'], "_");
    state
        .root()
        .join("trust")
        .join("servers")
        .join(format!("{}.pub", safe_id))
}

fn client_key_path(state: &StateConfig, node_id: &str) -> PathBuf {
    let safe_id = node_id.replace(['/', '\\', '.'], "_");
    state
        .root()
        .join("trust")
        .join("clients")
        .join(format!("{}.pub", safe_id))
}

fn write_public_key(path: &Path, key: &PublicKey) -> Result<()> {
    key.write_openssh_file(path).map_err(|source| {
        StorageError::PublicKeyWrite {
            path: path.to_path_buf(),
            source,
        }
        .into()
    })
}

/// Loads the pinned server public key for a specific node ID, if one has been trusted in the past.
///
/// This function also checks the legacy single-tenant trust file for backward
/// compatibility.
///
/// # Errors
///
/// Returns an error if a trust file exists but cannot be read or parsed.
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
pub fn load_all_authorized_clients(state: &StateConfig) -> Result<Vec<PublicKey>> {
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
                if let Ok(key) = PublicKey::read_openssh_file(&path) {
                    keys.push(key);
                }
            }
        }
    }

    // Fallback for legacy single-tenant file
    let legacy_path = state.root().join("trust/authorized_client.pub");
    if legacy_path.exists() {
        if let Ok(key) = PublicKey::read_openssh_file(&legacy_path) {
            keys.push(key);
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

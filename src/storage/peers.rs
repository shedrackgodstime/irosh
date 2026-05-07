//! Named peer routing profiles.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::StateConfig;
use crate::error::{Result, StorageError};

use crate::transport::ticket::Ticket;

/// Represents a saved peer connection target.
///
/// `PeerProfile` is the storage-layer record for a named alias and its
/// serialized ticket string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerProfile {
    /// The human-readable label for the peer (e.g., "my-server").
    pub name: String,
    /// The Iroh endpoint ticket.
    pub ticket: Ticket,
}

/// Ensures the peers storage subdirectory exists.
fn ensure_peers_dir(state: &StateConfig) -> Result<PathBuf> {
    let path = state.root().join("peers");
    if !path.exists() {
        fs::create_dir_all(&path).map_err(|source| StorageError::DirectoryCreate {
            path: path.clone(),
            source,
        })?;
    }
    Ok(path)
}

/// Generates the deterministic path for a given peer name.
fn peer_path(state: &StateConfig, name: &str) -> PathBuf {
    state.root().join("peers").join(format!("{}.json", name))
}

/// Saves a peer profile to disk.
///
/// # Errors
///
/// Returns an error if the peers directory cannot be created, if the peer name
/// is invalid for on-disk storage, or if the profile cannot be serialized or
/// written.
pub fn save_peer(state: &StateConfig, profile: &PeerProfile) -> Result<()> {
    ensure_peers_dir(state)?;

    // Validate the name doesn't contain path traversal vulnerabilities.
    if profile.name.contains('/') || profile.name.contains('\\') || profile.name == ".." {
        return Err(StorageError::PeerNameInvalid {
            name: profile.name.clone(),
        }
        .into());
    }

    let path = peer_path(state, &profile.name);
    let json = serde_json::to_vec_pretty(profile)
        .map_err(|source| StorageError::PeerProfileSerialize { source })?;

    crate::storage::utils::atomic_write_secure(&path, &json)?;

    Ok(())
}

/// Retrieves a peer profile by its human-readable name.
///
/// Returns `Ok(None)` if no saved profile exists under that name.
///
/// # Errors
///
/// Returns an error if the saved profile exists but cannot be read or parsed.
pub fn get_peer(state: &StateConfig, name: &str) -> Result<Option<PeerProfile>> {
    let path = peer_path(state, name);
    if !path.exists() {
        return Ok(None);
    }

    let json = fs::read_to_string(&path).map_err(|source| StorageError::FileRead {
        path: path.clone(),
        source,
    })?;

    let profile =
        serde_json::from_str(&json).map_err(|source| StorageError::PeerProfileParse { source })?;

    Ok(Some(profile))
}

/// Lists all saved peer profiles.
///
/// Profiles that cannot be parsed are skipped rather than aborting the full
/// listing.
///
/// # Errors
///
/// Returns an error if the peers directory cannot be read.
pub fn list_peers(state: &StateConfig) -> Result<Vec<PeerProfile>> {
    let dir = ensure_peers_dir(state)?;
    let mut profiles = Vec::new();

    let entries = fs::read_dir(&dir).map_err(|source| StorageError::DirectoryRead {
        path: dir.clone(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| StorageError::DirectoryEntryRead {
            path: dir.clone(),
            source,
        })?;
        let path = entry.path();

        if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
            let json = fs::read_to_string(&path).map_err(|source| StorageError::FileRead {
                path: path.clone(),
                source,
            })?;

            // If a profile fails to parse, we skip it rather than aborting the list.
            if let Ok(profile) = serde_json::from_str::<PeerProfile>(&json) {
                profiles.push(profile);
            }
        }
    }

    Ok(profiles)
}

/// Deletes a saved peer profile by name.
///
/// Returns `Ok(false)` if the profile did not exist.
///
/// # Errors
///
/// Returns an error if removing an existing profile fails.
pub fn delete_peer(state: &StateConfig, name: &str) -> Result<bool> {
    let path = peer_path(state, name);
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(StorageError::FileDelete { path, source }.into()),
    }
}

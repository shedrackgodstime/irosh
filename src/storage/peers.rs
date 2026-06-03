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

/// Maximum length for a peer profile name.
const MAX_NAME_LEN: usize = 128;

/// Windows reserved filenames that cannot be used as file names.
const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Validates a peer profile name for safe on-disk storage.
///
/// Returns an error if the name is empty, too long, contains special characters,
/// matches a reserved name, or would enable path traversal.
fn validate_peer_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(StorageError::PeerNameInvalid {
            name: name.to_string(),
        }
        .into());
    }
    if name.len() > MAX_NAME_LEN {
        return Err(StorageError::PeerNameInvalid {
            name: name.to_string(),
        }
        .into());
    }
    if name.contains('\0') {
        return Err(StorageError::PeerNameInvalid {
            name: name.to_string(),
        }
        .into());
    }
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(StorageError::PeerNameInvalid {
            name: name.to_string(),
        }
        .into());
    }
    let lower = name.to_lowercase();
    if RESERVED_NAMES.contains(&lower.as_str()) {
        return Err(StorageError::PeerNameInvalid {
            name: name.to_string(),
        }
        .into());
    }
    Ok(())
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
    state.root().join("peers").join(format!("{name}.json"))
}

/// Saves a peer profile to disk.
///
/// # Errors
///
/// Returns an error if the peers directory cannot be created, if the peer name
/// is invalid for on-disk storage, or if the profile cannot be serialized or
/// written.
#[must_use]
pub fn save_peer(state: &StateConfig, profile: &PeerProfile) -> Result<()> {
    ensure_peers_dir(state)?;
    validate_peer_name(&profile.name)?;
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
#[must_use]
pub fn load_peer(state: &StateConfig, name: &str) -> Result<Option<PeerProfile>> {
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
#[must_use]
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
#[must_use]
pub fn delete_peer(state: &StateConfig, name: &str) -> Result<bool> {
    let path = peer_path(state, name);
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(StorageError::FileDelete { path, source }.into()),
    }
}

/// Renames an existing peer profile.
///
/// Loads the profile under `old_name`, saves it under `new_name`, then
/// removes the old file.  Returns `Ok(false)` if `old_name` does not exist.
///
/// # Errors
///
/// Returns an error if `new_name` is invalid, if the read/write fails, or if
/// the old file cannot be deleted.
#[must_use]
pub fn rename_peer(state: &StateConfig, old_name: &str, new_name: &str) -> Result<bool> {
    let Some(profile) = load_peer(state, old_name)? else {
        return Ok(false);
    };

    save_peer(
        state,
        &PeerProfile {
            name: new_name.to_string(),
            ticket: profile.ticket,
        },
    )?;

    delete_peer(state, old_name)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(label: &str) -> StateConfig {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "irosh-peers-test-{}-{}",
            label,
            rand::random::<u32>()
        ));
        StateConfig::new(path)
    }

    fn make_ticket() -> Ticket {
        let pubkey = iroh::SecretKey::generate().public();
        Ticket::new(iroh::EndpointAddr::new(pubkey))
    }

    #[test]
    fn save_and_load_peer_round_trip() {
        let state = temp_state("roundtrip");
        let ticket = make_ticket();
        let profile = PeerProfile {
            name: "my-server".into(),
            ticket: ticket.clone(),
        };
        save_peer(&state, &profile).unwrap();
        let loaded = load_peer(&state, "my-server").unwrap().unwrap();
        assert_eq!(loaded.name, "my-server");
        assert_eq!(loaded.ticket, ticket);
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn load_peer_returns_none_when_missing() {
        let state = temp_state("missing");
        assert!(load_peer(&state, "nonexistent").unwrap().is_none());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn list_peers_returns_saved_profiles() {
        let state = temp_state("list");
        let ticket = make_ticket();
        save_peer(
            &state,
            &PeerProfile {
                name: "alpha".into(),
                ticket: ticket.clone(),
            },
        )
        .unwrap();
        save_peer(
            &state,
            &PeerProfile {
                name: "beta".into(),
                ticket,
            },
        )
        .unwrap();
        let peers = list_peers(&state).unwrap();
        assert_eq!(peers.len(), 2);
        let names: Vec<&str> = peers.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn delete_peer_removes_profile() {
        let state = temp_state("delete");
        let ticket = make_ticket();
        save_peer(
            &state,
            &PeerProfile {
                name: "to-delete".into(),
                ticket,
            },
        )
        .unwrap();
        assert!(delete_peer(&state, "to-delete").unwrap());
        assert!(load_peer(&state, "to-delete").unwrap().is_none());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn delete_peer_returns_false_when_missing() {
        let state = temp_state("delete-missing");
        assert!(!delete_peer(&state, "ghost").unwrap());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn rename_peer_changes_name() {
        let state = temp_state("rename");
        let ticket = make_ticket();
        save_peer(
            &state,
            &PeerProfile {
                name: "old-name".into(),
                ticket: ticket.clone(),
            },
        )
        .unwrap();
        assert!(rename_peer(&state, "old-name", "new-name").unwrap());
        assert!(load_peer(&state, "old-name").unwrap().is_none());
        let loaded = load_peer(&state, "new-name").unwrap().unwrap();
        assert_eq!(loaded.ticket, ticket);
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn rename_peer_returns_false_when_old_missing() {
        let state = temp_state("rename-missing");
        assert!(!rename_peer(&state, "ghost", "new-name").unwrap());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn save_peer_rejects_path_traversal_name() {
        let state = temp_state("traversal");
        let ticket = make_ticket();
        let result = save_peer(
            &state,
            &PeerProfile {
                name: "../evil".into(),
                ticket,
            },
        );
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn save_peer_rejects_name_with_slash() {
        let state = temp_state("slash");
        let ticket = make_ticket();
        let result = save_peer(
            &state,
            &PeerProfile {
                name: "a/b".into(),
                ticket,
            },
        );
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn save_peer_rejects_name_with_backslash() {
        let state = temp_state("backslash");
        let ticket = make_ticket();
        let result = save_peer(
            &state,
            &PeerProfile {
                name: "a\\b".into(),
                ticket,
            },
        );
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn list_peers_skips_corrupt_files() {
        let state = temp_state("corrupt");
        // Manually write an unparseable JSON file
        let dir = state.root().join("peers");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.json"), "not valid json").unwrap();

        // Also write a valid profile
        let ticket = make_ticket();
        save_peer(
            &state,
            &PeerProfile {
                name: "good".into(),
                ticket,
            },
        )
        .unwrap();

        let peers = list_peers(&state).unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].name, "good");
        let _ = std::fs::remove_dir_all(state.root());
    }
}

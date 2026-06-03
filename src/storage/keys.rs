//! Identity key bootstrapping and management.

use std::fmt;
use std::fs;
use std::str::FromStr;

use iroh::SecretKey;
use russh::keys::ssh_key::PrivateKey;
use russh::keys::ssh_key::private::Ed25519Keypair;
use tokio::task;

use crate::config::StateConfig;
use crate::error::{Result, StorageError};

/// Ensures the key storage directory exists.
fn ensure_key_dir(state: &StateConfig) -> Result<()> {
    let path = state.root().join("keys");
    if !path.exists() {
        fs::create_dir_all(&path).map_err(|source| StorageError::DirectoryCreate {
            path: path.clone(),
            source,
        })?;
    }
    Ok(())
}

/// Holds the unified cryptographic identity for both Iroh and SSH layers.
///
/// The same seed material is used to derive both the Iroh endpoint identity and
/// the SSH host/client key used by the library.
pub struct EndpointIdentity {
    /// The Iroh networking secret key.
    pub secret_key: SecretKey,
    /// The SSH protocol private key.
    pub ssh_key: PrivateKey,
}

impl fmt::Debug for EndpointIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EndpointIdentity")
            .field("endpoint_id", &self.endpoint_id())
            .field("secret_key", &"<redacted>")
            .field("ssh_key", &"<redacted>")
            .finish()
    }
}

impl EndpointIdentity {
    /// Returns the public Endpoint ID for this identity.
    #[must_use]
    pub fn endpoint_id(&self) -> String {
        self.secret_key.public().to_string()
    }
}

const SECRET_KEY_FILE: &str = "keys/endpoint.secret";

/// Loads the local identity from storage, or generates a new one if none exists.
///
/// This ensures that the Iroh endpoint ID and the SSH host key are derived from the
/// same secret seed for self-authenticating connections.
///
/// # Errors
///
/// Returns an error if the key directory cannot be created, if the persisted
/// secret cannot be read or parsed, or if a generated secret cannot be written.
#[must_use]
pub async fn load_or_generate_identity(state: &StateConfig) -> Result<EndpointIdentity> {
    let state = state.clone();
    task::spawn_blocking(move || load_or_generate_identity_blocking(&state))
        .await
        .map_err(|source| StorageError::BlockingTaskFailed {
            operation: "loading or generating identity",
            source,
        })?
}

/// Loads the local Iroh secret key from storage.
///
/// Unlike `load_or_generate_identity`, this will NOT generate a new key if it's missing.
///
/// # Errors
///
/// Returns an error if the secret file does not exist or is invalid.
#[must_use]
pub fn load_secret_key(state: &StateConfig) -> Result<SecretKey> {
    let path = state.root().join(SECRET_KEY_FILE);
    if !path.exists() {
        return Err(StorageError::FileRead {
            path,
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "node secret not found"),
        }
        .into());
    }

    let raw = fs::read_to_string(&path).map_err(|source| StorageError::FileRead {
        path: path.clone(),
        source,
    })?;
    let key = SecretKey::from_str(raw.trim()).map_err(|e| StorageError::EndpointSecretInvalid {
        path: path.clone(),
        details: e.to_string(),
        source: Box::new(e),
    })?;
    Ok(key)
}

fn load_or_generate_identity_blocking(state: &StateConfig) -> Result<EndpointIdentity> {
    ensure_key_dir(state)?;

    let path = state.root().join(SECRET_KEY_FILE);

    let secret_key = if path.exists() {
        let raw = fs::read_to_string(&path).map_err(|source| StorageError::FileRead {
            path: path.clone(),
            source,
        })?;
        SecretKey::from_str(raw.trim()).map_err(|e| StorageError::EndpointSecretInvalid {
            path: path.clone(),
            details: e.to_string(),
            source: Box::new(e),
        })?
    } else {
        let secret_key = SecretKey::generate();
        let hex = secret_key
            .to_bytes()
            .iter()
            .fold(String::with_capacity(64), |mut acc, b| {
                use std::fmt::Write;
                let _ = write!(acc, "{b:02x}");
                acc
            });
        crate::storage::utils::atomic_write_secure(&path, hex.as_bytes())?;
        secret_key
    };

    // Derive SSH key from Iroh secret bytes.
    let seed = secret_key.to_bytes();
    let keypair = Ed25519Keypair::from_seed(&seed);
    let ssh_key = PrivateKey::from(keypair);

    Ok(EndpointIdentity {
        secret_key,
        ssh_key,
    })
}
/// Deletes the local secret key from storage.
///
/// This is used to "rotate" the endpoint identity. Returns `true` if a file was deleted,
/// `false` if it didn't exist.
///
/// # Errors
///
/// Returns an error if the secret key file exists but cannot be removed
/// due to a file system error (permissions, read-only, etc.).
#[must_use]
pub fn delete_secret_key(state: &StateConfig) -> Result<bool> {
    let path = state.root().join(SECRET_KEY_FILE);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|source| StorageError::FileWrite {
            path: path.clone(),
            source,
        })?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Saves the local Iroh secret key to storage.
///
/// # Errors
///
/// Returns an error if the key directory cannot be created or if the
/// secret key file cannot be written atomically.
#[must_use]
pub fn save_secret_key(state: &StateConfig, key: &SecretKey) -> Result<()> {
    ensure_key_dir(state)?;
    let path = state.root().join(SECRET_KEY_FILE);
    let hex = key
        .to_bytes()
        .iter()
        .fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        });
    crate::storage::utils::atomic_write_secure(&path, hex.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(label: &str) -> StateConfig {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "irosh-keys-test-{}-{}",
            label,
            rand::random::<u32>()
        ));
        StateConfig::new(path)
    }

    #[test]
    fn save_and_load_secret_key_round_trip() {
        let state = temp_state("roundtrip");
        let key = SecretKey::generate();
        save_secret_key(&state, &key).unwrap();
        let loaded = load_secret_key(&state).unwrap();
        assert_eq!(key.to_bytes(), loaded.to_bytes());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn load_secret_key_errors_when_missing() {
        let state = temp_state("missing");
        let result = load_secret_key(&state);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn delete_secret_key_returns_true_when_exists() {
        let state = temp_state("delete-exists");
        let key = SecretKey::generate();
        save_secret_key(&state, &key).unwrap();
        assert!(delete_secret_key(&state).unwrap());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn delete_secret_key_returns_false_when_missing() {
        let state = temp_state("delete-missing");
        assert!(!delete_secret_key(&state).unwrap());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[tokio::test]
    async fn load_or_generate_identity_generates_new_key() {
        let state = temp_state("generate");
        let identity = load_or_generate_identity(&state).await.unwrap();
        assert!(!identity.endpoint_id().is_empty());
        // Second call should load the same identity
        let identity2 = load_or_generate_identity(&state).await.unwrap();
        assert_eq!(identity.endpoint_id(), identity2.endpoint_id());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[tokio::test]
    async fn load_or_generate_identity_uses_existing_key() {
        let state = temp_state("reuse");
        let key = SecretKey::generate();
        save_secret_key(&state, &key).unwrap();

        let identity = load_or_generate_identity(&state).await.unwrap();
        assert_eq!(identity.secret_key.to_bytes(), key.to_bytes());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn endpoint_identity_redacts_keys_in_debug() {
        let state = temp_state("debug-redact");
        let key = SecretKey::generate();
        let seed = key.to_bytes();
        let keypair = Ed25519Keypair::from_seed(&seed);
        let ssh_key = PrivateKey::from(keypair);
        let identity = EndpointIdentity {
            secret_key: key,
            ssh_key,
        };
        let debug = format!("{:?}", identity);
        assert!(debug.contains("<redacted>"));
        assert!(debug.contains("secret_key: \"<redacted>\""));
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn endpoint_identity_endpoint_id_matches_public_key() {
        let state = temp_state("endpoint-id");
        let key = SecretKey::generate();
        let expected_id = key.public().to_string();
        let seed = key.to_bytes();
        let keypair = Ed25519Keypair::from_seed(&seed);
        let ssh_key = PrivateKey::from(keypair);
        let identity = EndpointIdentity {
            secret_key: key,
            ssh_key,
        };
        assert_eq!(identity.endpoint_id(), expected_id);
        let _ = std::fs::remove_dir_all(state.root());
    }
}

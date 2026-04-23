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
/// The same seed material is used to derive both the Iroh node identity and
/// the SSH host/client key used by the library.
pub struct NodeIdentity {
    /// The Iroh networking secret key.
    pub secret_key: SecretKey,
    /// The SSH protocol private key.
    pub ssh_key: PrivateKey,
}

impl fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeIdentity")
            .field("node_id", &self.secret_key.public().to_string())
            .field("secret_key", &"<redacted>")
            .field("ssh_key", &"<redacted>")
            .finish()
    }
}

const SECRET_KEY_FILE: &str = "keys/node.secret";

/// Loads the local identity from storage, or generates a new one if none exists.
///
/// This ensures that the Iroh node ID and the SSH host key are derived from the
/// same secret seed for self-authenticating connections.
///
/// # Errors
///
/// Returns an error if the key directory cannot be created, if the persisted
/// secret cannot be read or parsed, or if a generated secret cannot be written.
pub async fn load_or_generate_identity(state: &StateConfig) -> Result<NodeIdentity> {
    let state = state.clone();
    task::spawn_blocking(move || load_or_generate_identity_blocking(&state))
        .await
        .map_err(|source| StorageError::BlockingTaskFailed {
            operation: "loading or generating identity",
            source,
        })?
}

fn load_or_generate_identity_blocking(state: &StateConfig) -> Result<NodeIdentity> {
    ensure_key_dir(state)?;

    let path = state.root().join(SECRET_KEY_FILE);

    let secret_key = if path.exists() {
        let raw = fs::read_to_string(&path).map_err(|source| StorageError::FileRead {
            path: path.clone(),
            source,
        })?;
        SecretKey::from_str(raw.trim()).map_err(|e| StorageError::NodeSecretInvalid {
            path: path.clone(),
            details: e.to_string(),
            source: Box::new(e),
        })?
    } else {
        let secret_key = SecretKey::generate(&mut rand::rng());
        let hex = secret_key
            .to_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        fs::write(&path, hex).map_err(|source| StorageError::FileWrite {
            path: path.clone(),
            source,
        })?;
        secret_key
    };

    // Derive SSH key from Iroh secret bytes.
    let seed = secret_key.to_bytes();
    let keypair = Ed25519Keypair::from_seed(&seed);
    let ssh_key = PrivateKey::from(keypair);

    Ok(NodeIdentity {
        secret_key,
        ssh_key,
    })
}

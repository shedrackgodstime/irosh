//! Persistent storage and security trust management.
//!
//! This module handles the long-term state of the irosh system, including:
//! - **Local Identity**: Bootstrapping and loading the node's Ed25519 secret key.
//! - **Peer Trust**: Managing known host keys (TOFU) and authorized client keys.
//! - **Peer Profiles**: Saving and retrieving friendly aliases for P2P tickets.
//!
//! Persistence is typically rooted in the `~/.irosh` directory (on Unix) or
//! equivalent OS-standard locations.

pub mod config;
pub mod keys;
pub mod peers;
pub mod shadow;
pub mod trust;
pub(crate) mod utils;

pub use config::{load_config, save_config};
pub use keys::{
    EndpointIdentity, delete_secret_key, load_or_generate_identity, load_secret_key,
    save_secret_key,
};
pub use peers::{PeerProfile, delete_peer, list_peers, load_peer, rename_peer, save_peer};
pub use shadow::{delete_shadow_file, load_shadow_file, write_shadow_file};
pub use trust::{
    load_all_authorized_clients, load_all_authorized_clients as list_authorized_keys,
    load_authorized_client, load_known_server, reset_authorized_client as revoke_key,
    write_authorized_client, write_known_server,
};

/// Fully resets the node's trust and configuration state.
pub fn reset_vault(state: &crate::config::StateConfig) -> crate::error::Result<()> {
    let trust_dir = state.root().join("trust");
    if trust_dir.exists() {
        let _ = std::fs::remove_dir_all(&trust_dir);
    }
    let _ = shadow::delete_shadow_file(state);
    Ok(())
}

/// Rotates the node's identity by deleting the existing secret key.
pub async fn rotate_identity(
    state: &crate::config::StateConfig,
) -> crate::error::Result<EndpointIdentity> {
    keys::delete_secret_key(state)?;
    keys::load_or_generate_identity(state).await
}

#[cfg(test)]
mod tests {
    use crate::config::StateConfig;

    fn temp_state(label: &str) -> StateConfig {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "irosh-storage-mod-test-{}-{}",
            label,
            rand::random::<u32>()
        ));
        StateConfig::new(path)
    }

    #[test]
    fn reset_vault_cleans_trust_and_shadow() {
        let state = temp_state("vault-reset");
        // Create some trust data
        let trust_dir = state.root().join("trust");
        std::fs::create_dir_all(&trust_dir).unwrap();
        std::fs::write(trust_dir.join("known_server.pub"), b"ssh-ed25519 AAA").unwrap();
        // Create shadow file
        super::shadow::write_shadow_file(&state, "hash").unwrap();

        super::reset_vault(&state).unwrap();
        assert!(!trust_dir.exists());
        assert!(super::shadow::load_shadow_file(&state).unwrap().is_none());
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[test]
    fn reset_vault_succeeds_on_clean_state() {
        let state = temp_state("vault-clean");
        // No trust data, no shadow — should not error
        super::reset_vault(&state).unwrap();
        let _ = std::fs::remove_dir_all(state.root());
    }

    #[tokio::test]
    async fn rotate_identity_generates_new_key() {
        let state = temp_state("rotate");
        // First create an identity
        let first = super::rotate_identity(&state).await.unwrap();
        let first_id = first.endpoint_id();
        // Rotate to a new one
        let second = super::rotate_identity(&state).await.unwrap();
        let second_id = second.endpoint_id();
        // The new identity should differ from the old one
        assert_ne!(first_id, second_id);
        let _ = std::fs::remove_dir_all(state.root());
    }
}

//! Persistent storage and security trust management.
//!
//! This module handles the long-term state of the irosh system, including:
//! - **Local Identity**: Bootstrapping and loading the node's Ed25519 secret key.
//! - **Peer Trust**: Managing known host keys (TOFU) and authorized client keys.
//! - **Peer Profiles**: Saving and retrieving friendly aliases for P2P tickets.
//!
//! Persistence is typically rooted in the `~/.irosh` directory (on Unix) or
//! equivalent OS-standard locations.

pub mod keys;
pub mod peers;
pub mod shadow;
pub mod trust;
pub(crate) mod utils;

pub use keys::{NodeIdentity, delete_secret_key, load_or_generate_identity, load_secret_key};
pub use peers::{PeerProfile, delete_peer, get_peer, list_peers, save_peer};
pub use shadow::{delete_shadow_file, load_shadow_file, write_shadow_file};
pub use trust::{
    load_all_authorized_clients, load_authorized_client, load_known_server,
    write_authorized_client, write_known_server,
};

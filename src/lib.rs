//! # Irosh: Peer-to-Peer Secure Shell Library
//!
//! `irosh` is a high-level networking library that combines the [Iroh](https://iroh.computer)
//! networking stack with the [SSH](https://en.wikipedia.org/wiki/Secure_Shell) protocol
//! to provide secure, ad-hoc, and persistent P2P shells.
//!
//! ## Key Features
//!
//! - **Self-Authenticating Nodes**: Uses Ed25519 keys for both network identity and SSH authentication.
//! - **NAT Traversal**: Automatic hole-punching and relaying via the Iroh stack.
//! - **Wormhole Pairing**: Secure out-of-band trust establishment using short human-friendly codes.
//! - **Unified Auth**: A flexible authentication system supporting Public Keys, Passwords, and TOFU.
//!
//! ## Crate Architecture
//!
//! This crate follows a **"Fat Library"** design. All logic related to networking,
//! cryptography, and protocol state resides here. The accompanying CLI (`irosh-cli`)
//! is a thin wrapper around this library, handling only UI and OS-specific setup.
//!
//! ### Core Components
//!
//! - [`server`]: The P2P SSH server implementation.
//! - [`client`]: The P2P SSH client implementation.
//! - [`auth`]: Pluggable authentication backends and security policies.
//! - [`transport`]: Low-level P2P ticket management and data transfer protocols.
//! - [`storage`]: Persistence layer for identities, trust records, and peer profiles.
//!
//! ## Security Notice
//!
//! Irosh is built on top of `iroh` and `russh`. While the underlying protocols are
//! industry-standard, this library is in early development. Users should perform
//! their own security audits before using it for mission-critical infrastructure.
pub mod auth;
pub mod client;
pub mod config;
pub mod diagnostic;
pub mod error;
pub mod server;
pub mod session;
pub mod storage;
pub mod sys;
pub mod transport;

pub use config::{SecurityConfig, StateConfig};
pub use error::{IroshError, Result};

pub use auth::{
    AuthMethod, Authenticator, CombinedAuth, ConfirmationCallback, Credentials, KeyOnlyAuth,
    PasswordAuth, PasswordPrompter, UnifiedAuthenticator,
};

#[cfg(feature = "server")]
pub use server::{Server, ServerOptions, ServerReady, ServerShutdown};

/// Re-export russh for downstream consumers (CLI).
#[cfg(any(feature = "server", feature = "client"))]
pub use russh;

/// Re-export iroh for downstream consumers (CLI).
pub use iroh;

#[cfg(feature = "client")]
pub use client::{
    Client, ClientOptions, ResolvedTarget, Session, SessionEvent, TransferProgress, ipc::IpcClient,
};

#[cfg(feature = "server")]
pub use server::ipc::{InternalCommand, IpcCommand, IpcResponse};

pub use session::{PtyOptions, PtySize, SessionState};

pub use transport::{
    metadata::PeerMetadata,
    ticket::Ticket,
    transfer::{
        GetRequest, PutRequest, TransferComplete, TransferFailure, TransferFailureCode,
        TransferReady,
    },
};

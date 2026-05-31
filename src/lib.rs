#![doc = include_str!("README.md")]
//!
//! # Examples
//!
//! ## Hosting a P2P Server
//!
//! ```no_run
//! use irosh::{Server, ServerOptions, StateConfig};
//!
//! #[tokio::main]
//! async fn main() -> irosh::Result<()> {
//!     let options = ServerOptions::new(StateConfig::new("./state".into()));
//!     let (ready, server) = Server::bind(options).await?;
//!
//!     println!("Server Ticket: {}", ready.ticket());
//!     server.run().await
//! }
//! ```
//!
//! ## Connecting as a Client
//!
//! ```no_run
//! use irosh::{Client, ClientOptions, StateConfig, Ticket};
//! use std::str::FromStr;
//!
//! #[tokio::main]
//! async fn main() -> irosh::Result<()> {
//!     let state = StateConfig::new("./state".into());
//!     let options = ClientOptions::new(state);
//!
//!     let ticket = Ticket::from_str("endpoint...")?;
//!     let mut session = Client::connect(&options, ticket).await?;
//!
//!     session.start_shell().await?;
//!     Ok(())
//! }
//! ```

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
    AuthMethod, AuthMode, Authenticator, CombinedAuth, ConfirmationCallback, Credentials,
    KeyOnlyAuth, PasswordAuth, PasswordPrompter, UnifiedAuthenticator,
};

#[cfg(feature = "server")]
pub use server::{Server, ServerOptions, ServerReady, ServerShutdown};

/// Re-export russh for downstream consumers (CLI).
#[cfg(any(feature = "server", feature = "client"))]
pub use russh;

/// Re-export iroh for downstream consumers (CLI).
#[cfg(feature = "transport")]
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

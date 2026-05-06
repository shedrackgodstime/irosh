#![doc = include_str!("../README.md")]

pub mod auth;
pub mod config;
pub mod error;

pub use russh; // Protocol re-export for library consumers.

#[cfg(feature = "transport")]
pub mod transport;

#[cfg(all(feature = "transport", feature = "storage"))]
pub mod diagnostic;

#[cfg(feature = "storage")]
pub mod storage;

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "client")]
pub mod client;

#[cfg(any(feature = "server", feature = "client"))]
pub mod session;

pub use config::{SecurityConfig, StateConfig};
pub use error::{IroshError, Result};

pub use auth::{
    AuthMethod, Authenticator, CombinedAuth, ConfirmationCallback, Credentials, KeyOnlyAuth,
    PairingAuthenticator, PasswordAuth, PasswordPrompter,
};

#[cfg(feature = "server")]
pub use server::{Server, ServerOptions, ServerReady, ServerShutdown};

#[cfg(feature = "client")]
pub use client::{
    Client, ClientOptions, ResolvedTarget, Session, SessionEvent, TransferProgress, ipc::IpcClient,
};

#[cfg(feature = "server")]
pub use server::ipc::{InternalCommand, IpcCommand, IpcResponse};

#[cfg(any(feature = "server", feature = "client"))]
pub use session::{PtyOptions, PtySize, SessionState};

#[cfg(feature = "transport")]
pub use transport::{
    metadata::PeerMetadata,
    ticket::Ticket,
    transfer::{
        GetRequest, PutRequest, TransferComplete, TransferFailure, TransferFailureCode,
        TransferReady,
    },
};

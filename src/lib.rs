// irosh (Fat Library)
// All core networking, cryptography, and state synchronization go here.
// DO NOT put UI prompts or println! in this crate.
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

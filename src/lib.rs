#![doc = include_str!("../README.md")]

pub mod config;
pub mod error;

pub use russh; // Protocol re-export for library consumers.

#[cfg(feature = "transport")]
pub mod transport;

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

#[cfg(feature = "server")]
pub use server::{Server, ServerOptions, ServerReady, ServerShutdown};

#[cfg(feature = "client")]
pub use client::{Client, ClientOptions, Session, SessionEvent, TransferProgress};

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

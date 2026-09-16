//! P2P SSH Client implementation.
//!
//! This module provides the [`Client`] and [`Session`] abstractions
//! for connecting to remote Irosh hosts. It handles peer discovery,
//! ALPN negotiation, and interactive shell multiplexing.
//!
//! ## Connection Flow
//!
//! 1. **Discovery**: Resolve a [`crate::transport::ticket::Ticket`] or [`ResolvedTarget`] into a set
//!    of network addresses.
//! 2. **Authentication**: Negotiate SSH keys or passwords via the configured
//!    [`crate::auth::Authenticator`].
//! 3. **Session**: Establish a bi-directional [`Session`] for shell access
//!    or data transfer.

mod connect;
pub mod handler;
pub mod ipc;
mod session;
mod session_event;
#[cfg(test)]
mod tests;
mod transfer;

pub use self::connect::{Client, ClientOptions};
pub use crate::session::SessionState;
pub use crate::session::pty::PtyOptions;
pub use session::{ExecOutput, Session};
pub use session_event::SessionEvent;

/// Progress state for an ongoing file transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct TransferProgress {
    /// Bytes successfully transferred so far.
    pub transferred: u64,
    /// Total expected size in bytes.
    pub total: u64,
}

/// A target for an irosh connection, which can be a direct ticket or a wormhole code.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolvedTarget {
    /// A direct Iroh connection ticket.
    Ticket(crate::transport::ticket::Ticket),
    /// A 3-word wormhole code that needs to be resolved via Gossip.
    WormholeCode(crate::config::WormholeCode),
}

impl From<crate::transport::ticket::Ticket> for ResolvedTarget {
    fn from(ticket: crate::transport::ticket::Ticket) -> Self {
        Self::Ticket(ticket)
    }
}

impl TransferProgress {
    pub(crate) fn new(transferred: u64, total: u64) -> Self {
        Self { transferred, total }
    }

    /// Returns the completion percentage clamped to `0..=100`.
    #[must_use]
    pub fn percent(&self) -> u8 {
        if self.total == 0 {
            return 0;
        }
        self.transferred
            .saturating_mul(100)
            .checked_div(self.total)
            .unwrap_or(100)
            .min(100) as u8
    }
}

#[cfg(test)]
mod send_sync_tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn client_handler_is_send_sync() {
        assert_send_sync::<handler::ClientHandler>();
    }
}

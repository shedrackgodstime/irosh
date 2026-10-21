//! Client connection handling.
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use russh::client;

use secrecy::ExposeSecret;

use crate::auth::Credentials;
use crate::client::ResolvedTarget;
use crate::client::{Session, handler::ClientHandler};
use crate::config::{EndpointId, HostKeyPolicy, SecurityConfig, StateConfig, WormholeCode};
use crate::error::{ClientError, IroshError, Result};
use crate::session::SessionState;
use crate::storage::keys::load_or_generate_identity;
use crate::storage::trust::load_known_server;
use crate::transport::iroh::{bind_client_endpoint, derive_alpn};
use crate::transport::metadata::{read_metadata, write_metadata_request};
use crate::transport::stream::IrohDuplex;
use crate::transport::ticket::Ticket;
use tracing::{info, warn};

/// Configuration options for the irosh client.
#[derive(Clone, Debug)]
#[must_use = "builders do nothing unless consumed"]
pub struct ClientOptions {
    state: StateConfig,
    security: SecurityConfig,
    secret: Option<String>,
    pub(crate) relay_mode: iroh::RelayMode,
    credentials: Option<Credentials>,
    prompter: Option<Arc<dyn crate::auth::PasswordPrompter>>,
}

impl ClientOptions {
    /// Creates a new client options set with a specific state directory.
    pub fn new(state: StateConfig) -> Self {
        Self {
            state,
            security: SecurityConfig::default(),
            secret: None,
            relay_mode: iroh::RelayMode::Default,
            credentials: None,
            prompter: None,
        }
    }

    /// Configures the relay mode for the client.
    pub fn relay_mode(mut self, mode: iroh::RelayMode) -> Self {
        self.relay_mode = mode;
        self
    }

    /// Configures the security policy for host key trust.
    pub fn security(mut self, security: SecurityConfig) -> Self {
        self.security = security;
        self
    }

    /// Configures an optional shared secret for stealth connections.
    ///
    /// An empty or whitespace-only secret is treated as "no secret" rather
    /// than as a stealth secret. A degenerate secret would still derive a
    /// secret-specific ALPN, but the secrecy it buys is zero: any observer
    /// that sees the derived ALPN in the QUIC ClientHello can reproduce it
    /// trivially, so the peer would *look* hidden while being fully
    /// discoverable.
    pub fn secret(mut self, secret: impl Into<String>) -> Self {
        let secret = secret.into();
        self.secret = if secret.trim().is_empty() {
            None
        } else {
            Some(secret)
        };
        self
    }

    /// Sets username + password credentials for password authentication.
    ///
    /// When provided, the client will attempt password authentication
    /// if public key authentication is rejected by the server.
    pub fn credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Sets an interactive password prompter callback.
    ///
    /// If the server requires a password and explicit credentials were not
    /// provided (or they failed), the client will invoke this callback
    /// to prompt the user.
    pub fn password_prompter(mut self, prompter: impl crate::auth::PasswordPrompter) -> Self {
        self.prompter = Some(Arc::new(prompter));
        self
    }

    /// Returns a reference to the [`StateConfig`] this client was configured with.
    #[must_use]
    pub fn state(&self) -> &StateConfig {
        &self.state
    }

    pub(crate) fn security_config(&self) -> SecurityConfig {
        self.security
    }

    pub(crate) fn secret_value(&self) -> Option<&str> {
        self.secret.as_deref()
    }
}

/// A handle to establish irosh client connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Client;

impl Client {

/// Timeout for discovery endpoint teardown during wormhole rendezvous.
/// 
/// Gives the peer 5 minutes to respond before the local discovery endpoint
/// is torn down.

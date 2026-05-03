//! SSH Server orchestration and connection handlers.

pub mod handler;
pub(crate) mod shell_access;
pub(crate) mod side_streams;
pub(crate) mod startup;
pub(crate) mod transfer;

use russh::keys::ssh_key::PublicKey;
use russh::server;
use std::fmt;
use std::sync::Arc;
use tracing::{info, warn};

use crate::config::{SecurityConfig, StateConfig};
use crate::error::Result;
use crate::server::handler::ServerHandler;
use crate::server::side_streams::spawn_metadata_and_transfer_acceptor;
use crate::server::startup::{bind_server, inspect_server};
pub(crate) use crate::server::transfer::ConnectionShellState;
use crate::transport::stream::IrohDuplex;

/// Builder-style configuration for [`Server::bind`] and [`Server::inspect`].
///
/// `ServerOptions` carries the state directory, host-key policy, optional
/// shared secret, and any pre-authorized client keys that should be trusted
/// immediately at startup.
///
/// # Example
///
/// ```no_run
/// # use std::error::Error;
/// use irosh::{SecurityConfig, ServerOptions, StateConfig};
///
/// # fn main() -> Result<(), Box<dyn Error>> {
/// let options = ServerOptions::new(StateConfig::new("/tmp/irosh-server".into()))
///     .security(SecurityConfig::default())
///     .secret("shared-secret");
/// # let _ = options;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerOptions {
    state: StateConfig,
    security: SecurityConfig,
    secret: Option<String>,
    authorized_keys: Vec<russh::keys::ssh_key::PublicKey>,
}

impl ServerOptions {
    /// Creates server options anchored at a concrete state directory.
    pub fn new(state: StateConfig) -> Self {
        Self {
            state,
            security: SecurityConfig::default(),
            secret: None,
            authorized_keys: Vec::new(),
        }
    }

    /// Replaces the server security policy.
    pub fn security(mut self, security: SecurityConfig) -> Self {
        self.security = security;
        self
    }

    /// Sets the optional shared secret used to derive the transport ALPN.
    pub fn secret(mut self, secret: impl Into<String>) -> Self {
        self.secret = Some(secret.into());
        self
    }

    /// Adds a pre-authorized client public key.
    pub fn authorized_key(mut self, key: russh::keys::ssh_key::PublicKey) -> Self {
        self.authorized_keys.push(key);
        self
    }

    /// Replaces the pre-authorized client list.
    pub fn authorized_keys(
        mut self,
        keys: impl IntoIterator<Item = russh::keys::ssh_key::PublicKey>,
    ) -> Self {
        self.authorized_keys = keys.into_iter().collect();
        self
    }

    pub(crate) fn state(&self) -> &StateConfig {
        &self.state
    }

    pub(crate) fn security_config(&self) -> SecurityConfig {
        self.security
    }

    pub(crate) fn secret_value(&self) -> Option<&str> {
        self.secret.as_deref()
    }

    pub(crate) fn authorized_key_list(&self) -> &[russh::keys::ssh_key::PublicKey] {
        &self.authorized_keys
    }
}

use serde::{Deserialize, Serialize};

/// The connection details required for clients to reach this server.
///
/// `ServerReady` is returned by [`Server::bind`] and [`Server::inspect`]. It is
/// intended for callers that need to display or persist the ticket, node ID,
/// and host key information associated with a server instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerReady {
    endpoint_id: String,
    ticket: crate::transport::ticket::Ticket,
    relay_urls: Vec<String>,
    direct_addresses: Vec<String>,
    host_key_openssh: String,
}

impl ServerReady {
    /// Returns the public endpoint node identifier.
    pub fn endpoint_id(&self) -> &str {
        &self.endpoint_id
    }

    /// Returns the connection ticket clients should dial.
    pub fn ticket(&self) -> &crate::transport::ticket::Ticket {
        &self.ticket
    }

    /// Returns the relay URLs the server is currently connected through.
    pub fn relay_urls(&self) -> &[String] {
        &self.relay_urls
    }

    /// Returns directly reachable network addresses when available.
    pub fn direct_addresses(&self) -> &[String] {
        &self.direct_addresses
    }

    /// Returns the OpenSSH-formatted host key.
    pub fn host_key_openssh(&self) -> &str {
        &self.host_key_openssh
    }
}

/// The running SSH server primitive.
///
/// A `Server` value represents a bound server that is ready to accept incoming
/// connections once [`Server::run`] is called.
pub struct Server {
    endpoint: iroh::Endpoint,
    config: Arc<server::Config>,
    authorized_clients: Vec<PublicKey>,
    security: SecurityConfig,
    state: StateConfig,
    secret: Option<String>,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server")
            .field("authorized_clients", &self.authorized_clients.len())
            .field("security", &self.security)
            .field("state", &self.state)
            .field("has_secret", &self.secret.is_some())
            .finish()
    }
}

/// An explicit shutdown handle for a running server.
#[derive(Clone, Debug)]
pub struct ServerShutdown {
    endpoint: iroh::Endpoint,
}

impl ServerShutdown {
    /// Closes the underlying Iroh endpoint and stops accepting new connections.
    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}

impl Server {
    /// Inspects the server state and returns identity information without starting the network.
    ///
    /// This is useful for diagnostics, background-service integrations, or
    /// retrieving server identity material without entering the accept loop.
    ///
    /// # Errors
    ///
    /// Returns an error if identity state cannot be loaded or if the ready-state
    /// information cannot be assembled from local state.
    pub async fn inspect(options: ServerOptions) -> Result<ServerReady> {
        inspect_server(&options).await
    }

    /// Initializes the server, allocates identity keys, and binds the Iroh network endpoint.
    ///
    /// This method prepares the server but does not start accepting
    /// connections. Call [`Server::run`] on the returned `Server` to enter the
    /// accept loop.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use std::error::Error;
    /// use irosh::{Server, ServerOptions, StateConfig};
    ///
    /// # async fn run() -> Result<(), Box<dyn Error>> {
    /// let (ready, server) =
    ///     Server::bind(ServerOptions::new(StateConfig::new("/tmp/irosh-server".into()))).await?;
    ///
    /// let _ticket = ready.ticket().to_string();
    /// let _server = server;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if local identity material cannot be loaded or created,
    /// or if the Iroh endpoint cannot be bound.
    pub async fn bind(options: ServerOptions) -> Result<(ServerReady, Self)> {
        bind_server(options).await
    }

    /// Returns an explicit shutdown handle for the running server.
    pub fn shutdown_handle(&self) -> ServerShutdown {
        ServerShutdown {
            endpoint: self.endpoint.clone(),
        }
    }

    /// Engages the server listen loop to accept connections until the endpoint closes.
    ///
    /// Use [`Server::shutdown_handle`] from another task if you need explicit
    /// remote shutdown control.
    ///
    /// # Errors
    ///
    /// Returns an error only if the outer server loop fails before entering its
    /// normal shutdown path. Individual session failures are logged and do not
    /// stop the accept loop.
    pub async fn run(self) -> Result<()> {
        info!("Server actively listening for connections.");

        while let Some(incoming) = self.endpoint.accept().await {
            let mut accepting = match incoming.accept() {
                Ok(accepting) => accepting,
                Err(err) => {
                    warn!("Incoming connection rejected before ALPN exchange: {err}");
                    continue;
                }
            };

            let alpn = match accepting.alpn().await {
                Ok(alpn) => alpn,
                Err(err) => {
                    warn!("Failed ALPN read: {}", err);
                    continue;
                }
            };

            if alpn != crate::transport::iroh::derive_alpn(self.secret.as_deref()) {
                warn!(
                    "Ignoring unexpected ALPN: {}",
                    String::from_utf8_lossy(&alpn)
                );
                continue;
            }

            let conn = match accepting.await {
                Ok(conn) => conn,
                Err(err) => {
                    warn!("P2P connection handshake failed: {}", err);
                    continue;
                }
            };

            let (send, recv) = match conn.accept_bi().await {
                Ok(pair) => pair,
                Err(err) => {
                    warn!("Failed to establish bi-directional stream: {}", err);
                    continue;
                }
            };

            info!("Established bi-directional SSH stream over Irosh");

            let shell_state = ConnectionShellState::new();
            spawn_metadata_and_transfer_acceptor(conn, shell_state.clone());

            let stream = IrohDuplex::new(send, recv);
            let handler = ServerHandler::new(
                self.authorized_clients.clone(),
                self.security,
                self.state.clone(),
                shell_state,
            );

            let config = self.config.clone();
            tokio::spawn(async move {
                if let Err(err) = server::run_stream(config, stream, handler).await {
                    warn!("Server session error: {:?}", err);
                }
            });
        }

        self.endpoint.close().await;
        info!("Server shut down gracefully.");
        Ok(())
    }
}

#[cfg(test)]
mod tests;

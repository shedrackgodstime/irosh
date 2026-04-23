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
use crate::server::startup::bind_server;
use crate::transport::stream::IrohDuplex;

use self::side_streams::spawn_metadata_and_transfer_acceptor;
use self::transfer::ConnectionShellState;

/// Configuration options for the irosh server.
#[derive(Clone, Debug)]
pub struct ServerOptions {
    state: StateConfig,
    security: SecurityConfig,
    secret: Option<String>,
    authorized_keys: Vec<russh::keys::ssh_key::PublicKey>,
}

impl ServerOptions {
    /// Creates a new server options set with a specific state directory.
    pub fn new(state: StateConfig) -> Self {
        Self {
            state,
            security: SecurityConfig::default(),
            secret: None,
            authorized_keys: Vec::new(),
        }
    }

    /// Configures the security policy for host key trust.
    pub fn security(mut self, security: SecurityConfig) -> Self {
        self.security = security;
        self
    }

    /// Configures an optional shared secret for stealth connections.
    pub fn secret(mut self, secret: impl Into<String>) -> Self {
        self.secret = Some(secret.into());
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
/// used to generate the connection ticket.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerReady {
    /// The unique Iroh node ID of the server.
    pub endpoint_id: String,
    /// The connection ticket containing relay and addressing information.
    pub ticket: crate::transport::ticket::Ticket,
    /// The list of relay server URLs.
    pub relay_urls: Vec<String>,
    /// The list of directly reachable IP addresses.
    pub direct_addresses: Vec<String>,
    /// The OpenSSH formatted host public key.
    pub host_key_openssh: String,
}

impl ServerReady {
    /// Returns the unique Iroh node identifier.
    pub fn endpoint_id(&self) -> &str {
        &self.endpoint_id
    }

    /// Returns the connection ticket for this server.
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
    shutdown_tx: tokio::sync::mpsc::Sender<()>,
    shutdown_rx: tokio::sync::mpsc::Receiver<()>,
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
    shutdown_tx: tokio::sync::mpsc::Sender<()>,
}

impl ServerShutdown {
    /// Closes the underlying Iroh endpoint and stops accepting new connections.
    pub async fn close(self) {
        let _ = self.shutdown_tx.send(()).await;
        self.endpoint.close().await;
    }
}

impl Server {
    /// Inspects the server's readiness details without binding to the network.
    ///
    /// This is useful for pre-generating connection tickets before the server
    /// is fully operational.
    ///
    /// # Errors
    ///
    /// Returns an error if the server identity cannot be loaded or created.
    pub async fn inspect(options: &ServerOptions) -> Result<ServerReady> {
        startup::inspect_server(options).await
    }

    /// Binds the server to the Iroh networking stack and prepares for execution.
    ///
    /// This method starts the underlying Iroh endpoint, which might involve
    /// hole-punching and relay negotiation.
    ///
    /// ```no_run
    /// # use irosh::{Server, ServerOptions, StateConfig};
    /// # async fn example() -> irosh::error::Result<()> {
    /// let state = StateConfig::new("./state".into());
    /// let (ready, server) = Server::bind(ServerOptions::new(state)).await?;
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
            shutdown_tx: self.shutdown_tx.clone(),
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
    pub async fn run(mut self) -> Result<()> {
        use tokio::task::JoinSet;
        info!("Server actively listening for connections.");

        let mut sessions = JoinSet::new();

        loop {
            tokio::select! {
                biased;
                _ = self.shutdown_rx.recv() => {
                    tracing::debug!("Server received explicit shutdown signal.");
                    break;
                }
                incoming = self.endpoint.accept() => {
                    let Some(incoming) = incoming else {
                        tracing::debug!("Server endpoint closed, no more incoming connections.");
                        break;
                    };

                    tracing::debug!("Server accepted new incoming connection");
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

                    tracing::debug!("P2P connection established: {:?}", conn.remote_id());

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
                    sessions.spawn(async move {
                        tracing::debug!("Starting SSH session task");
                        if let Err(err) = server::run_stream(config, stream, handler).await {
                            warn!("Server session error: {:?}", err);
                        }
                        tracing::debug!("SSH session task finished");
                    });
                }
                res = sessions.join_next(), if !sessions.is_empty() => {
                    if let Some(res) = res {
                        match res {
                            Ok(()) => {},
                            Err(err) if err.is_cancelled() => {
                                tracing::debug!("SSH session task was cancelled.");
                            }
                            Err(err) => {
                                warn!("SSH session task panicked or failed: {:?}", err);
                            }
                        }
                    }
                }
            }
        }

        tracing::debug!(
            "Server loop exiting, waiting for {} sessions to finish",
            sessions.len()
        );
        while let Some(res) = sessions.join_next().await {
            match res {
                Ok(()) => {}
                Err(err) if err.is_cancelled() => {
                    tracing::debug!("SSH session task was cancelled during shutdown.");
                }
                Err(err) => {
                    warn!(
                        "SSH session task panicked or failed during shutdown: {:?}",
                        err
                    );
                }
            }
        }

        self.endpoint.close().await;
        info!("Server shut down gracefully.");
        Ok(())
    }
}

#[cfg(test)]
mod tests;

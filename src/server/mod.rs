//! SSH Server orchestration and connection handlers.

pub mod handler;
pub(crate) mod ipc;
pub(crate) mod shell_access;
pub(crate) mod side_streams;
pub(crate) mod startup;
pub(crate) mod transfer;

use russh::server;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tracing::{info, warn};

use crate::auth::Authenticator;

use crate::config::{SecurityConfig, StateConfig};
use crate::error::Result;
use crate::server::handler::ServerHandler;
use crate::server::startup::bind_server;
use crate::transport::stream::IrohDuplex;

use self::side_streams::spawn_metadata_and_transfer_acceptor;
use self::transfer::ConnectionShellState;

/// Configuration options for the irosh server.
#[derive(Debug)]
pub struct ServerOptions {
    state: StateConfig,
    security: SecurityConfig,
    pub(crate) secret: Option<String>,
    pub(crate) ipc_enabled: bool,
    pub(crate) relay_mode: iroh::RelayMode,
    pub(crate) relay_url: Option<String>,
    authorized_keys: Vec<russh::keys::ssh_key::PublicKey>,
    authenticator: Option<Arc<dyn Authenticator>>,
    wormhole_confirmation: Option<Arc<dyn crate::auth::ConfirmationCallback>>,
}

impl Clone for ServerOptions {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            security: self.security,
            ipc_enabled: self.ipc_enabled,
            secret: self.secret.clone(),
            relay_mode: self.relay_mode.clone(),
            relay_url: self.relay_url.clone(),
            authorized_keys: self.authorized_keys.clone(),
            authenticator: self.authenticator.clone(),
            wormhole_confirmation: self.wormhole_confirmation.clone(),
        }
    }
}

impl ServerOptions {
    /// Creates a new server options set with a specific state directory.
    pub fn new(state: StateConfig) -> Self {
        Self {
            state,
            ipc_enabled: true,
            security: SecurityConfig::default(),
            secret: None,
            relay_mode: iroh::RelayMode::Default,
            relay_url: None,
            authorized_keys: Vec::new(),
            authenticator: None,
            wormhole_confirmation: None,
        }
    }

    /// Configures the relay mode for the server.
    pub fn relay_mode(mut self, mode: iroh::RelayMode, url: Option<String>) -> Self {
        self.relay_mode = mode;
        self.relay_url = url;
        self
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

    /// Sets a custom authentication backend.
    ///
    /// This replaces the default key-only authentication with a pluggable
    /// backend. See [`crate::auth`] for built-in options.
    ///
    /// If not called, the server uses [`crate::auth::KeyOnlyAuth`] with
    /// the configured security policy (backward compatible).
    pub fn authenticator(mut self, auth: impl Authenticator) -> Self {
        self.authenticator = Some(Arc::new(auth));
        self
    }

    /// Sets a confirmation callback for interactive wormhole pairing.
    ///
    /// When set, the server will invoke this callback to prompt the operator
    /// before authorizing a wormhole pairing request. This is used by the
    /// foreground CLI to display a y/n prompt.
    pub fn wormhole_confirmation(
        mut self,
        callback: impl crate::auth::ConfirmationCallback,
    ) -> Self {
        self.wormhole_confirmation = Some(Arc::new(callback));
        self
    }

    /// Disables the IPC control server. Useful for foreground/ephemeral servers.
    pub fn disable_ipc(mut self) -> Self {
        self.ipc_enabled = false;
        self
    }

    pub fn state(&self) -> &StateConfig {
        &self.state
    }

    pub(crate) fn security_config(&self) -> SecurityConfig {
        self.security
    }

    pub(crate) fn secret_value(&self) -> Option<&str> {
        self.secret.as_deref()
    }

    #[allow(dead_code)]
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
    ipc_enabled: bool,
    config: Arc<server::Config>,
    authenticator: Arc<dyn Authenticator>,
    state: StateConfig,
    security: SecurityConfig,
    secret: Option<String>,
    shutdown_tx: tokio::sync::mpsc::Sender<()>,
    shutdown_rx: tokio::sync::mpsc::Receiver<()>,
    control_tx: tokio::sync::mpsc::Sender<ipc::InternalCommand>,
    control_rx: tokio::sync::mpsc::Receiver<ipc::InternalCommand>,
    ticket: crate::transport::ticket::Ticket,
    gossip: iroh_gossip::net::Gossip,
    /// Reserved for future server-side interactive pairing confirmation prompts.
    #[allow(dead_code)]
    wormhole_confirmation: Option<Arc<dyn crate::auth::ConfirmationCallback>>,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server")
            .field("authenticator", &self.authenticator)
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

    /// Returns a channel to send control commands to the server loop.
    pub fn control_handle(&self) -> tokio::sync::mpsc::Sender<ipc::InternalCommand> {
        self.control_tx.clone()
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

        // Spawn the IPC control server if enabled.
        if self.ipc_enabled {
            let ipc_server =
                ipc::IpcServer::new(self.state.root().to_path_buf(), self.control_tx.clone());
            tokio::spawn(async move {
                if let Err(e) = ipc_server.run().await {
                    warn!("IPC server failed: {}", e);
                }
            });
        }
        struct ActiveWormhole {
            #[allow(dead_code)]
            code: String,
            password: Option<String>,
            persistent: bool,
            task: tokio::task::JoinHandle<()>,
            failed_attempts: Arc<AtomicU32>,
            success: Arc<std::sync::atomic::AtomicBool>,
            expiry_task: tokio::task::JoinHandle<()>,
        }

        let mut wormhole: Option<ActiveWormhole> = None;

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

                    if alpn == iroh_gossip::ALPN {
                        let gossip = self.gossip.clone();
                        tokio::spawn(async move {
                            match accepting.await {
                                Ok(conn) => {
                                    if let Err(e) = gossip.handle_connection(conn).await {
                                        tracing::debug!("Gossip connection handling failed: {}", e);
                                    }
                                }
                                Err(e) => {
                                    tracing::debug!("Failed to confirm Gossip connection: {}", e);
                                }
                            }
                        });
                        continue;
                    }

                    let primary_alpn = crate::transport::iroh::derive_alpn(self.secret.as_deref());
                    let is_pairing_alpn = alpn == crate::transport::wormhole::PAIRING_ALPN;

                    if alpn != primary_alpn && !is_pairing_alpn {
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
                    let mut session_authenticator = self.authenticator.clone();
                    // By default, use the server-wide config (pre-built method set).
                    let mut session_config = self.config.clone();

                    if is_pairing_alpn {
                        if let Some(wh) = &wormhole {
                            info!("Pairing connection established via wormhole code.");

                            let vault = crate::storage::load_all_authorized_clients(&self.state).unwrap_or_default();
                            let keys: Vec<_> = vault.into_iter().map(|(_, k)| k).collect();
                            let node_password = crate::storage::load_shadow_file(&self.state).unwrap_or_default();

                            let pairing_auth = crate::auth::UnifiedAuthenticator::with_tracking(
                                self.state.clone(),
                                self.security.host_key_policy,
                                keys,
                                node_password,
                                wh.password.clone(),
                                wh.success.clone(),
                                wh.failed_attempts.clone(),
                            );

                            // CRITICAL: Build a per-session russh::Config that advertises
                            // the correct auth methods for THIS connection. The server-wide
                            // config may not include Password if no permanent password was
                            // set at startup, but this session may have a temp password.
                            let pairing_methods = pairing_auth.supported_methods();
                            let mut pairing_method_set = russh::MethodSet::empty();
                            for m in &pairing_methods {
                                match m {
                                    crate::auth::AuthMethod::PublicKey => pairing_method_set.push(russh::MethodKind::PublicKey),
                                    crate::auth::AuthMethod::Password => pairing_method_set.push(russh::MethodKind::Password),
                                }
                            }
                            session_config = Arc::new(russh::server::Config {
                                auth_rejection_time: self.config.auth_rejection_time,
                                keys: self.config.keys.clone(),
                                methods: pairing_method_set,
                                ..Default::default()
                            });

                            session_authenticator = Arc::new(pairing_auth);
                        } else {
                            warn!("Pairing connection attempted but no wormhole active.");
                            continue;
                        }
                    }

                    let handler = ServerHandler::new(
                        session_authenticator,
                        shell_state,
                    );

                    let config = session_config;
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

                    // After any session completes, check if the wormhole needs to be rate-limited
                    // or auto-burned on success.
                    if let Some(wh) = &wormhole {
                        let fails = wh.failed_attempts.load(Ordering::Relaxed);
                        let success = wh.success.load(Ordering::Relaxed);

                        if fails >= 3 {
                            warn!("Wormhole rate limit exceeded ({} failed attempts). Burning wormhole.", fails);
                            wh.task.abort();
                            wh.expiry_task.abort();
                            let code = wh.code.clone();
                            tokio::spawn(async move {
                                let _ = crate::transport::wormhole::unpublish_ticket(&code).await;
                            });
                            wormhole = None;
                        } else if success && !wh.persistent {
                            info!("Wormhole successfully consumed. Auto-burning.");
                            wh.task.abort();
                            wh.expiry_task.abort();
                            let code = wh.code.clone();
                            tokio::spawn(async move {
                                let _ = crate::transport::wormhole::unpublish_ticket(&code).await;
                            });
                            wormhole = None;
                        }
                    }
                }
                msg = self.control_rx.recv() => {
                    if let Some(msg) = msg {
                        match msg {
                            ipc::InternalCommand::EnableWormhole {
                                code,
                                password,
                                persistent,
                                tx,
                            } => {
                                info!(
                                    "Wormhole enabled via IPC: {} (persistent: {})",
                                    code, persistent
                                );

                                // Abort existing wormhole if any.
                                if let Some(wh) = wormhole.take() {
                                    wh.task.abort();
                                    wh.expiry_task.abort();
                                    let code = wh.code.clone();
                                    tokio::spawn(async move {
                                        let _ =
                                            crate::transport::wormhole::unpublish_ticket(&code).await;
                                    });
                                }

                                let gossip = self.gossip.clone();
                                let ticket = self.ticket.clone();
                                let code_clone = code.clone();
                                let task = tokio::spawn(async move {
                                    if let Err(e) = crate::transport::wormhole::broadcast_ticket_loop(
                                        &gossip,
                                        &code_clone,
                                        ticket,
                                    )
                                    .await
                                    {
                                        warn!("Wormhole broadcast failed: {}", e);
                                    }
                                });

                                // 24-hour expiry timer: automatically disables
                                // the wormhole if no pairing occurs.
                                let expiry_control = self.control_tx.clone();
                                let expiry_task = tokio::spawn(async move {
                                    tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60))
                                        .await;
                                    info!("Wormhole expired after 24 hours.");
                                    let (res_tx, _) = tokio::sync::oneshot::channel();
                                    let _ = expiry_control
                                        .send(ipc::InternalCommand::DisableWormhole { tx: res_tx })
                                        .await;
                                });

                                wormhole = Some(ActiveWormhole {
                                    code,
                                    password: password.clone(),
                                    persistent,
                                    task,
                                    failed_attempts: Arc::new(AtomicU32::new(0)),
                                    success: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                                    expiry_task,
                                });
                                let _ = tx.send(ipc::IpcResponse::Ok);
                            }
                            ipc::InternalCommand::DisableWormhole { tx } => {
                                info!("Wormhole disabled via IPC");
                                if let Some(wh) = wormhole.take() {
                                    wh.task.abort();
                                    wh.expiry_task.abort();
                                    let code = wh.code.clone();
                                    tokio::spawn(async move {
                                        let _ =
                                            crate::transport::wormhole::unpublish_ticket(&code).await;
                                    });
                                }
                                let _ = tx.send(ipc::IpcResponse::Ok);
                            }
                            ipc::InternalCommand::GetStatus { tx } => {
                                let _ = tx.send(ipc::IpcResponse::Status {
                                    wormhole_active: wormhole.is_some(),
                                    wormhole_code: wormhole.as_ref().map(|w| w.code.clone()),
                                    active_sessions: sessions.len(),
                                });
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

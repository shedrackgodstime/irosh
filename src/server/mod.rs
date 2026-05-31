//! P2P SSH Server implementation.
//!
//! This module provides the [`Server`] struct, which orchestrates the
//! life-cycle of an Irosh host. It handles incoming QUIC connections,
//! manages SSH channel multiplexing, and provides an IPC interface
//! for background control.
//!
//! ## Architecture
//!
//! The server runs on top of an [`iroh::Endpoint`]. It listens for
//! connections with specific ALPNs (Application-Layer Protocol Negotiation):
//! - `irosh/primary/v1`: The standard P2P SSH session.
//! - `irosh/pairing/v1`: Temporary ad-hoc pairing via Wormhole.
//!
//! For every connection, the server spawns a dedicated task that
//! handles PTY allocation and command execution.

pub mod handler;
pub mod ipc;
pub(crate) mod shell_access;
pub(crate) mod side_streams;
pub(crate) mod startup;
pub(crate) mod transfer;

use russh::server;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::auth::Authenticator;

use crate::config::{SecurityConfig, StateConfig};
use crate::error::Result;
use crate::server::handler::ServerHandler;
use crate::server::startup::bind_server;
use crate::transport::stream::IrohDuplex;

use self::side_streams::spawn_side_stream_listener;
use self::transfer::ConnectionShellState;

/// Configuration options for the irosh server.
#[derive(Debug)]
#[must_use = "builders do nothing unless consumed"]
pub struct ServerOptions {
    state: StateConfig,
    security: SecurityConfig,
    pub(crate) secret: Option<String>,
    pub(crate) ipc_enabled: bool,
    pub(crate) relay_mode: iroh::RelayMode,
    pub(crate) relay_url: Option<String>,
    authorized_keys: Vec<russh::keys::ssh_key::PublicKey>,
    authenticator: Option<Arc<dyn Authenticator>>,
    pub(crate) shutdown_on_wormhole_success: bool,
    pub(crate) auth_mode: crate::auth::AuthMode,
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
            shutdown_on_wormhole_success: self.shutdown_on_wormhole_success,
            auth_mode: self.auth_mode,
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
            shutdown_on_wormhole_success: false,
            auth_mode: crate::auth::AuthMode::Unified,
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

    /// Configures the authentication mode for the server.
    pub fn auth_mode(mut self, mode: crate::auth::AuthMode) -> Self {
        self.auth_mode = mode;
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

    /// Disables the IPC control server. Useful for foreground/ephemeral servers.
    pub fn disable_ipc(mut self) -> Self {
        self.ipc_enabled = false;
        self
    }

    /// Automatically shutdowns the server after a successful wormhole pairing.
    pub fn shutdown_on_wormhole_success(mut self) -> Self {
        self.shutdown_on_wormhole_success = true;
        self
    }

    /// Returns a reference to the [`StateConfig`] this server was configured with.
    pub fn state(&self) -> &StateConfig {
        &self.state
    }

    pub(crate) fn security_config(&self) -> SecurityConfig {
        self.security
    }

    /// Returns the optional shared secret for wormhole authentication.
    pub fn secret_value(&self) -> Option<&str> {
        self.secret.as_deref()
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

/// A currently connected remote peer session.
#[derive(Debug, Clone)]
pub(crate) struct ActiveSession {
    /// The remote peer's node identifier.
    pub(crate) peer_id: String,
    /// Timestamp when this session was established.
    pub(crate) started_at: chrono::DateTime<chrono::Utc>,
    /// Total bytes transmitted to the peer.
    pub(crate) bytes_sent: Arc<AtomicU64>,
    /// Total bytes received from the peer.
    pub(crate) bytes_received: Arc<AtomicU64>,
}

/// Manages the set of active remote sessions.
#[derive(Default, Clone)]
pub(crate) struct SessionTracker {
    /// Map of session IDs to active session state.
    pub(crate) sessions: Arc<Mutex<HashMap<usize, ActiveSession>>>,
    /// Monotonically increasing session ID counter.
    pub(crate) next_id: Arc<AtomicUsize>,
}

impl SessionTracker {
    /// Creates a new empty tracker.
    fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Registers a new session and returns its ID and byte-counting atomics.
    async fn register(&self, peer_id: String) -> (usize, Arc<AtomicU64>, Arc<AtomicU64>) {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let sent = Arc::new(AtomicU64::new(0));
        let received = Arc::new(AtomicU64::new(0));
        let session = ActiveSession {
            peer_id,
            started_at: chrono::Utc::now(),
            bytes_sent: sent.clone(),
            bytes_received: received.clone(),
        };
        self.sessions.lock().await.insert(id, session);
        (id, sent, received)
    }

    /// Removes a session from the tracker by its ID.
    async fn unregister(&self, id: usize) {
        self.sessions.lock().await.remove(&id);
    }

    /// Returns a snapshot of all active sessions for IPC reporting.
    async fn snapshot(&self) -> Vec<ipc::SessionStatus> {
        let sessions = self.sessions.lock().await;
        sessions
            .values()
            .map(|s| ipc::SessionStatus {
                peer_id: s.peer_id.clone(),
                started_at: s.started_at.to_rfc3339(),
                bytes_sent: s.bytes_sent.load(std::sync::atomic::Ordering::Relaxed),
                bytes_received: s.bytes_received.load(std::sync::atomic::Ordering::Relaxed),
            })
            .collect()
    }
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
    blobs: iroh_blobs::store::fs::FsStore,
    shutdown_on_wormhole_success: bool,
    session_tracker: SessionTracker,
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

struct ActiveWormhole {
    code: String,
    password: Option<String>,
    persistent: bool,
    task: tokio::task::JoinHandle<()>,
    failed_attempts: Arc<AtomicU32>,
    success: Arc<std::sync::atomic::AtomicBool>,
    expiry_task: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
struct GossipProtocol(iroh_gossip::net::Gossip);

impl std::fmt::Debug for GossipProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GossipProtocol")
    }
}

impl iroh::protocol::ProtocolHandler for GossipProtocol {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> std::result::Result<(), iroh::protocol::AcceptError> {
        if let Err(e) = self.0.handle_connection(connection).await {
            tracing::debug!("Gossip connection handling failed: {}", e);
        }
        Ok(())
    }
}

#[derive(Clone)]
struct SshProtocol {
    is_pairing: bool,
    state: StateConfig,
    security: SecurityConfig,
    config: Arc<server::Config>,
    authenticator: Arc<dyn Authenticator>,
    wormhole: Arc<tokio::sync::Mutex<Option<ActiveWormhole>>>,
    success_tx: tokio::sync::mpsc::Sender<()>,
    failure_tx: tokio::sync::mpsc::Sender<()>,
    active_sessions: Arc<std::sync::atomic::AtomicUsize>,
    blobs: iroh_blobs::store::fs::FsStore,
    session_tracker: Arc<SessionTracker>,
}

impl std::fmt::Debug for SshProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SshProtocol {{ is_pairing: {} }}", self.is_pairing)
    }
}

impl iroh::protocol::ProtocolHandler for SshProtocol {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> std::result::Result<(), iroh::protocol::AcceptError> {
        self.active_sessions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        struct SessionGuard(
            Arc<std::sync::atomic::AtomicUsize>,
            usize,
            Arc<SessionTracker>,
        );
        impl Drop for SessionGuard {
            fn drop(&mut self) {
                self.0.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                let id = self.1;
                let tracker = self.2.clone();
                tokio::spawn(async move {
                    tracker.unregister(id).await;
                });
            }
        }

        let (session_id, bytes_sent, bytes_received) = self
            .session_tracker
            .register(connection.remote_id().to_string())
            .await;

        let _guard = SessionGuard(
            self.active_sessions.clone(),
            session_id,
            self.session_tracker.clone(),
        );

        tracing::debug!("P2P connection established: {:?}", connection.remote_id());

        let (send, recv) = match connection.accept_bi().await {
            Ok(pair) => pair,
            Err(err) => {
                warn!("Failed to establish bi-directional stream: {}", err);
                return Ok(());
            }
        };

        info!("Established bi-directional SSH stream over Irosh");

        let shell_state =
            ConnectionShellState::new(self.state.root().to_path_buf(), self.blobs.clone());
        spawn_side_stream_listener(connection, shell_state.clone());

        let stream = IrohDuplex::with_stats(send, recv, bytes_sent, bytes_received);
        let mut session_authenticator = self.authenticator.clone();
        let mut session_config = self.config.clone();

        if self.is_pairing {
            let mut wh_lock = self.wormhole.lock().await;
            if let Some(wh) = wh_lock.as_mut() {
                info!("Pairing connection established via wormhole code.");
                let vault =
                    crate::storage::load_all_authorized_clients(&self.state).unwrap_or_default();
                let keys: Vec<_> = vault.into_iter().map(|(_, k)| k).collect();

                let pairing_auth = crate::auth::UnifiedAuthenticator::with_tracking(
                    self.state.clone(),
                    self.security.host_key_policy,
                    keys,
                    wh.password.clone(),
                    crate::auth::PairingMonitor {
                        success_flag: wh.success.clone(),
                        failed_attempts: wh.failed_attempts.clone(),
                        success_tx: Some(self.success_tx.clone()),
                        failure_tx: Some(self.failure_tx.clone()),
                    },
                );

                let pairing_methods = pairing_auth.supported_methods();
                let mut pairing_method_set = russh::MethodSet::empty();
                for m in &pairing_methods {
                    match m {
                        crate::auth::AuthMethod::PublicKey => {
                            pairing_method_set.push(russh::MethodKind::PublicKey)
                        }
                        crate::auth::AuthMethod::Password => {
                            pairing_method_set.push(russh::MethodKind::Password)
                        }
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
                return Ok(());
            }
        }

        let handler = ServerHandler::new(session_authenticator, shell_state);
        let config = session_config;

        tracing::debug!("Starting SSH session task");
        if let Err(err) = server::run_stream(config, stream, handler).await {
            warn!("Server session error: {:?}", err);
        }
        tracing::debug!("SSH session task finished");

        Ok(())
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
        info!("Server actively listening for connections.");

        let (ipc_shutdown_tx, ipc_shutdown_rx) = tokio::sync::mpsc::channel(1);

        // Spawn the IPC control server if enabled.
        if self.ipc_enabled {
            let ipc_server =
                ipc::IpcServer::new(self.state.root().to_path_buf(), self.control_tx.clone());
            tokio::spawn(async move {
                if let Err(e) = ipc_server.run(ipc_shutdown_rx).await {
                    warn!("IPC server failed: {}", e);
                }
            });
        }

        let wormhole = Arc::new(tokio::sync::Mutex::new(None));
        let (success_tx, mut success_rx) = tokio::sync::mpsc::channel(1);
        let (failure_tx, mut failure_rx) = tokio::sync::mpsc::channel(1);
        let active_sessions = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let primary_alpn = crate::transport::iroh::derive_alpn(self.secret.as_deref());
        let pairing_alpn = crate::transport::wormhole::PAIRING_ALPN.to_vec();

        let base_protocol = SshProtocol {
            is_pairing: false,
            state: self.state.clone(),
            security: self.security,
            config: self.config.clone(),
            authenticator: self.authenticator.clone(),
            wormhole: wormhole.clone(),
            success_tx: success_tx.clone(),
            failure_tx: failure_tx.clone(),
            active_sessions: active_sessions.clone(),
            blobs: self.blobs.clone(),
            session_tracker: Arc::new(SessionTracker::new()),
        };

        let mut pairing_protocol = base_protocol.clone();
        pairing_protocol.is_pairing = true;

        let blobs_protocol = iroh_blobs::BlobsProtocol::new(&self.blobs, None);

        let stealth_mode = self.secret.is_some();

        let mut builder = iroh::protocol::Router::builder(self.endpoint.clone())
            .accept(primary_alpn, base_protocol)
            .accept(iroh_gossip::ALPN, GossipProtocol(self.gossip.clone()))
            .accept(iroh_blobs::ALPN, blobs_protocol);

        // In stealth mode the pairing ALPN was never bound on the endpoint,
        // so there is no point registering a handler for it.
        if !stealth_mode {
            builder = builder.accept(pairing_alpn, pairing_protocol);
        }

        let router = builder.spawn();

        let mut shutdown_requested = false;

        loop {
            if shutdown_requested && active_sessions.load(std::sync::atomic::Ordering::Relaxed) == 0
            {
                break;
            }

            tokio::select! {
                _ = self.shutdown_rx.recv() => {
                    tracing::debug!("Server received explicit shutdown signal.");
                    shutdown_requested = true;
                    let _ = ipc_shutdown_tx.send(()).await;
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

                                let mut wh_lock = wormhole.lock().await;
                                if let Some(wh) = wh_lock.take() {
                                    wh.task.abort();
                                    wh.expiry_task.abort();
                                    let old_code = wh.code.clone();
                                    tokio::spawn(async move {
                                        let _ = crate::transport::wormhole::unpublish_ticket(&old_code).await;
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

                                *wh_lock = Some(ActiveWormhole {
                                    code,
                                    password,
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
                                let mut wh_lock = wormhole.lock().await;
                                if let Some(wh) = wh_lock.take() {
                                    wh.task.abort();
                                    wh.expiry_task.abort();
                                    let old_code = wh.code.clone();
                                    tokio::spawn(async move {
                                        let _ = crate::transport::wormhole::unpublish_ticket(&old_code).await;
                                    });
                                }
                                let _ = tx.send(ipc::IpcResponse::Ok);
                            }
                            ipc::InternalCommand::GetStatus { tx } => {
                                let wh_lock = wormhole.lock().await;
                                let sessions = self.session_tracker.snapshot().await;
                                let _ = tx.send(ipc::IpcResponse::Status(ipc::DaemonStatus {
                                    endpoint_id: self.endpoint.id().to_string(),
                                    ticket: self.ticket.to_string(),
                                    wormhole_active: wh_lock.is_some(),
                                    wormhole_code: wh_lock.as_ref().map(|w| w.code.clone()),
                                    active_sessions: active_sessions.load(std::sync::atomic::Ordering::Relaxed),
                                    sessions,
                                }));
                            }
                            ipc::InternalCommand::Shutdown { tx } => {
                                info!("Graceful shutdown requested via IPC");
                                shutdown_requested = true;
                                let _ = ipc_shutdown_tx.send(()).await;
                                let _ = tx.send(ipc::IpcResponse::Ok);
                            }
                        }
                    } else {
                        break;
                    }
                }
                _ = success_rx.recv() => {
                    let mut wh_lock = wormhole.lock().await;
                    if let Some(wh) = wh_lock.as_ref() {
                        if !wh.persistent {
                            info!("Wormhole pairing successful. Auto-burning.");
                            wh.task.abort();
                            wh.expiry_task.abort();
                            let code = wh.code.clone();
                            tokio::spawn(async move {
                                let _ = crate::transport::wormhole::unpublish_ticket(&code).await;
                            });

                            if self.shutdown_on_wormhole_success {
                                info!("Shutdown on wormhole success requested.");
                                shutdown_requested = true;
                                let _ = ipc_shutdown_tx.send(()).await;
                            }
                            *wh_lock = None;
                        }
                    }
                }
                _ = failure_rx.recv() => {
                    let mut wh_lock = wormhole.lock().await;
                    if let Some(wh) = wh_lock.take() {
                        warn!("Wormhole rate limit exceeded. Burning wormhole.");
                        wh.task.abort();
                        wh.expiry_task.abort();
                        let code = wh.code.clone();
                        tokio::spawn(async move {
                            let _ = crate::transport::wormhole::unpublish_ticket(&code).await;
                        });
                    }
                }
            }

            if shutdown_requested && active_sessions.load(std::sync::atomic::Ordering::Relaxed) == 0
            {
                info!("Shutdown conditions met. Exiting server loop.");
                break;
            }
        }

        tracing::debug!("Server loop exiting, waiting for sessions to finish");
        while active_sessions.load(std::sync::atomic::Ordering::Relaxed) > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        let _ = router.shutdown().await;
        self.endpoint.close().await;
        info!("Server shut down gracefully.");
        Ok(())
    }
}

#[cfg(test)]
mod tests;

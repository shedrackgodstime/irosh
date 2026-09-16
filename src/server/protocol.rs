//! ALPN protocol handlers: the primary SSH stream and the Gossip overlay.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize};

use russh::server;
use tracing::{info, warn};

use crate::auth::Authenticator;
use crate::config::{PeerId, SecurityConfig, StateConfig};
use crate::server::handler::ServerHandler;
use crate::server::side_streams::spawn_side_stream_listener;
use crate::server::transfer::ConnectionShellState;
use crate::transport::stream::IrohDuplex;

use super::session_tracker::SessionTracker;

#[derive(Clone)]
pub(super) struct GossipProtocol(pub(super) iroh_gossip::net::Gossip);

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
pub(super) struct SshProtocol {
    pub(super) is_pairing: bool,
    pub(super) state: StateConfig,
    pub(super) security: SecurityConfig,
    pub(super) config: Arc<server::Config>,
    pub(super) authenticator: Arc<dyn Authenticator>,
    pub(super) wormhole: Arc<tokio::sync::Mutex<Option<ActiveWormhole>>>,
    pub(super) success_tx: tokio::sync::mpsc::Sender<()>,
    pub(super) failure_tx: tokio::sync::mpsc::Sender<()>,
    pub(super) active_sessions: Arc<AtomicUsize>,
    pub(super) blobs: iroh_blobs::store::fs::FsStore,
    pub(super) session_tracker: Arc<SessionTracker>,
    pub(super) metrics: crate::metrics::Metrics,
    pub(super) idle_timeout: std::time::Duration,
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

        self.active_sessions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let _metrics_guard = self.metrics.register_connection();

        let (session_id, bytes_sent, bytes_received) = self
            .session_tracker
            .register(PeerId::new(connection.remote_id().to_string()))
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
        let metrics = self.metrics.clone();
        let (auth_gate_tx, auth_gate_rx) = tokio::sync::watch::channel(false);
        spawn_side_stream_listener(
            connection,
            shell_state.clone(),
            metrics.clone(),
            auth_gate_rx,
        );

        let stream = IrohDuplex::with_stats(send, recv, bytes_sent, bytes_received);
        let mut session_authenticator = self.authenticator.clone();
        let mut session_config = self.config.clone();

        if self.is_pairing {
            // Extract pairing data while holding lock briefly, then release before blocking await.
            let (password, success, failed_attempts, wormhole_active) = {
                let wh_lock = self.wormhole.lock().await;
                if let Some(wh) = wh_lock.as_ref() {
                    (
                        wh.password.clone(),
                        wh.success.clone(),
                        wh.failed_attempts.clone(),
                        true,
                    )
                } else {
                    (
                        None,
                        Arc::new(std::sync::atomic::AtomicBool::new(false)),
                        Arc::new(std::sync::atomic::AtomicU32::new(0)),
                        false,
                    )
                }
            };

            if !wormhole_active {
                warn!("Pairing connection attempted but no wormhole active.");
                return Ok(());
            }

            info!("Pairing connection established via wormhole code.");
            let vault = match tokio::task::spawn_blocking({
                let state = self.state.clone();
                move || crate::storage::load_all_authorized_clients(&state)
            })
            .await
            {
                Ok(Ok(vault)) => vault,
                _ => Vec::new(),
            };
            let keys: Vec<_> = vault.into_iter().map(|(_, k)| k).collect();

            let pairing_auth = crate::auth::UnifiedAuthenticator::with_tracking(
                self.state.clone(),
                self.security.host_key_policy,
                keys,
                password,
                crate::auth::PairingMonitor {
                    success_flag: success,
                    failed_attempts: failed_attempts,
                    success_tx: Some(self.success_tx.clone()),
                    failure_tx: Some(self.failure_tx.clone()),
                },
            );

            let pairing_methods = pairing_auth.supported_methods().await;
            let mut pairing_method_set = russh::MethodSet::empty();
            for m in &pairing_methods {
                match m {
                    crate::auth::AuthMethod::PublicKey => {
                        pairing_method_set.push(russh::MethodKind::PublicKey);
                    }
                    crate::auth::AuthMethod::Password => {
                        pairing_method_set.push(russh::MethodKind::Password);
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
        }

        let handler = ServerHandler::with_metrics(session_authenticator, shell_state, metrics)
            .with_auth_gate(auth_gate_tx)
            .with_idle_timeout(self.idle_timeout);
        let config = session_config;

        tracing::debug!("Starting SSH session task");
        if let Err(err) = server::run_stream(config, stream, handler).await {
            warn!("Server session error: {:?}", err);
        }
        tracing::debug!("SSH session task finished");

        Ok(())
    }
}

pub(super) struct ActiveWormhole {
    pub(super) code: String,
    pub(super) password: Option<String>,
    pub(super) persistent: bool,
    pub(super) task: tokio::task::JoinHandle<()>,
    pub(super) failed_attempts: Arc<AtomicU32>,
    pub(super) success: Arc<std::sync::atomic::AtomicBool>,
    pub(super) expiry_task: tokio::task::JoinHandle<()>,
}

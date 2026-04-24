//! SSH client handler trait implementations governing session verification.

use std::sync::{Arc, Mutex as StdMutex, MutexGuard};

use russh::client::{self, DisconnectReason};
use russh::keys::ssh_key::{HashAlg, PublicKey};
use tracing::{info, warn};

use crate::config::{SecurityConfig, StateConfig};
use crate::error::IroshError;
use crate::storage::trust::write_known_server;

/// A registry mapping (remote_host, remote_port) to (local_host, local_port)
type TunnelRegistry = Arc<StdMutex<std::collections::HashMap<(String, u32), (String, u16)>>>;

/// Handles incoming connection verification events from the SSH server.
#[derive(Clone)]
pub struct ClientHandler {
    node_id: String,
    known_server: Arc<StdMutex<Option<PublicKey>>>,
    last_disconnect: Arc<StdMutex<Option<String>>>,
    security: SecurityConfig,
    state: StateConfig,
    remote_tunnels: TunnelRegistry,
}

impl ClientHandler {
    /// Creates a new `ClientHandler` with the designated server validation state.
    pub(crate) fn new(
        node_id: String,
        known_server: Option<PublicKey>,
        last_disconnect: Arc<StdMutex<Option<String>>>,
        security: SecurityConfig,
        state: StateConfig,
    ) -> Self {
        Self {
            node_id,
            known_server: Arc::new(StdMutex::new(known_server)),
            last_disconnect,
            security,
            state,
            remote_tunnels: Arc::new(StdMutex::new(std::collections::HashMap::new())),
        }
    }

    /// Registers a mapping for an incoming remote port forward.
    pub(crate) fn register_remote_tunnel(
        &self,
        remote_host: String,
        remote_port: u32,
        local_host: String,
        local_port: u16,
    ) {
        if let Ok(mut tunnels) = self.remote_tunnels.lock() {
            tunnels.insert((remote_host, remote_port), (local_host, local_port));
        }
    }

    fn lock_known_server(&self) -> MutexGuard<'_, Option<PublicKey>> {
        match self.known_server.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("known server state mutex poisoned; recovering inner state");
                poisoned.into_inner()
            }
        }
    }

    fn lock_last_disconnect(&self) -> MutexGuard<'_, Option<String>> {
        match self.last_disconnect.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("disconnect summary mutex poisoned; recovering inner state");
                poisoned.into_inner()
            }
        }
    }
}

impl client::Handler for ClientHandler {
    type Error = IroshError;

    async fn check_server_key(
        &mut self,
        key: &PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        use crate::config::HostKeyPolicy;

        if self.security.host_key_policy == HostKeyPolicy::AcceptAll {
            info!("AcceptAll policy is active. Automatically accepting server key.");
            return Ok(true);
        }

        let mut known = self.lock_known_server();

        match known.as_ref() {
            Some(known_key) if key == known_key => {
                info!("Server matched trusted key. Connection verified.");
                Ok(true)
            }
            Some(known_key) => {
                let expected = known_key.fingerprint(HashAlg::Sha256);
                let actual = key.fingerprint(HashAlg::Sha256);
                warn!(
                    "SECURITY WARNING: Server key mismatch! Expected {}, got {}",
                    expected, actual
                );
                Err(IroshError::ServerKeyMismatch {
                    expected: expected.to_string(),
                    actual: actual.to_string(),
                })
            }
            None => match self.security.host_key_policy {
                HostKeyPolicy::Strict => {
                    warn!("No trusted server key found and strict host key checking is active.");
                    Err(IroshError::ServerKeyMismatch {
                        expected: "trusted key".to_string(),
                        actual: "unknown key".to_string(),
                    })
                }
                HostKeyPolicy::Tofu => {
                    info!("No trusted server key found. Trusting server key on first use.");
                    let event = write_known_server(&self.state, &self.node_id, key)?;
                    info!(
                        "Trusted first server key and saved it to {}",
                        event.path.display()
                    );
                    *known = Some(key.clone());
                    Ok(true)
                }
                HostKeyPolicy::AcceptAll => {
                    unreachable!("AcceptAll already handled at top of function")
                }
            },
        }
    }

    async fn disconnected(
        &mut self,
        reason: DisconnectReason<Self::Error>,
    ) -> std::result::Result<(), Self::Error> {
        let summary = match &reason {
            DisconnectReason::ReceivedDisconnect(info) => {
                let message = info.message.trim();
                if message.is_empty() {
                    format!("server disconnected with reason {:?}", info.reason_code)
                } else {
                    format!(
                        "server disconnected with reason {:?}: {}",
                        info.reason_code, message
                    )
                }
            }
            DisconnectReason::Error(err) => err.to_string(),
        };

        *self.lock_last_disconnect() = Some(summary);

        match reason {
            DisconnectReason::ReceivedDisconnect(_) => Ok(()),
            DisconnectReason::Error(err) => Err(err),
        }
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut client::Session,
    ) -> std::result::Result<(), Self::Error> {
        let target = {
            let tunnels = self.remote_tunnels.lock().map_err(|_| {
                IroshError::Client(crate::error::ClientError::TunnelFailed {
                    details: "mutex poisoned".to_string(),
                })
            })?;
            // We check for (host, port) match.
            tunnels
                .get(&(connected_address.to_string(), connected_port))
                .or_else(|| {
                    // Also try with "0.0.0.0" or "localhost" if exact host doesn't match
                    // but port does.
                    tunnels
                        .iter()
                        .find(|((_, p), _)| *p == connected_port)
                        .map(|(_, target)| target)
                })
                .cloned()
        };

        if let Some((local_host, local_port)) = target {
            tokio::spawn(async move {
                if let Ok(mut stream) =
                    tokio::net::TcpStream::connect(format!("{}:{}", local_host, local_port)).await
                {
                    let mut channel_stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut channel_stream, &mut stream).await;
                }
            });
        } else {
            warn!(
                "Received unexpected remote forward request for {}:{}",
                connected_address, connected_port
            );
        }
        Ok(())
    }
}

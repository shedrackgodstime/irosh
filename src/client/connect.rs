use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use russh::client;

use crate::SessionState;
use crate::client::{Session, handler::ClientHandler};
use crate::config::{HostKeyPolicy, SecurityConfig, StateConfig};
use crate::error::{ClientError, IroshError, Result};
use crate::storage::keys::load_or_generate_identity;
use crate::storage::trust::load_known_server;
use crate::transport::iroh::{bind_client_endpoint, derive_alpn};
use crate::transport::metadata::{read_metadata, write_metadata_request};
use crate::transport::stream::IrohDuplex;
use crate::transport::ticket::Ticket;

/// Configuration options for the irosh client.
#[derive(Clone, Debug)]
pub struct ClientOptions {
    state: StateConfig,
    security: SecurityConfig,
    secret: Option<String>,
}

impl ClientOptions {
    /// Creates a new client options set with a specific state directory.
    pub fn new(state: StateConfig) -> Self {
        Self {
            state,
            security: SecurityConfig::default(),
            secret: None,
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
#[derive(Debug)]
pub struct Client;

impl Client {
    const METADATA_OPEN_TIMEOUT: Duration = Duration::from_secs(5);
    const METADATA_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
    const DEFAULT_USER: &'static str = "irosh";

    /// Connects to a remote irosh peer using the provided connection ticket.
    ///
    /// This performs the full P2P connection, SSH handshake, and metadata
    /// synchronization.
    ///
    /// # Errors
    ///
    /// Returns an error if the P2P connection fails, SSH authentication is
    /// rejected, or the transport is interrupted.
    ///
    /// ```no_run
    /// # use irosh::{Client, ClientOptions, StateConfig, transport::ticket::Ticket};
    /// # use std::str::FromStr;
    /// # async fn example() -> irosh::error::Result<()> {
    /// let state = StateConfig::new("./state".into());
    /// let options = ClientOptions::new(state);
    /// # let ticket = irosh::transport::ticket::Ticket::from_str("...")?;
    /// let session = Client::connect(&options, ticket).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(options: &ClientOptions, ticket: Ticket) -> Result<Session> {
        let target_addr = ticket.to_addr();
        let identity = load_or_generate_identity(options.state()).await?;
        let client_key = identity.ssh_key;

        let node_id = target_addr.id.to_string();
        let known_server = if options.security_config().host_key_policy == HostKeyPolicy::AcceptAll
        {
            None
        } else {
            load_known_server(options.state(), &node_id)?
        };

        let alpn = derive_alpn(options.secret_value());
        let endpoint = bind_client_endpoint(identity.secret_key, alpn.clone()).await?;

        let connection: iroh::endpoint::Connection =
            match endpoint.connect(target_addr, &alpn).await {
                Ok(connection) => connection,
                Err(err) => {
                    endpoint.close().await;
                    return Err(ClientError::ConnectFailed { source: err }.into());
                }
            };
        let mut state = SessionState::TransportConnected;

        let (send, recv): (iroh::endpoint::SendStream, iroh::endpoint::RecvStream) =
            match connection.open_bi().await {
                Ok(streams) => streams,
                Err(err) => {
                    endpoint.close().await;
                    return Err(ClientError::StreamOpenFailed { source: err }.into());
                }
            };

        let stream = IrohDuplex::new(send, recv);
        let config = Arc::new(client::Config::default());
        let last_disconnect = Arc::new(StdMutex::new(None));
        let handler = ClientHandler::new(
            node_id,
            known_server,
            last_disconnect.clone(),
            options.security_config(),
            options.state().clone(),
        );

        let session_result = async {
            state = SessionState::SshHandshaking;
            let mut handle = client::connect_stream(config, stream, handler)
                .await
                .map_err(|e| {
                    let detail = lock_or_recover(&last_disconnect).clone();
                    match (e, detail) {
                        (IroshError::Russh(russh::Error::Disconnect), detail) => {
                            IroshError::Client(ClientError::SshHandshakeDisconnected { detail })
                        }
                        (IroshError::Russh(russh_err), _) => {
                            IroshError::Client(ClientError::SshNegotiationFailed {
                                source: russh_err,
                            })
                        }
                        (other, _) => other,
                    }
                })?;

            let auth_res = handle
                .authenticate_publickey(
                    Self::DEFAULT_USER,
                    russh::keys::PrivateKeyWithHashAlg::new(Arc::new(client_key), None),
                )
                .await
                .map_err(|e| ClientError::SshNegotiationFailed { source: e })?;

            if !matches!(auth_res, client::AuthResult::Success) {
                return Err(IroshError::AuthenticationFailed);
            }

            state = SessionState::Authenticated;
            Ok(Arc::new(handle))
        }
        .await;

        match session_result {
            Ok(handle) => {
                let remote_metadata =
                    match tokio::time::timeout(Self::METADATA_OPEN_TIMEOUT, async {
                        let (send, recv) = connection
                            .open_bi()
                            .await
                            .map_err(|e| ClientError::StreamOpenFailed { source: e })?;
                        let mut stream = IrohDuplex::new(send, recv);

                        let metadata_res =
                            tokio::time::timeout(Self::METADATA_REQUEST_TIMEOUT, async {
                                write_metadata_request(&mut stream).await?;
                                read_metadata(&mut stream).await
                            })
                            .await;

                        match metadata_res {
                            Ok(Ok(metadata)) => Ok(metadata),
                            Ok(Err(e)) => Err(ClientError::MetadataFailed {
                                detail: e.to_string(),
                            }),
                            Err(_) => Err(ClientError::MetadataFailed {
                                detail: "timeout".to_string(),
                            }),
                        }
                    })
                    .await
                    {
                        Ok(Ok(metadata)) => Some(metadata),
                        _ => None,
                    };

                Ok(Session {
                    handle,
                    channel: None,
                    connection: Some(connection),
                    endpoint: Some(endpoint),
                    remote_metadata,
                    state,
                })
            }
            Err(e) => {
                connection.close(0u32.into(), b"SSH handshake failed");
                endpoint.close().await;
                Err(e)
            }
        }
    }

    /// Parses a connection target (ticket or peer alias) into a ticket.
    ///
    /// # Errors
    ///
    /// Returns an error if the target is unparseable or a requested alias is not found.
    #[cfg(feature = "storage")]
    pub fn parse_target(state: &StateConfig, target: &str) -> Result<Ticket> {
        use std::str::FromStr;

        if let Ok(ticket) = Ticket::from_str(target) {
            return Ok(ticket);
        }

        let peers = crate::storage::peers::list_peers(state)?;
        if let Some(peer) = peers.into_iter().find(|p| p.name == target) {
            return Ok(peer.ticket);
        }

        Err(IroshError::InvalidTarget {
            raw: target.to_string(),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn classify_connect_error(error: &IroshError) -> SessionState {
        SessionState::from_irosh_error(error)
    }
}

fn lock_or_recover<T>(mutex: &Arc<StdMutex<T>>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

use std::sync::MutexGuard;

impl SessionState {
    #[allow(dead_code)]
    pub(crate) fn from_irosh_error(error: &IroshError) -> Self {
        match error {
            IroshError::AuthenticationFailed => SessionState::AuthRejected,
            IroshError::ServerKeyMismatch { .. } => SessionState::TrustMismatch,
            _ => SessionState::Closed,
        }
    }
}

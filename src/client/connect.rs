use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use russh::client;

use crate::client::{Session, handler::ClientHandler};
use crate::config::{HostKeyPolicy, SecurityConfig, StateConfig};
use crate::error::{ClientError, IroshError, Result};
use crate::session::SessionState;
use crate::storage::load_or_generate_identity;
use crate::storage::peers::list_peers;
use crate::storage::trust::load_known_server;
use crate::transport::iroh::{bind_client_endpoint, derive_alpn};
use crate::transport::metadata::{read_metadata, write_metadata_request};
use crate::transport::stream::IrohDuplex;
use crate::transport::ticket::Ticket;

fn lock_or_recover<T>(mutex: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!("client session state mutex poisoned; recovering inner state");
            poisoned.into_inner()
        }
    }
}

/// Builder-style configuration for [`Client::connect`].
///
/// A `ClientOptions` value carries the local state directory, host-key policy,
/// and optional shared secret used to derive the transport ALPN.
///
/// # Example
///
/// ```no_run
/// # use std::error::Error;
/// use irosh::{ClientOptions, SecurityConfig, StateConfig};
///
/// # fn main() -> Result<(), Box<dyn Error>> {
/// let options = ClientOptions::new(StateConfig::new("/tmp/irosh-client".into()))
///     .security(SecurityConfig::default())
///     .secret("shared-secret");
/// # let _ = options;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientOptions {
    state: StateConfig,
    security: SecurityConfig,
    secret: Option<String>,
}

impl ClientOptions {
    /// Creates client options anchored at a concrete state directory.
    pub fn new(state: StateConfig) -> Self {
        Self {
            state,
            security: SecurityConfig::default(),
            secret: None,
        }
    }

    /// Replaces the client security policy.
    pub fn security(mut self, security: SecurityConfig) -> Self {
        self.security = security;
        self
    }

    /// Sets the optional shared secret used to derive the transport ALPN.
    pub fn secret(mut self, secret: impl Into<String>) -> Self {
        self.secret = Some(secret.into());
        self
    }

    /// Returns the state directory used by this client configuration.
    pub fn state(&self) -> &StateConfig {
        &self.state
    }

    /// Returns the active security policy.
    pub fn security_config(&self) -> SecurityConfig {
        self.security
    }

    /// Returns the optional shared secret if one was configured.
    pub fn secret_value(&self) -> Option<&str> {
        self.secret.as_deref()
    }
}

/// Entry point for client-side connection setup.
///
/// `Client` is a zero-sized facade that resolves tickets and saved peers, then
/// establishes an SSH-over-Iroh session yielding a [`Session`].
#[derive(Debug)]
pub struct Client;

impl Client {
    const METADATA_OPEN_TIMEOUT: Duration = Duration::from_secs(2);
    const METADATA_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
    const METADATA_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
    const DEFAULT_USER: &str = "demo";

    /// Resolves either a saved peer alias or a raw ticket string into a [`Ticket`].
    ///
    /// Saved peers are looked up in the storage layer first. If no saved peer
    /// matches `raw`, the value is parsed as a literal ticket string.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage layer cannot be read or if `raw` is
    /// neither a saved peer alias nor a valid ticket string.
    pub fn parse_target(state: &StateConfig, raw: &str) -> Result<Ticket> {
        if let Some(peer) = list_peers(state)?.into_iter().find(|p| p.name == raw) {
            return Ok(peer.ticket);
        }
        raw.parse()
            .map_err(|_| crate::error::TransportError::TicketFormatInvalid.into())
    }

    /// Connects to a remote endpoint and returns an authenticated [`Session`].
    ///
    /// This method performs the full client-side setup flow:
    ///
    /// 1. loads or creates local identity material
    /// 2. applies the configured host-key policy
    /// 3. establishes the Iroh transport connection
    /// 4. negotiates SSH and authenticates
    /// 5. opens the primary session channel
    /// 6. attempts best-effort metadata retrieval on a side stream
    ///
    /// # Errors
    ///
    /// Returns an error if local identity state cannot be loaded, the target
    /// cannot be reached over Iroh, SSH negotiation or authentication fails, or
    /// the primary SSH session channel cannot be opened.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use std::error::Error;
    /// # use irosh::{Client, ClientOptions, StateConfig, Ticket};
    /// # async fn example() -> Result<(), Box<dyn Error>> {
    /// # let options = ClientOptions::new(StateConfig::new("/tmp/irosh-client".into()));
    /// # let target: Ticket = "endpoint...".parse()?;
    /// let session = Client::connect(&options, target).await?;
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

            let channel = handle
                .channel_open_session()
                .await
                .map_err(|e| ClientError::ChannelOpenFailed { source: e })?;

            state = SessionState::Authenticated;
            Ok((handle, channel))
        }
        .await;

        match session_result {
            Ok((handle, channel)) => {
                let remote_metadata =
                    match tokio::time::timeout(Self::METADATA_OPEN_TIMEOUT, async {
                        let (send, recv) = connection
                            .open_bi()
                            .await
                            .map_err(|e| ClientError::StreamOpenFailed { source: e })?;
                        let mut stream = IrohDuplex::new(send, recv);
                        tokio::time::timeout(Self::METADATA_REQUEST_TIMEOUT, async {
                            write_metadata_request(&mut stream).await
                        })
                        .await
                        .map_err(|_| ClientError::MetadataFailed {
                            detail: "request timed out".to_string(),
                        })?
                        .map_err(|e| ClientError::MetadataFailed {
                            detail: e.to_string(),
                        })?;

                        tokio::time::timeout(Self::METADATA_RESPONSE_TIMEOUT, async {
                            read_metadata(&mut stream).await
                        })
                        .await
                        .map_err(|_| ClientError::MetadataFailed {
                            detail: "response timed out".to_string(),
                        })?
                        .map_err(|e| ClientError::MetadataFailed {
                            detail: e.to_string(),
                        })
                    })
                    .await
                    {
                        Ok(Ok(metadata)) => Some(metadata),
                        _ => None,
                    };

                Ok(Session {
                    handle,
                    channel,
                    connection: Some(connection),
                    endpoint: Some(endpoint),
                    remote_metadata,
                    state,
                })
            }
            Err(err) => {
                endpoint.close().await;
                Err(err)
            }
        }
    }

    /// Maps a connection failure into the terminal [`SessionState`] it represents.
    pub fn classify_connect_error(error: &IroshError) -> SessionState {
        match error {
            IroshError::AuthenticationFailed => SessionState::AuthRejected,
            IroshError::ServerKeyMismatch { .. } => SessionState::TrustMismatch,
            _ => SessionState::Closed,
        }
    }
}

//! SSH Client orchestration, connections, and interactive shells.

mod connect;
pub mod handler;
#[cfg(test)]
mod tests;
mod transfer;

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use russh::ChannelMsg;
use russh::client;

pub use self::connect::{Client, ClientOptions};
pub use crate::SessionState;
pub use crate::session::pty::PtyOptions;

use crate::error::{ClientError, Result};
use crate::session::pty::PtySize;

/// Progress state for an ongoing file transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferProgress {
    /// Bytes successfully transferred so far.
    pub transferred: u64,
    /// Total expected size in bytes.
    pub total: u64,
}

impl TransferProgress {
    pub(crate) fn new(transferred: u64, total: u64) -> Self {
        Self { transferred, total }
    }

    /// Returns the completion percentage clamped to `0..=100`.
    pub fn percent(&self) -> u8 {
        self.transferred
            .saturating_mul(100)
            .checked_div(self.total)
            .unwrap_or(100)
            .min(100) as u8
    }
}

/// A high-level SSH session over Iroh transport.
pub struct Session {
    pub(crate) handle: Arc<tokio::sync::RwLock<client::Handle<handler::ClientHandler>>>,
    pub(super) handler: handler::ClientHandler,
    channel: Option<russh::Channel<russh::client::Msg>>,
    connection: Option<iroh::endpoint::Connection>,
    endpoint: Option<iroh::Endpoint>,
    remote_metadata: Option<crate::transport::metadata::PeerMetadata>,
    state: SessionState,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("state", &self.state)
            .field("has_metadata", &self.remote_metadata.is_some())
            .finish()
    }
}

/// Represents the output of a remote command execution.
#[derive(Debug, Clone, Default)]
pub struct ExecOutput {
    /// The captured stdout bytes.
    pub stdout: Vec<u8>,
    /// The captured stderr bytes.
    pub stderr: Vec<u8>,
    /// The remote process exit status.
    pub exit_status: u32,
}

impl Session {
    /// Returns the current library-owned lifecycle state for this session.
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Returns remote metadata if it was obtained during session setup.
    pub fn remote_metadata(&self) -> Option<&crate::transport::metadata::PeerMetadata> {
        self.remote_metadata.as_ref()
    }

    /// Requests a PTY for the active session channel.
    ///
    /// This is typically called before [`Session::start_shell`] for an
    /// interactive terminal session.
    /// # Errors
    ///
    /// Returns an error if the request cannot be sent or the remote SSH server rejects
    /// the PTY request.
    pub async fn request_pty(&mut self, options: PtyOptions) -> Result<()> {
        let size = options.size();
        let channel = self.ensure_channel().await?;
        channel
            .request_pty(
                true,
                options.term(),
                size.cols as u32,
                size.rows as u32,
                size.pixel_width as u32,
                size.pixel_height as u32,
                options.modes_slice(),
            )
            .await
            .map_err(|e| ClientError::PtyRequestFailed { source: e })?;
        Ok(())
    }

    /// Transitions the session to a live interactive shell.
    ///
    /// # Errors
    ///
    /// Returns an error if the shell request cannot be sent or is rejected by
    /// the remote peer.
    pub async fn start_shell(&mut self) -> Result<()> {
        let channel = self.ensure_channel().await?;
        channel
            .request_shell(true)
            .await
            .map_err(|e| ClientError::ShellRequestFailed { source: e })?;
        self.state = SessionState::ShellReady;
        Ok(())
    }

    /// Requests execution of a single remote command.
    ///
    /// # Errors
    ///
    /// Returns an error if the command request cannot be sent or is rejected by
    /// the remote side.
    pub async fn exec(&mut self, command: &str) -> Result<()> {
        let channel = self.ensure_channel().await?;
        channel
            .exec(true, command)
            .await
            .map_err(|e| ClientError::ExecFailed { source: e })?;
        self.state = SessionState::ShellReady;
        Ok(())
    }

    /// Ensures that the primary session channel is open, opening it if necessary.
    pub(crate) async fn ensure_channel(
        &mut self,
    ) -> Result<&mut russh::Channel<russh::client::Msg>> {
        if self.channel.is_none() {
            let handle = self.handle.read().await;
            let channel = handle
                .channel_open_session()
                .await
                .map_err(|e| ClientError::ChannelOpenFailed { source: e })?;
            self.channel = Some(channel);
        }
        self.channel.as_mut().ok_or_else(|| {
            ClientError::ChannelOpenFailed {
                source: russh::Error::ChannelOpenFailure(russh::ChannelOpenFailure::ConnectFailed),
            }
            .into()
        })
    }

    /// Requests execution of a single remote command and captures its output.
    ///
    /// This method will block until the command completes or the session is closed.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to start or the session is lost.
    pub async fn capture_exec(&mut self, command: &str) -> Result<ExecOutput> {
        // This is standard SSH behavior and avoids conflicting with the shell channel.
        let handle = self.handle.read().await;
        let mut channel = handle
            .channel_open_session()
            .await
            .map_err(|e| ClientError::ChannelOpenFailed { source: e })?;
        channel
            .exec(true, command)
            .await
            .map_err(|e| ClientError::ExecFailed { source: e })?;

        let mut output = ExecOutput::default();
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    output.stdout.extend_from_slice(&data);
                }
                Some(ChannelMsg::ExtendedData { data, ext: 1 }) => {
                    output.stderr.extend_from_slice(&data);
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    output.exit_status = exit_status;
                }
                Some(ChannelMsg::Close) | None => break,
                _ => {}
            }
        }
        Ok(output)
    }

    /// Initiates a local port forwarding tunnel.
    ///
    /// This will bind to `local_addr` and forward all incoming connections to `remote_host:remote_port`
    /// via the remote SSH peer.
    ///
    /// This method returns a [`tokio::task::JoinHandle`] for the forwarding task and the actually bound [`SocketAddr`].
    /// The task will run until the listener is closed or the session is lost.
    ///
    /// # Errors
    ///
    /// Returns an error if the local listener cannot be bound.
    pub async fn local_forward(
        &self,
        local_addr: impl tokio::net::ToSocketAddrs,
        remote_host: String,
        remote_port: u32,
    ) -> Result<(tokio::task::JoinHandle<()>, SocketAddr)> {
        let listener = tokio::net::TcpListener::bind(local_addr)
            .await
            .map_err(|e| ClientError::TunnelFailed {
                details: format!("failed to bind local listener: {}", e),
            })?;

        let bound_addr = listener
            .local_addr()
            .map_err(|e| ClientError::TunnelFailed {
                details: format!("failed to resolve bound local address: {}", e),
            })?;

        let handle = self.handle.clone();

        let join_handle = tokio::spawn(async move {
            tracing::info!(
                "Local port forwarding active on {:?}",
                listener.local_addr()
            );
            loop {
                let Ok((stream, addr)) = listener.accept().await else {
                    break;
                };
                tracing::debug!("Accepted local connection for tunnel from {:?}", addr);

                let handle = handle.clone();
                let remote_host = remote_host.clone();

                tokio::spawn(async move {
                    let handle = handle.read().await;
                    let channel = match handle
                        .channel_open_direct_tcpip(
                            &remote_host,
                            remote_port,
                            &addr.ip().to_string(),
                            addr.port() as u32,
                        )
                        .await
                    {
                        Ok(c) => c,
                        Err(err) => {
                            tracing::warn!(
                                "Failed to open direct-tcpip channel for {}: {}: {}",
                                remote_host,
                                remote_port,
                                err
                            );
                            return;
                        }
                    };

                    let (mut reader, mut writer) = tokio::io::split(stream);
                    let (mut channel_reader, mut channel_writer) =
                        tokio::io::split(channel.into_stream());

                    let _ = tokio::select! {
                        res = tokio::io::copy(&mut reader, &mut channel_writer) => res,
                        res = tokio::io::copy(&mut channel_reader, &mut writer) => res,
                    };
                });
            }
        });

        Ok((join_handle, bound_addr))
    }

    /// Sends raw input bytes to the remote session.
    ///
    /// # Errors
    ///
    /// Returns an error if the SSH channel is closed or cannot accept more data.
    pub async fn send(&mut self, data: &[u8]) -> Result<()> {
        let channel = self.ensure_channel().await?;
        channel
            .data(data)
            .await
            .map_err(|e| ClientError::DataSendFailed { source: e }.into())
    }

    /// Signals EOF to the remote session.
    ///
    /// # Errors
    ///
    /// Returns an error if the EOF signal cannot be sent on the current session channel.
    pub async fn eof(&mut self) -> Result<()> {
        let channel = self.ensure_channel().await?;
        channel
            .eof()
            .await
            .map_err(|e| ClientError::EofSendFailed { source: e }.into())
    }

    /// Resizes the remote PTY if one is active.
    ///
    /// # Errors
    ///
    /// Returns an error if the resize request cannot be sent or the remote side
    /// no longer accepts PTY changes.
    pub async fn resize(&mut self, size: PtySize) -> Result<()> {
        let channel = self.ensure_channel().await?;
        channel
            .window_change(
                size.cols as u32,
                size.rows as u32,
                size.pixel_width as u32,
                size.pixel_height as u32,
            )
            .await
            .map_err(|e| ClientError::WindowChangeFailed { source: e }.into())
    }

    /// Waits for the next session event from the remote peer.
    ///
    /// This returns `None` if the session was closed gracefully.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying transport or SSH channel fails.
    pub async fn next_event(&mut self) -> Result<Option<SessionEvent>> {
        let Some(channel) = self.channel.as_mut() else {
            return Ok(None);
        };
        match channel.wait().await {
            Some(msg) => {
                tracing::debug!("Received low-level SSH message: {:?}", msg);
                Ok(Some(SessionEvent::from(msg)))
            }
            None => {
                tracing::debug!("Low-level SSH event stream ended (None)");
                self.state = SessionState::Closed;
                Ok(None)
            }
        }
    }

    /// Disconnects the session and closes all underlying transport streams.
    ///
    /// # Errors
    ///
    /// Returns an error if the disconnect signal cannot be sent.
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(channel) = self.channel.take() {
            let _ = channel.close().await;
        }
        let handle = self.handle.read().await;
        handle
            .disconnect(russh::Disconnect::ByApplication, "", "en-US")
            .await
            .map_err(|e| ClientError::DisconnectFailed { source: e })?;

        // Explicitly close Iroh resources to avoid ungraceful drop panics.
        if let Some(conn) = self.connection.take() {
            conn.close(0u32.into(), b"Session disconnected");
        }
        if let Some(endpoint) = self.endpoint.take() {
            endpoint.close().await;
        }

        self.state = SessionState::Closed;
        Ok(())
    }

    /// Requests the remote server to forward a port back to a local address.
    ///
    /// This corresponds to the `-R` flag in standard SSH.
    ///
    /// # Errors
    ///
    /// Returns an error if the request is rejected by the server.
    pub async fn remote_forward(
        &self,
        remote_host: String,
        remote_port: u32,
        local_host: String,
        local_port: u16,
    ) -> Result<()> {
        let mut handle = self.handle.write().await;
        handle
            .tcpip_forward(remote_host.clone(), remote_port)
            .await
            .map_err(|e| ClientError::TunnelFailed {
                details: format!("server rejected remote forward request: {}", e),
            })?;

        // Register the tunnel in the handler so we know where to route it
        // when the server opens a channel back to us.
        self.handler
            .register_remote_tunnel(remote_host, remote_port, local_host, local_port);

        Ok(())
    }

    /// Requests tab completion matches from the remote server for the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or the server rejects the request.
    pub async fn remote_completion(&mut self, path: &str) -> Result<Vec<String>> {
        let mut stream = self.open_transfer_stream("completion unavailable").await?;

        crate::transport::transfer::write_completion_request(
            &mut stream,
            &crate::transport::transfer::CompletionRequest {
                path: path.to_string(),
            },
        )
        .await
        .map_err(crate::error::TransportError::from)?;

        match crate::transport::transfer::read_next_frame(&mut stream)
            .await
            .map_err(crate::error::TransportError::from)?
        {
            crate::transport::transfer::TransferFrame::CompletionResponse(res) => Ok(res.matches),
            crate::transport::transfer::TransferFrame::Error(failure) => {
                Err(ClientError::TransferRejected {
                    details: failure.to_string(),
                }
                .into())
            }
            other => Err(ClientError::DownloadFailed {
                details: format!("unexpected completion frame: {other:?}"),
            }
            .into()),
        }
    }

    /// Closes the session, consuming it.
    ///
    /// # Errors
    ///
    /// Returns any error produced by [`Session::disconnect`].
    pub async fn close(mut self) -> Result<()> {
        self.disconnect().await
    }
}

/// Events that can occur during an active SSH session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// Raw data received from remote stdout.
    Data(Vec<u8>),
    /// Raw data received from remote stderr or other extended streams.
    ExtendedData(Vec<u8>, u32),
    /// The remote process has exited with the given status code.
    ExitStatus(u32),
    /// The remote process was terminated by a signal.
    ExitSignal {
        /// Signal name (e.g. "TERM", "KILL").
        signal: String,
        /// Whether a core dump was generated.
        core_dumped: bool,
        /// Human-readable error message.
        error_message: String,
        /// Language tag for the error message.
        lang_tag: String,
    },
    /// The remote session has been closed.
    Closed,
    /// An internal SSH message that the library doesn't need to surface.
    Ignore,
}

impl From<ChannelMsg> for SessionEvent {
    fn from(msg: ChannelMsg) -> Self {
        match msg {
            ChannelMsg::Data { data } => Self::Data(data.to_vec()),
            ChannelMsg::ExtendedData { data, ext } => Self::ExtendedData(data.to_vec(), ext),
            ChannelMsg::ExitStatus { exit_status } => Self::ExitStatus(exit_status),
            ChannelMsg::ExitSignal {
                signal_name,
                core_dumped,
                error_message,
                lang_tag,
            } => Self::ExitSignal {
                signal: format!("{:?}", signal_name),
                core_dumped,
                error_message: error_message.to_string(),
                lang_tag: lang_tag.to_string(),
            },
            ChannelMsg::Eof => Self::Ignore,
            ChannelMsg::Close => Self::Closed,
            _ => Self::Ignore,
        }
    }
}

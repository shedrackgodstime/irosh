//! High-level [`Session`] API: interactive shell, exec, tunneling, and teardown.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use russh::{ChannelMsg, client};

use crate::client::handler;
use crate::error::{ClientError, Result};
use crate::session::SessionState;
use crate::session::pty::{PtyOptions, PtySize};
use crate::transport::metadata::PeerMetadata;

use super::session_event::SessionEvent;

/// A high-level SSH session over Iroh transport.
pub struct Session {
    pub(crate) handle: Arc<tokio::sync::RwLock<client::Handle<handler::ClientHandler>>>,
    pub(super) handler: handler::ClientHandler,
    pub(super) channel: tokio::sync::Mutex<Option<russh::Channel<russh::client::Msg>>>,
    pub(super) connection: Option<iroh::endpoint::Connection>,
    pub(super) blobs_connection: Option<iroh::endpoint::Connection>,
    pub(super) endpoint: Option<iroh::Endpoint>,
    pub(super) remote_metadata: Option<PeerMetadata>,
    pub(super) state: SessionState,
    pub(crate) blobs: iroh_blobs::store::fs::FsStore,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("state", &self.state)
            .field("has_metadata", &self.remote_metadata.is_some())
            .field("has_connection", &self.connection.is_some())
            .field("has_blobs_connection", &self.blobs_connection.is_some())
            .field("has_endpoint", &self.endpoint.is_some())
            .finish_non_exhaustive()
    }
}

/// Represents the output of a remote command execution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
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

    /// Returns whether all iroh transport resources (channel, connections,
    /// endpoint) have been released after [`Session::disconnect`] or
    /// [`Session::close`].
    ///
    /// Used by integration tests to assert that a clean session end does not
    /// leave the endpoint open until drop.
    #[doc(hidden)]
    #[must_use]
    pub fn transport_resources_released(&self) -> bool {
        let channel_released = match self.channel.try_lock() {
            Ok(guard) => guard.is_none(),
            Err(_) => false,
        };
        channel_released
            && self.connection.is_none()
            && self.blobs_connection.is_none()
            && self.endpoint.is_none()
    }

    /// Returns remote metadata if it was obtained during session setup.
    pub fn remote_metadata(&self) -> Option<&PeerMetadata> {
        self.remote_metadata.as_ref()
    }

    fn check_open(&self) -> Result<()> {
        if self.state.is_terminal() {
            let details = match self.state {
                SessionState::AuthRejected => "session is rejected",
                SessionState::TrustMismatch => "session is trust mismatch",
                SessionState::Closed => "session is closed",
                _ => "session is unavailable",
            };
            return Err(ClientError::TransportUnavailable { details }.into());
        }
        Ok(())
    }

    /// Requests a PTY for the active session channel.
    ///
    /// This is typically called before [`Session::start_shell`] for an
    /// interactive terminal session.
    /// # Errors
    ///
    /// Returns an error if the request cannot be sent or the remote SSH server rejects
    /// the PTY request.
    #[must_use]
    #[tracing::instrument(skip(self))]
    pub async fn request_pty(&self, options: PtyOptions) -> Result<()> {
        self.check_open()?;
        let guard = self.ensure_channel().await?;
        let Some(ref channel) = *guard else {
            return Err(ClientError::TransportUnavailable {
                details: "no channel available",
            }
            .into());
        };
        let size = options.size();
        channel
            .request_pty(
                false,
                options.term(),
                u32::from(size.cols),
                u32::from(size.rows),
                u32::from(size.pixel_width),
                u32::from(size.pixel_height),
                options.modes_slice(),
            )
            .await
            .map_err(|e| ClientError::PtyRequestFailed { source: e }.into())
    }

    /// Starts an interactive shell session.
    ///
    /// # Errors
    ///
    /// Returns an error if the remote server rejects the shell request or the session
    /// is disconnected.
    #[must_use]
    #[tracing::instrument(skip(self))]
    pub async fn start_shell(&mut self) -> Result<()> {
        self.check_open()?;
        {
            let mut guard = self.ensure_channel().await?;
            let channel = guard
                .as_mut()
                .ok_or_else(|| ClientError::ChannelOpenFailed {
                    source: russh::Error::ChannelOpenFailure(
                        russh::ChannelOpenFailure::ConnectFailed,
                    ),
                })?;
            channel
                .request_shell(true)
                .await
                .map_err(|e| ClientError::ShellRequestFailed { source: e })?;
        }
        self.state = SessionState::ShellReady;
        Ok(())
    }

    /// Executes a single command on the remote server via the shell channel.
    ///
    /// # Errors
    ///
    /// Returns an error if the remote server rejects the exec request or the session
    /// is disconnected.
    #[must_use]
    #[tracing::instrument(skip(self))]
    pub async fn exec(&mut self, command: &str) -> Result<()> {
        self.check_open()?;
        let mut guard = self.ensure_channel().await?;
        let channel = guard
            .as_mut()
            .ok_or_else(|| ClientError::ChannelOpenFailed {
                source: russh::Error::ChannelOpenFailure(russh::ChannelOpenFailure::ConnectFailed),
            })?;
        channel
            .exec(true, command)
            .await
            .map_err(|e| ClientError::ExecFailed { source: e }.into())
    }

    /// Returns remote metadata if it was obtained during session setup.
    /// Ensures that the primary session channel is open, opening it if necessary.
    #[must_use]
    pub(crate) async fn ensure_channel(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<russh::Channel<russh::client::Msg>>>> {
        // Fast path: check if channel exists (short lock hold)
        {
            let guard = self.channel.lock().await;
            if guard.is_some() {
                return Ok(guard);
            }
        }

        // Slow path: open a new channel WITHOUT holding the lock.
        // This avoids blocking other callers during the network round-trip.
        let handle = self.handle.read().await;
        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| ClientError::ChannelOpenFailed { source: e })?;

        // Re-acquire lock and install if still None (CAS loop).
        let mut guard = self.channel.lock().await;
        if guard.is_none() {
            *guard = Some(channel);
        }
        Ok(guard)
    }

    /// Requests execution of a single remote command and captures its output.
    ///
    /// This method will block until the command completes or the session is closed.
    ///
    /// Note: This opens a **secondary** SSH channel to avoid conflicting with
    /// the primary shell channel (used by [`start_shell`](Self::start_shell)).
    /// The session state is not modified and continues to reflect the primary
    /// channel.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to start or the session is lost.
    #[must_use]
    #[tracing::instrument(skip(self))]
    pub async fn capture_exec(&mut self, command: &str) -> Result<ExecOutput> {
        self.check_open()?;
        // Secondary channel — does not go through `ensure_channel`.
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
    #[must_use]
    pub async fn local_forward(
        &self,
        local_addr: impl tokio::net::ToSocketAddrs,
        remote_host: String,
        remote_port: u32,
    ) -> Result<(tokio::task::JoinHandle<()>, SocketAddr)> {
        self.check_open()?;
        let listener = tokio::net::TcpListener::bind(local_addr)
            .await
            .map_err(|e| ClientError::TunnelFailed {
                details: format!("failed to bind local listener: {e}"),
            })?;

        let bound_addr = listener
            .local_addr()
            .map_err(|e| ClientError::TunnelFailed {
                details: format!("failed to resolve bound local address: {e}"),
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
                            u32::from(addr.port()),
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
    #[must_use]
    #[tracing::instrument(skip(self))]
    pub async fn send(&self, data: &[u8]) -> Result<()> {
        self.check_open()?;
        let mut guard = self.ensure_channel().await?;
        let channel = guard.as_mut().ok_or_else(|| ClientError::DataSendFailed {
            source: russh::Error::ChannelOpenFailure(russh::ChannelOpenFailure::ConnectFailed),
        })?;
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
    #[must_use]
    #[tracing::instrument(skip(self))]
    pub async fn eof(&self) -> Result<()> {
        self.check_open()?;
        let mut guard = self.ensure_channel().await?;
        let channel = guard.as_mut().ok_or_else(|| ClientError::EofSendFailed {
            source: russh::Error::ChannelOpenFailure(russh::ChannelOpenFailure::ConnectFailed),
        })?;
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
    #[must_use]
    #[tracing::instrument(skip(self))]
    pub async fn resize(&self, size: PtySize) -> Result<()> {
        self.check_open()?;
        let mut guard = self.ensure_channel().await?;
        let channel = guard
            .as_mut()
            .ok_or_else(|| ClientError::WindowChangeFailed {
                source: russh::Error::ChannelOpenFailure(russh::ChannelOpenFailure::ConnectFailed),
            })?;
        channel
            .window_change(
                u32::from(size.cols),
                u32::from(size.rows),
                u32::from(size.pixel_width),
                u32::from(size.pixel_height),
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
    #[must_use]
    pub async fn next_event(&mut self) -> Result<Option<SessionEvent>> {
        let mut guard = self.channel.lock().await;
        let Some(channel) = guard.as_mut() else {
            return Ok(None);
        };
        if let Some(msg) = channel.wait().await {
            tracing::debug!("received SSH channel message");
            Ok(Some(SessionEvent::from(msg)))
        } else {
            tracing::debug!("Low-level SSH event stream ended (None)");
            self.state = SessionState::Closed;
            Ok(None)
        }
    }

    /// Disconnects the session and closes all underlying transport streams.
    ///
    /// Tear down happens regardless of the current session state: when the
    /// remote stream ends, [`Session::next_event`] marks the session `Closed`
    /// immediately, and iroh resources must still be released so the endpoint
    /// is not dropped unclosed (which makes iroh log "Endpoint dropped without
    /// calling `Endpoint::close`" at error level). Every step is idempotent, so
    /// calling this more than once is safe.
    ///
    /// # Errors
    ///
    /// Returns an error if the SSH disconnect signal cannot be sent while the
    /// session is still open.
    #[must_use]
    #[tracing::instrument(skip(self))]
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(channel) = self.channel.lock().await.take() {
            let _ = channel.close().await;
        }

        if !self.state.is_terminal() {
            let handle = self.handle.read().await;
            handle
                .disconnect(russh::Disconnect::ByApplication, "", "en-US")
                .await
                .map_err(|e| ClientError::DisconnectFailed { source: e })?;
        }

        // Explicitly close Iroh resources to avoid ungraceful drop panics.
        if let Some(conn) = self.connection.take() {
            conn.close(0u32.into(), b"Session disconnected");
        }
        if let Some(conn) = self.blobs_connection.take() {
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
    #[must_use]
    pub async fn remote_forward(
        &self,
        remote_host: String,
        remote_port: u32,
        local_host: String,
        local_port: u16,
    ) -> Result<()> {
        self.check_open()?;
        let handle = self.handle.write().await;
        handle
            .tcpip_forward(remote_host.clone(), remote_port)
            .await
            .map_err(|e| ClientError::TunnelFailed {
                details: format!("server rejected remote forward request: {e}"),
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
    #[must_use]
    pub async fn remote_completion(&mut self, path: &str) -> Result<Vec<String>> {
        self.check_open()?;
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
                Err(ClientError::TransferRejected { failure }.into())
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
    #[must_use]
    #[tracing::instrument(skip(self))]
    pub async fn close(mut self) -> Result<()> {
        self.disconnect().await
    }
}

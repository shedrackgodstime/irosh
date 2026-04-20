//! SSH Client orchestration, connections, and interactive shells.

mod connect;
pub mod handler;
#[cfg(test)]
mod tests;
mod transfer;

use std::fmt;

use russh::ChannelMsg;
use russh::client;

pub use self::connect::{Client, ClientOptions};

use crate::error::{ClientError, Result};
use crate::session::{PtyOptions, PtySize, SessionState};

/// An active, authenticated irosh session.
///
/// `Session` is the main runtime handle returned by [`Client::connect`]. It
/// owns the SSH session channel together with the underlying Iroh connection
/// and endpoint state required for the lifetime of that session.
///
/// Callers drive the remote session explicitly through methods like
/// [`Session::request_pty`], [`Session::start_shell`], [`Session::exec`],
/// [`Session::send`], and [`Session::next_event`].
///
/// Cleanup should preferably happen through [`Session::disconnect`] or
/// [`Session::close`] instead of relying on drop.
///
/// # Warning
///
/// Dropping a `Session` while it still owns an active `iroh::Endpoint` may
/// trigger a synchronous close of the underlying Iroh transport, which can
/// block the async runtime depending on the driver. Always prefer explicit
/// shutdown.
pub struct Session {
    pub(crate) handle: client::Handle<handler::ClientHandler>,
    pub(crate) channel: russh::Channel<client::Msg>,
    pub(crate) connection: Option<iroh::endpoint::Connection>,
    pub(crate) endpoint: Option<iroh::Endpoint>,
    pub(crate) remote_metadata: Option<crate::transport::metadata::PeerMetadata>,
    pub(crate) state: SessionState,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("has_connection", &self.connection.is_some())
            .field("has_endpoint", &self.endpoint.is_some())
            .field("has_remote_metadata", &self.remote_metadata.is_some())
            .field("state", &self.state)
            .finish()
    }
}

/// Library-owned session events surfaced to callers.
///
/// These events are produced by [`Session::next_event`] and provide a stable
/// library surface instead of exposing raw `russh` channel messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// Standard output bytes produced by the remote side.
    Stdout(Vec<u8>),
    /// Standard error bytes produced by the remote side.
    Stderr(Vec<u8>),
    /// Process exit status reported by the remote side.
    ExitStatus(u32),
    /// Notification that the remote side closed the session channel.
    Closed,
}

/// Progress information for a file transfer.
///
/// This is a lightweight snapshot value used by the transfer APIs and CLI
/// integration code to report progress without exposing transport internals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferProgress {
    transferred: u64,
    total: u64,
}

impl TransferProgress {
    /// Creates a new transfer progress snapshot.
    pub fn new(transferred: u64, total: u64) -> Self {
        Self { transferred, total }
    }

    /// Returns the number of bytes transferred so far.
    pub fn transferred(&self) -> u64 {
        self.transferred
    }

    /// Returns the expected total number of bytes for the transfer.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Returns the completion percentage clamped to `0..=100`.
    pub fn percent(&self) -> u8 {
        if self.total == 0 {
            100
        } else {
            ((self.transferred.saturating_mul(100)) / self.total).min(100) as u8
        }
    }
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
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use std::error::Error;
    /// # use irosh::{Session, PtyOptions, session::default_pty_size};
    /// # async fn example(mut session: Session) -> Result<(), Box<dyn Error>> {
    /// session.request_pty(PtyOptions::new("xterm-256color", default_pty_size())).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn request_pty(&mut self, options: PtyOptions) -> Result<()> {
        let size = options.size();
        self.channel
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
            .map_err(|e| ClientError::PtyRequestFailed { source: e }.into())
    }

    /// Requests the default interactive shell and marks the session as shell-ready.
    ///
    /// # Errors
    ///
    /// Returns an error if the request cannot be sent or the remote SSH server rejects
    /// the shell request.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use std::error::Error;
    /// # use irosh::Session;
    /// # async fn example(mut session: Session) -> Result<(), Box<dyn Error>> {
    /// session.start_shell().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn start_shell(&mut self) -> Result<()> {
        self.channel
            .request_shell(true)
            .await
            .map_err(|e| ClientError::ShellRequestFailed { source: e })?;
        self.state = SessionState::ShellReady;
        Ok(())
    }

    /// Requests execution of a single remote command.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use std::error::Error;
    /// # use irosh::{Client, ClientOptions, StateConfig, Ticket};
    /// # async fn run() -> Result<(), Box<dyn Error>> {
    /// # let options = ClientOptions::new(StateConfig::new("/tmp/irosh-client".into()));
    /// # let target: Ticket = "endpoint...".parse()?;
    /// # let mut session = Client::connect(&options, target).await?;
    /// session.exec("uname -a").await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the command request cannot be sent or is rejected by
    /// the remote side.
    pub async fn exec(&mut self, command: &str) -> Result<()> {
        self.channel
            .exec(true, command)
            .await
            .map_err(|e| ClientError::ExecFailed { source: e })?;
        self.state = SessionState::ShellReady;
        Ok(())
    }

    /// Sends raw input bytes to the remote session.
    ///
    /// # Errors
    ///
    /// Returns an error if the SSH channel is closed or cannot accept more data.
    pub async fn send(&mut self, data: &[u8]) -> Result<()> {
        self.channel
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
        self.channel
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
        self.channel
            .window_change(
                size.cols as u32,
                size.rows as u32,
                size.pixel_width as u32,
                size.pixel_height as u32,
            )
            .await
            .map_err(|e| ClientError::WindowChangeFailed { source: e }.into())
    }

    /// Waits for the next SSH channel message from the remote side.
    ///
    /// This translates raw SSH channel traffic into library-owned
    /// [`SessionEvent`] values.
    ///
    /// # Errors
    ///
    /// Returns an error if waiting on the underlying SSH channel fails.
    pub async fn next_event(&mut self) -> Result<Option<SessionEvent>> {
        loop {
            let Some(message) = self.channel.wait().await else {
                self.state = SessionState::Closed;
                return Ok(None);
            };

            match message {
                ChannelMsg::Data { data } => return Ok(Some(SessionEvent::Stdout(data.to_vec()))),
                ChannelMsg::ExtendedData { data, .. } => {
                    return Ok(Some(SessionEvent::Stderr(data.to_vec())));
                }
                ChannelMsg::ExitStatus { exit_status } => {
                    return Ok(Some(SessionEvent::ExitStatus(exit_status)));
                }
                ChannelMsg::Eof | ChannelMsg::Close => {
                    self.state = SessionState::Closed;
                    return Ok(Some(SessionEvent::Closed));
                }
                _ => {}
            }
        }
    }

    /// Disconnects the SSH session and closes the underlying endpoint.
    ///
    /// Prefer calling this explicitly rather than relying on drop for cleanup.
    ///
    /// # Errors
    ///
    /// Returns an error if the SSH disconnect request cannot be sent.
    pub async fn disconnect(&mut self) -> Result<()> {
        self.handle
            .disconnect(russh::Disconnect::ByApplication, "", "English")
            .await
            .map_err(|e| ClientError::DisconnectFailed { source: e })?;
        if let Some(endpoint) = self.endpoint.as_ref() {
            endpoint.close().await;
        }
        self.connection = None;
        self.state = SessionState::Closed;
        Ok(())
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

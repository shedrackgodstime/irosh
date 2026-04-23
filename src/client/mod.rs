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
pub use crate::SessionState;
pub use crate::session::pty::PtyOptions;

use crate::error::{ClientError, Result};
use crate::session::pty::PtySize;

/// A high-level SSH session over Iroh transport.
pub struct Session {
    handle: client::Handle<handler::ClientHandler>,
    channel: russh::Channel<russh::client::Msg>,
    #[allow(dead_code)]
    connection: Option<iroh::endpoint::Connection>,
    #[allow(dead_code)]
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
        self.channel
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
        self.channel
            .exec(true, command)
            .await
            .map_err(|e| ClientError::ExecFailed { source: e })?;
        self.state = SessionState::ShellReady;
        Ok(())
    }

    /// Requests execution of a single remote command and captures its output.
    ///
    /// This method will block until the command completes or the session is closed.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails to start or the session is lost.
    pub async fn capture_exec(&mut self, command: &str) -> Result<ExecOutput> {
        // Open a NEW channel for every exec request.
        // This is standard SSH behavior and avoids conflicting with the shell channel.
        let mut channel = self
            .handle
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

    /// Waits for the next session event from the remote peer.
    ///
    /// This returns `None` if the session was closed gracefully.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying transport or SSH channel fails.
    pub async fn next_event(&mut self) -> Result<Option<SessionEvent>> {
        match self.channel.wait().await {
            Some(msg) => Ok(Some(SessionEvent::from(msg))),
            None => {
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
        let _ = self.channel.close().await;
        self.handle
            .disconnect(russh::Disconnect::ByApplication, "", "en-US")
            .await
            .map_err(|e| ClientError::DisconnectFailed { source: e })?;
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
            ChannelMsg::Close => Self::Closed,
            _ => Self::Closed,
        }
    }
}

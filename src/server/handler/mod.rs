//! SSH server handler trait implementations governing interactive terminal sessions.

mod exec;
mod pty;
mod shell;

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};

use russh::{Channel, ChannelId, ChannelOpenFailure, server};
use russh::{MethodKind, MethodSet};
use tracing::{debug, info, warn};

use crate::auth::{AuthMethod, Authenticator};
use crate::metrics::Metrics;
use crate::server::ConnectionShellState;

use self::pty::ChannelState;

/// SSH server handler that manages interactive terminal sessions.
///
/// This handler implements [`russh::server::Handler`] to accept SSH connections,
/// authenticate clients, manage channels, and route data between the SSH layer
/// and the irosh transfer protocol.
#[derive(Clone)]
pub struct ServerHandler {
    channels: Arc<StdMutex<HashMap<ChannelId, ChannelState>>>,
    /// Tracks which channels are being handled via streams (e.g. port forwarding)
    /// to avoid double-processing in the data() handler.
    streamed_channels: Arc<StdMutex<std::collections::HashSet<ChannelId>>>,
    authenticator: Arc<dyn Authenticator>,
    shell_state: ConnectionShellState,
    metrics: Metrics,
    /// Set once SSH authentication succeeds. Side streams (file transfer,
    /// metadata) are gated on this so unauthenticated peers cannot drive them.
    auth_gate: Option<tokio::sync::watch::Sender<bool>>,
    /// Inactivity timeout for shell channels (`Duration::ZERO` = disabled).
    idle_timeout: std::time::Duration,
    /// Last-traffic instant per channel, driving the idle reaper.
    idle: Arc<StdMutex<HashMap<ChannelId, std::time::Instant>>>,
    /// Ensures one reaper task per connection.
    reaper_started: Arc<std::sync::atomic::AtomicBool>,
}

impl ServerHandler {
    /// Creates a new server handler with the given authenticator and shell state.
    #[must_use]
    pub fn new(authenticator: Arc<dyn Authenticator>, shell_state: ConnectionShellState) -> Self {
        Self {
            channels: Arc::new(StdMutex::new(HashMap::new())),
            streamed_channels: Arc::new(StdMutex::new(std::collections::HashSet::new())),
            authenticator,
            shell_state,
            metrics: Metrics::new(),
            auth_gate: None,
            idle_timeout: std::time::Duration::ZERO,
            idle: Arc::new(StdMutex::new(HashMap::new())),
            reaper_started: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Creates a new handler with the given metrics collector.
    #[must_use]
    pub fn with_metrics(
        authenticator: Arc<dyn Authenticator>,
        shell_state: ConnectionShellState,
        metrics: Metrics,
    ) -> Self {
        Self {
            channels: Arc::new(StdMutex::new(HashMap::new())),
            streamed_channels: Arc::new(StdMutex::new(std::collections::HashSet::new())),
            authenticator,
            shell_state,
            metrics,
            auth_gate: None,
            idle_timeout: std::time::Duration::ZERO,
            idle: Arc::new(StdMutex::new(HashMap::new())),
            reaper_started: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Sets the inactivity timeout after which idle shell channels are closed
    /// (`Duration::ZERO` disables the reaper).
    #[must_use]
    pub(crate) fn with_idle_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Associates this handler with an auth gate that is opened once the SSH
    /// authentication succeeds. Used to gate side-stream dispatch.
    #[must_use]
    pub(crate) fn with_auth_gate(mut self, gate: tokio::sync::watch::Sender<bool>) -> Self {
        self.auth_gate = Some(gate);
        self
    }

    /// Opens the auth gate so side streams may be dispatched.
    fn open_auth_gate(&self) {
        if let Some(gate) = &self.auth_gate {
            let _ = gate.send(true);
        }
    }

    fn lock_channels(&self) -> MutexGuard<'_, HashMap<ChannelId, ChannelState>> {
        match self.channels.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("server channel state mutex poisoned; recovering inner state");
                poisoned.into_inner()
            }
        }
    }

    /// Records traffic on a channel for the idle timeout. Cheap no-op when
    /// the timeout is disabled.
    pub(super) fn mark_channel_active(&self, channel: ChannelId) {
        if self.idle_timeout.is_zero() {
            return;
        }
        Self::touch_idle(&self.idle, channel);
    }

    /// Records `now` for a channel in an idle map owned elsewhere (e.g. the
    /// PTY reader tasks, which have no `&self`). Silently skips poisoned locks.
    fn touch_idle(idle: &StdMutex<HashMap<ChannelId, std::time::Instant>>, channel: ChannelId) {
        if let Ok(mut guard) = idle.lock() {
            guard.insert(channel, std::time::Instant::now());
        }
    }

    /// Stops tracking a channel for the idle timeout (closed or failed).
    fn forget_channel(&self, channel: ChannelId) {
        if let Ok(mut guard) = self.idle.lock() {
            guard.remove(&channel);
        }
    }

    /// Starts the per-connection idle reaper exactly once. The task holds
    /// only a `Weak` reference to the idle map, so it exits on its own once
    /// the connection (and with it the handler) is gone.
    fn ensure_reaper(&self, handle: &server::Handle) {
        if self.idle_timeout.is_zero() {
            return;
        }
        if self
            .reaper_started
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        let idle = Arc::downgrade(&self.idle);
        let handle = handle.clone();
        let timeout = self.idle_timeout;
        tokio::spawn(async move {
            let period = (timeout / 4).clamp(
                std::time::Duration::from_millis(10),
                std::time::Duration::from_secs(60),
            );
            let mut ticker = tokio::time::interval(period);
            loop {
                ticker.tick().await;
                let Some(idle) = idle.upgrade() else {
                    break;
                };
                let now = std::time::Instant::now();
                let stale: Vec<ChannelId> = match idle.lock() {
                    Ok(guard) => guard
                        .iter()
                        .filter_map(|(channel, last)| {
                            (now.duration_since(*last) > timeout).then_some(*channel)
                        })
                        .collect(),
                    Err(_) => break,
                };
                for channel in stale {
                    // Drop the entry first: a racing `data()` simply records
                    // a fresh timestamp afterwards.
                    if let Ok(mut guard) = idle.lock() {
                        guard.remove(&channel);
                    }
                    if handle.close(channel).await.is_err() {
                        break;
                    }
                    debug!(
                        ?channel,
                        ?timeout,
                        "closed shell channel after inactivity timeout"
                    );
                }
            }
        });
    }

    /// Builds a `MethodSet` of the remaining auth methods after excluding `used`.
    fn remaining_methods(methods: Vec<AuthMethod>, used: AuthMethod) -> Option<MethodSet> {
        let remaining: Vec<_> = methods.into_iter().filter(|m| *m != used).collect();
        if remaining.is_empty() {
            None
        } else {
            let mut set = MethodSet::empty();
            for m in remaining {
                match m {
                    AuthMethod::PublicKey => set.push(MethodKind::PublicKey),
                    AuthMethod::Password => set.push(MethodKind::Password),
                }
            }
            Some(set)
        }
    }
}

impl server::Handler for ServerHandler {
    type Error = crate::error::IroshError;

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &russh::keys::ssh_key::PublicKey,
    ) -> std::result::Result<server::Auth, Self::Error> {
        debug!("auth_publickey request for user '{}'", user);
        let accepted = self.authenticator.check_public_key(user, key).await?;
        if accepted {
            self.open_auth_gate();
            Ok(server::Auth::Accept)
        } else {
            self.metrics.record_error();
            let methods = self.authenticator.supported_methods().await;
            Ok(server::Auth::Reject {
                proceed_with_methods: Self::remaining_methods(methods, AuthMethod::PublicKey),
                partial_success: false,
            })
        }
    }

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> std::result::Result<server::Auth, Self::Error> {
        debug!("auth_password request for user '{}'", user);
        let accepted = self.authenticator.check_password(user, password).await?;
        if accepted {
            self.open_auth_gate();
            Ok(server::Auth::Accept)
        } else {
            self.metrics.record_error();
            let methods = self.authenticator.supported_methods().await;
            Ok(server::Auth::Reject {
                proceed_with_methods: Self::remaining_methods(methods, AuthMethod::Password),
                partial_success: false,
            })
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        debug!(
            "channel_open_session request for channel {:?}",
            channel.id()
        );
        self.lock_channels().entry(channel.id()).or_default();
        reply.accept().await;
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: server::ChannelOpenHandle,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        info!(
            "Incoming direct-tcpip request for {}:{}",
            host_to_connect, port_to_connect
        );

        let target = format!("{host_to_connect}:{port_to_connect}");
        let connect = tokio::net::TcpStream::connect(&target);
        let mut stream =
            match tokio::time::timeout(std::time::Duration::from_secs(15), connect).await {
                Ok(Ok(stream)) => stream,
                Ok(Err(err)) => {
                    warn!(
                        "Failed to connect to direct-tcpip target {}: {}",
                        target, err
                    );
                    reply.reject(ChannelOpenFailure::ConnectFailed).await;
                    return Ok(());
                }
                Err(_) => {
                    warn!("Timed out connecting to direct-tcpip target {}", target);
                    reply.reject(ChannelOpenFailure::ConnectFailed).await;
                    return Ok(());
                }
            };

        let channel_id = channel.id();
        let handle = session.handle();
        {
            if let Ok(mut streamed) = self.streamed_channels.lock() {
                streamed.insert(channel_id);
            }
        }

        reply.accept().await;

        tokio::spawn(async move {
            let mut channel_stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut channel_stream).await;
            let _ = handle.close(channel_id).await;
        });

        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        modes: &[(russh::Pty, u32)],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        info!(
            "pty_request for channel {:?}: term={}, cols={}, rows={}",
            channel, term, col_width, row_height
        );
        // Honor the client's terminal ECHO mode (OpenSSH convention: echo is
        // on unless explicitly disabled). ConPTY never echoes by itself; the
        // flag is consumed at spawn to decide on server-side echo for shells
        // without their own line rendering (cmd.exe).
        let echo = !modes
            .iter()
            .any(|(mode, value)| *mode == russh::Pty::ECHO && *value == 0);
        self.set_channel_pty(
            channel,
            term,
            crate::session::pty::pty_size(col_width, row_height, pix_width, pix_height),
            echo,
            session,
        )
    }

    async fn env_request(
        &mut self,
        channel: ChannelId,
        variable_name: &str,
        variable_value: &str,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        debug!(
            "env_request for channel {:?}: {}={}",
            channel, variable_name, variable_value
        );
        self.record_env(channel, variable_name, variable_value, session)
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        info!("shell_request for channel {:?}", channel);
        self.start_command(channel, session, None)?;
        self.mark_channel_active(channel);
        self.ensure_reaper(&session.handle());
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        let command = String::from_utf8_lossy(data).trim().to_string();
        debug!("exec_request for channel {:?}: {}", channel, command);
        self.start_command(channel, session, Some(&command))?;
        self.mark_channel_active(channel);
        self.ensure_reaper(&session.handle());
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        // If this channel is being handled by a stream (into_stream),
        // we MUST NOT consume the data here or the stream will be starved.
        {
            if let Ok(streamed) = self.streamed_channels.lock() {
                if streamed.contains(&channel) {
                    return Ok(());
                }
            }
        }

        debug!(
            "data received for channel {:?}: {} bytes",
            channel,
            data.len()
        );
        self.mark_channel_active(channel);
        self.write_channel_data(channel, data).await;
        // Server-side echo for shells without their own input rendering.
        if self.channel_server_echo(channel) {
            let echo = pty::filter_echo_bytes(data);
            if !echo.is_empty() {
                let _ = session
                    .handle()
                    .data(channel, bytes::Bytes::from(echo))
                    .await;
            }
        }
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        debug!("window_change_request for channel {:?}", channel);
        self.resize_channel(
            channel, col_width, row_height, pix_width, pix_height, session,
        )
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        debug!("channel_eof for channel {:?}", channel);
        self.forget_channel(channel);
        self.close_channel_writer(channel);
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        debug!("channel_close for channel {:?}", channel);
        {
            if let Ok(mut streamed) = self.streamed_channels.lock() {
                streamed.remove(&channel);
            }
        }
        self.forget_channel(channel);
        self.close_channel(channel);
        Ok(())
    }

    async fn signal(
        &mut self,
        channel: ChannelId,
        signal: russh::Sig,
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        debug!("signal request for channel {:?}: {:?}", channel, signal);
        self.forward_signal(channel, &signal);
        Ok(())
    }
}

#[cfg(test)]
mod send_sync_tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn server_handler_is_send_sync() {
        assert_send_sync::<ServerHandler>();
    }
}

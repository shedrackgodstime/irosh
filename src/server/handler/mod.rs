//! SSH server handler trait implementations governing interactive terminal sessions.

mod pty;

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};

use russh::{Channel, ChannelId, server};
use russh::{MethodKind, MethodSet};
use tracing::{debug, info, warn};

use crate::auth::{AuthMethod, Authenticator};
use crate::server::ConnectionShellState;

use self::pty::ChannelState;

#[derive(Clone)]
pub(crate) struct ServerHandler {
    channels: Arc<StdMutex<HashMap<ChannelId, ChannelState>>>,
    /// Tracks which channels are being handled via streams (e.g. port forwarding)
    /// to avoid double-processing in the data() handler.
    streamed_channels: Arc<StdMutex<std::collections::HashSet<ChannelId>>>,
    authenticator: Arc<dyn Authenticator>,
    shell_state: ConnectionShellState,
}

impl ServerHandler {
    pub(crate) fn new(
        authenticator: Arc<dyn Authenticator>,
        shell_state: ConnectionShellState,
    ) -> Self {
        Self {
            channels: Arc::new(StdMutex::new(HashMap::new())),
            streamed_channels: Arc::new(StdMutex::new(std::collections::HashSet::new())),
            authenticator,
            shell_state,
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

    /// Builds a `MethodSet` of the remaining auth methods after excluding `used`.
    fn remaining_methods(&self, used: AuthMethod) -> Option<MethodSet> {
        let supported = self.authenticator.supported_methods();
        let remaining: Vec<_> = supported.into_iter().filter(|m| *m != used).collect();
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
        match self.authenticator.check_public_key(user, key)? {
            true => Ok(server::Auth::Accept),
            false => Ok(server::Auth::Reject {
                proceed_with_methods: self.remaining_methods(AuthMethod::PublicKey),
                partial_success: false,
            }),
        }
    }

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> std::result::Result<server::Auth, Self::Error> {
        debug!("auth_password request for user '{}'", user);
        match self.authenticator.check_password(user, password)? {
            true => Ok(server::Auth::Accept),
            false => Ok(server::Auth::Reject {
                proceed_with_methods: self.remaining_methods(AuthMethod::Password),
                partial_success: false,
            }),
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        debug!(
            "channel_open_session request for channel {:?}",
            channel.id()
        );
        self.lock_channels().entry(channel.id()).or_default();
        Ok(true)
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        info!(
            "Incoming direct-tcpip request for {}:{}",
            host_to_connect, port_to_connect
        );

        let target = format!("{}:{}", host_to_connect, port_to_connect);
        let mut stream = match tokio::net::TcpStream::connect(&target).await {
            Ok(stream) => stream,
            Err(err) => {
                warn!(
                    "Failed to connect to direct-tcpip target {}: {}",
                    target, err
                );
                return Ok(false);
            }
        };

        let channel_id = channel.id();
        let handle = _session.handle();
        {
            if let Ok(mut streamed) = self.streamed_channels.lock() {
                streamed.insert(channel_id);
            }
        }

        tokio::spawn(async move {
            let mut channel_stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut channel_stream).await;
            let _ = handle.close(channel_id).await;
        });

        Ok(true)
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        debug!("pty_request for channel {:?}", channel);
        self.set_channel_pty(
            channel,
            term,
            crate::session::pty::pty_size(col_width, row_height, pix_width, pix_height),
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
        debug!("shell_request for channel {:?}", channel);
        self.start_command(channel, session, None)?;
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
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut server::Session,
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
        self.write_channel_data(channel, data);
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
        self.forward_signal(channel, signal);
        Ok(())
    }
}

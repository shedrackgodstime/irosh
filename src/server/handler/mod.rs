//! SSH server handler trait implementations governing interactive terminal sessions.

mod pty;

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};

use russh::keys::ssh_key::PublicKey;
use russh::{Channel, ChannelId, server};
use tracing::{info, warn};

use crate::config::{HostKeyPolicy, SecurityConfig, StateConfig};
use crate::error::Result;
use crate::server::ConnectionShellState;
use crate::storage::trust::write_authorized_client;

use self::pty::ChannelState;

#[derive(Clone)]
pub(crate) struct ServerHandler {
    channels: Arc<StdMutex<HashMap<ChannelId, ChannelState>>>,
    authorized_clients: Arc<StdMutex<Vec<PublicKey>>>,
    security: SecurityConfig,
    state: StateConfig,
    shell_state: ConnectionShellState,
}

impl ServerHandler {
    pub(crate) fn new(
        authorized_clients: Vec<PublicKey>,
        security: SecurityConfig,
        state: StateConfig,
        shell_state: ConnectionShellState,
    ) -> Self {
        Self {
            channels: Arc::new(StdMutex::new(HashMap::new())),
            authorized_clients: Arc::new(StdMutex::new(authorized_clients)),
            security,
            state,
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

    fn lock_authorized_clients(&self) -> MutexGuard<'_, Vec<PublicKey>> {
        match self.authorized_clients.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("authorized client state mutex poisoned; recovering inner state");
                poisoned.into_inner()
            }
        }
    }

    fn authenticate_client(&self, key: &russh::keys::ssh_key::PublicKey) -> Result<server::Auth> {
        let fingerprint = key
            .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
            .to_string();

        if self.security.host_key_policy == HostKeyPolicy::AcceptAll {
            info!(%fingerprint, "AcceptAll policy: automatically accepting client key.");
            return Ok(server::Auth::Accept);
        }

        let mut authorized = self.lock_authorized_clients();

        // 1. Check if we already have this key or ANY other keys authorized.
        if !authorized.is_empty() {
            if authorized.contains(key) {
                info!(%fingerprint, "Client matched pre-authorized key. Access granted.");
                return Ok(server::Auth::Accept);
            }

            warn!(%fingerprint, "Client key not in authorized list. Rejecting connection.");
            return Ok(server::Auth::reject());
        }

        // 2. No authorized keys yet. Check policy for new keys.
        match self.security.host_key_policy {
            HostKeyPolicy::Strict => {
                warn!(%fingerprint, "Strict policy: No pre-authorized keys found. Rejecting connection.");
                Ok(server::Auth::reject())
            }
            HostKeyPolicy::Tofu => {
                info!(%fingerprint, "Tofu policy: No pre-authorized keys found. Trusting first client.");
                let _event = write_authorized_client(&self.state, &fingerprint, key)?;
                authorized.push(key.clone());
                Ok(server::Auth::Accept)
            }
            HostKeyPolicy::AcceptAll => unreachable!(),
        }
    }
}

impl server::Handler for ServerHandler {
    type Error = crate::error::IroshError;

    async fn auth_publickey(
        &mut self,
        _user: &str,
        key: &russh::keys::ssh_key::PublicKey,
    ) -> std::result::Result<server::Auth, Self::Error> {
        self.authenticate_client(key)
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        self.lock_channels().entry(channel.id()).or_default();
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
        self.record_env(channel, variable_name, variable_value, session)
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
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
        self.start_command(channel, session, Some(&command))?;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
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
        self.resize_channel(
            channel, col_width, row_height, pix_width, pix_height, session,
        )
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        self.close_channel_writer(channel);
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        self.close_channel(channel);
        Ok(())
    }

    async fn signal(
        &mut self,
        channel: ChannelId,
        signal: russh::Sig,
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        self.forward_signal(channel, signal);
        Ok(())
    }
}

use std::collections::HashMap;
use std::io::{Read, Write};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use russh::{ChannelId, server};
use tracing::{error, warn};

use crate::error::{Result, ServerError};
use crate::session::pty::{default_pty_size, pty_size};

use super::ServerHandler;

#[derive(Default)]
pub(super) struct ChannelState {
    pty: PtySpec,
    env: HashMap<String, String>,
    process: Option<RunningPty>,
}

struct RunningPty {
    master: Box<dyn MasterPty + Send>,
    writer: Option<Box<dyn Write + Send>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pid: Option<u32>,
    #[cfg(unix)]
    pgid: Option<libc::pid_t>,
}

#[derive(Clone)]
struct PtySpec {
    term: String,
    size: PtySize,
}

impl Default for PtySpec {
    fn default() -> Self {
        Self {
            term: "xterm-256color".to_string(),
            size: default_pty_size(),
        }
    }
}

impl ServerHandler {
    pub(super) fn set_channel_pty(
        &self,
        channel: ChannelId,
        term: &str,
        size: PtySize,
        session: &mut server::Session,
    ) -> std::result::Result<(), crate::error::IroshError> {
        let mut channels = self.lock_channels();
        let state_entry = channels.entry(channel).or_default();
        state_entry.pty = PtySpec {
            term: term.to_string(),
            size,
        };
        session.channel_success(channel)?;
        Ok(())
    }

    pub(super) fn start_command(
        &self,
        channel: ChannelId,
        session: &mut server::Session,
        command: Option<&str>,
    ) -> Result<()> {
        let mut channels = self.lock_channels();
        let state_entry = channels.entry(channel).or_default();
        if state_entry.process.is_some() {
            session
                .channel_failure(channel)
                .map_err(|e| ServerError::ChannelError {
                    operation: "reject duplicate channel",
                    details: e.to_string(),
                })?;
            return Ok(());
        }

        let pty_system = native_pty_system();
        let pair =
            pty_system
                .openpty(state_entry.pty.size)
                .map_err(|e| ServerError::ShellError {
                    details: format!("failed to open PTY: {e}"),
                })?;

        let mut builder = if let Some(command) = command {
            let mut command_builder = CommandBuilder::new("sh");
            command_builder.arg("-lc");
            command_builder.arg(command);
            command_builder
        } else {
            CommandBuilder::new_default_prog()
        };
        builder.env("TERM", &state_entry.pty.term);
        for (key, value) in &state_entry.env {
            builder.env(key, value);
        }

        #[cfg(unix)]
        let pgid = pair
            .master
            .process_group_leader()
            .map(|id| id as libc::pid_t);

        let mut child = pair
            .slave
            .spawn_command(builder)
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to spawn command in PTY: {e}"),
            })?;
        let pid = child.process_id();
        self.shell_state.set_shell_pid(pid);
        let killer = child.clone_killer();
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to clone PTY reader: {e}"),
            })?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to take PTY writer: {e}"),
            })?;
        let handle = session.handle();
        let runtime = tokio::runtime::Handle::current();
        let channels_ref = self.channels.clone();
        let shell_state = self.shell_state.clone();

        state_entry.process = Some(RunningPty {
            master: pair.master,
            writer: Some(writer),
            killer,
            pid,
            #[cfg(unix)]
            pgid,
        });

        session
            .channel_success(channel)
            .map_err(|e| ServerError::ChannelError {
                operation: "confirm channel success",
                details: e.to_string(),
            })?;

        tokio::task::spawn_blocking(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = buf[..n].to_vec();
                        let _ = runtime.block_on(handle.data(channel, data.into()));
                    }
                    Err(err) => {
                        error!("PTY read error on channel {:?}: {}", channel, err);
                        break;
                    }
                }
            }

            let exit_status = child
                .wait()
                .ok()
                .map(|status| status.exit_code())
                .unwrap_or(255);
            runtime.block_on(async {
                let _ = handle.exit_status_request(channel, exit_status).await;
                let _ = handle.eof(channel).await;
                let _ = handle.close(channel).await;
            });
            shell_state.clear_shell_pid_if_matches(pid);

            let mut channels = match channels_ref.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    warn!("server channel state mutex poisoned; recovering inner state");
                    poisoned.into_inner()
                }
            };
            channels.remove(&channel);
        });

        Ok(())
    }

    pub(super) fn record_env(
        &self,
        channel: ChannelId,
        variable_name: &str,
        variable_value: &str,
        session: &mut server::Session,
    ) -> std::result::Result<(), crate::error::IroshError> {
        let mut channels = self.lock_channels();
        let state_entry = channels.entry(channel).or_default();
        state_entry
            .env
            .insert(variable_name.to_string(), variable_value.to_string());
        session.channel_success(channel)?;
        Ok(())
    }

    pub(super) fn write_channel_data(&self, channel: ChannelId, data: &[u8]) {
        let mut channels = self.lock_channels();
        if let Some(state_entry) = channels.get_mut(&channel)
            && let Some(process) = state_entry.process.as_mut()
            && let Some(writer) = process.writer.as_mut()
        {
            let _ = writer.write_all(data);
            let _ = writer.flush();
        }
    }

    pub(super) fn resize_channel(
        &self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        session: &mut server::Session,
    ) -> std::result::Result<(), crate::error::IroshError> {
        let size = pty_size(col_width, row_height, pix_width, pix_height);
        let mut channels = self.lock_channels();
        let state_entry = channels.entry(channel).or_default();
        state_entry.pty.size = size;
        if let Some(process) = state_entry.process.as_ref() {
            let _ = process.master.resize(size);
        }
        session.channel_success(channel)?;
        Ok(())
    }

    pub(super) fn close_channel_writer(&self, channel: ChannelId) {
        let mut channels = self.lock_channels();
        if let Some(state_entry) = channels.get_mut(&channel)
            && let Some(process) = state_entry.process.as_mut()
        {
            process.writer.take();
        }
    }

    pub(super) fn close_channel(&self, channel: ChannelId) {
        let mut channels = self.lock_channels();
        if let Some(mut state_entry) = channels.remove(&channel)
            && let Some(mut process) = state_entry.process.take()
        {
            self.shell_state.clear_shell_pid_if_matches(process.pid);
            process.writer.take();
            let _ = process.killer.kill();
        }
    }

    pub(super) fn forward_signal(&self, channel: ChannelId, signal: russh::Sig) {
        #[cfg(unix)]
        {
            let channels = self.lock_channels();
            if let Some(state_entry) = channels.get(&channel)
                && let Some(process) = state_entry.process.as_ref()
            {
                if let (Some(pgid), Some(sig)) =
                    (process.pgid, crate::session::pty::map_sig(signal))
                {
                    // SAFETY: The pgid is a valid process group ID created during PTY allocation
                    // for this specific channel. This ensures all members of the shell
                    // session are terminated.
                    unsafe {
                        libc::killpg(pgid, sig);
                    }
                }
            }
        }
    }
}

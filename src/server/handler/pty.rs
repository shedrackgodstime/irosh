use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex as StdMutex};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use russh::{ChannelId, server};
use tracing::{debug, info, warn};

#[cfg(unix)]
use tracing::error;

use crate::error::{Result, ServerError};
use crate::server::transfer::ConnectionShellState;
use crate::session::pty::{default_pty_size, pty_size};

use super::ServerHandler;

use tokio_util::sync::CancellationToken;

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
    shutdown: CancellationToken,
}

struct CleanupGuard {
    channel: ChannelId,
    pid: u32,
    shell_state: ConnectionShellState,
    channels: Arc<StdMutex<HashMap<ChannelId, ChannelState>>>,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        debug!("Performing PTY cleanup for channel {:?}", self.channel);
        self.shell_state.clear_shell_pid_if_matches(Some(self.pid));

        let mut channels = match self.channels.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("server channel state mutex poisoned during cleanup; recovering");
                poisoned.into_inner()
            }
        };
        channels.remove(&self.channel);
    }
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
        debug!(
            "start_command called for channel {:?}, command: {:?}",
            channel, command
        );
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
        if command.is_none() {
            info!("Registering PRIMARY shell PID {:?} for session state", pid);
            self.shell_state.set_shell_pid(pid);
        } else {
            debug!(
                "Exec command PID {:?} started (not registering as primary session PID)",
                pid
            );
        }
        let killer = child.clone_killer();

        #[allow(unused_mut)]
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

        #[cfg(unix)]
        let maybe_fd = pair.master.as_raw_fd();

        #[cfg(unix)]
        if let Some(fd) = maybe_fd {
            // SAFETY: `fd` is a valid file descriptor from `portable_pty`.
            // Setting it to non-blocking is required for `AsyncFd`.
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                if flags != -1 {
                    let _ = libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                }
            }
        }

        let handle = session.handle();
        let channels_ref = self.channels.clone();
        let shell_state = self.shell_state.clone();

        let shutdown = CancellationToken::new();
        let task_shutdown = shutdown.clone();

        state_entry.process = Some(RunningPty {
            master: pair.master,
            writer: Some(writer),
            killer,
            pid,
            #[cfg(unix)]
            pgid,
            shutdown,
        });

        session
            .channel_success(channel)
            .map_err(|e| ServerError::ChannelError {
                operation: "confirm channel success",
                details: e.to_string(),
            })?;

        tokio::spawn(async move {
            debug!("PTY reader task started for channel {:?}", channel);
            let _guard = CleanupGuard {
                channel,
                pid: pid.unwrap_or(0),
                shell_state,
                channels: channels_ref,
            };

            #[cfg(unix)]
            if let Some(fd) = maybe_fd {
                use tokio::io::unix::AsyncFd;

                debug!("PTY reader using FD {:?} for channel {:?}", fd, channel);

                // We wrap the MasterPty's raw FD in an AsyncFd to perform
                // non-blocking reads that are integrated with the Tokio reactor.
                struct RawFdWrapper(std::os::unix::io::RawFd);
                impl std::os::unix::io::AsRawFd for RawFdWrapper {
                    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
                        self.0
                    }
                }

                let async_fd = match AsyncFd::new(RawFdWrapper(fd)) {
                    Ok(fd) => Some(fd),
                    Err(err) => {
                        error!("Failed to create AsyncFd for PTY reader: {}", err);
                        None
                    }
                };

                if let Some(async_fd) = async_fd {
                    let mut buf = [0u8; 8192];
                    loop {
                        tokio::select! {
                            biased;
                            _ = task_shutdown.cancelled() => {
                                debug!("PTY reader task cancelled for channel {:?}", channel);
                                return;
                            }
                            res = async_fd.readable() => {
                                match res {
                                    Ok(mut guard) => {
                                        match reader.read(&mut buf) {
                                            Ok(0) => {
                                                debug!("PTY reader received EOF for channel {:?}", channel);
                                                break;
                                            }
                                            Ok(n) => {
                                                guard.retain_ready();
                                                let data = buf[..n].to_vec();
                                                debug!("PTY reader read {} bytes from channel {:?}", n, channel);
                                                if let Err(err) = handle.data(channel, data.into()).await {
                                                    error!("PTY reader failed to send {} bytes to channel {:?}: {:?}", n, channel, err);
                                                    break;
                                                }
                                                debug!("PTY reader sent {} bytes to channel {:?}", n, channel);
                                            }
                                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                                guard.clear_ready();
                                            }
                                            Err(err) => {
                                                error!("PTY read error on channel {:?}: {}", channel, err);
                                                break;
                                            }
                                        }
                                    }
                                    Err(err) => {
                                        error!("AsyncFd error on channel {:?}: {}", channel, err);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            #[cfg(not(unix))]
            {
                // Fallback for non-unix platforms.
                let mut reader = reader;
                let handle = handle.clone();
                let task_shutdown = task_shutdown.clone();

                tokio::task::spawn_blocking(move || {
                    let mut buf = [0u8; 8192];
                    let runtime = tokio::runtime::Handle::current();
                    loop {
                        if task_shutdown.is_cancelled() {
                            break;
                        }
                        match reader.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                let data = buf[..n].to_vec();
                                if runtime.block_on(handle.data(channel, data.into())).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                })
                .await
                .ok();
            }

            debug!("PTY reader task waiting for child process {:?}...", pid);
            let exit_status = tokio::task::spawn_blocking(move || {
                child
                    .wait()
                    .ok()
                    .map(|status| status.exit_code())
                    .unwrap_or(255)
            })
            .await
            .unwrap_or(255);

            debug!(
                "PTY reader task finishing for channel {:?} with exit code {}",
                channel, exit_status
            );

            if let Err(e) = handle.exit_status_request(channel, exit_status).await {
                warn!(
                    "Failed to send exit status {} for channel {:?}: {:?}",
                    exit_status, channel, e
                );
            }
            if let Err(e) = handle.eof(channel).await {
                debug!(
                    "Failed to send EOF for channel {:?}: {:?} (likely already closed)",
                    channel, e
                );
            }
            if let Err(e) = handle.close(channel).await {
                debug!(
                    "Failed to send close for channel {:?}: {:?} (likely already closed)",
                    channel, e
                );
            }
        });

        Ok(())
    }

    pub(super) fn record_env(
        &mut self,
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
            process.shutdown.cancel();
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
        #[cfg(not(unix))]
        {
            let _ = (channel, signal);
        }
    }
}

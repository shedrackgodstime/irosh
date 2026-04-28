use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex as StdMutex};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use russh::{ChannelId, server};
use tracing::{debug, info, warn};

use crate::error::{Result, ServerError};
use crate::server::transfer::ConnectionShellState;
use crate::session::pty::{default_pty_size, pty_size};

use super::ServerHandler;

use tokio_util::sync::CancellationToken;

/// Shared ownership of the master PTY handle.
///
/// Both `RunningPty` (for resize operations) and the spawned reader task (for
/// closing the ConPTY on Windows when the child exits) need to access the master.
/// Wrapping it in `Arc<StdMutex<Option<...>>>` allows the task to take and drop the
/// handle without requiring `RunningPty` to be moved into the task.
type SharedMaster = Arc<StdMutex<Option<Box<dyn MasterPty + Send>>>>;

#[derive(Default)]
pub(super) struct ChannelState {
    pty: PtySpec,
    env: HashMap<String, String>,
    process: Option<RunningPty>,
}

struct RunningPty {
    /// Shared master PTY handle. Kept here for `resize` and, on Windows, to allow
    /// the reader task to close the ConPTY when the child exits.
    master: SharedMaster,
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
            #[cfg(unix)]
            {
                let mut command_builder = CommandBuilder::new("sh");
                command_builder.arg("-lc");
                command_builder.arg(command);
                command_builder
            }
            #[cfg(windows)]
            {
                let mut command_builder = CommandBuilder::new(windows_command_processor());
                command_builder.arg("/C");
                command_builder.arg(command);
                command_builder
            }
            #[cfg(not(any(unix, windows)))]
            {
                let mut command_builder = CommandBuilder::new("sh");
                command_builder.arg("-c");
                command_builder.arg(command);
                command_builder
            }
        } else {
            #[cfg(windows)]
            {
                CommandBuilder::new(windows_command_processor())
            }
            #[cfg(not(windows))]
            {
                CommandBuilder::new_default_prog()
            }
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
        info!(
            "Spawned PTY child for channel {:?}: command={:?}, pid={:?}",
            channel, command, pid
        );
        if command.is_none() {
            info!("Registering PRIMARY shell PID {:?} for session state", pid);
            self.shell_state.set_shell_pid(pid);
        } else {
            info!(
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

        // Wrap the master in a shared handle. The spawned task holds a clone so
        // it can close the ConPTY on Windows when the child exits (which unblocks
        // the blocking reader thread). The `RunningPty` entry retains the other
        // clone for resize operations.
        let shared_master: SharedMaster = Arc::new(StdMutex::new(Some(pair.master)));

        let handle = session.handle();
        let channels_ref = self.channels.clone();
        let shell_state = self.shell_state.clone();

        let shutdown = CancellationToken::new();
        #[cfg(unix)]
        let task_shutdown = shutdown.clone();

        // Clone for the spawned task (Windows only needs it to drop on child exit).
        let task_master = shared_master.clone();

        state_entry.process = Some(RunningPty {
            master: shared_master,
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
        
        let _ = session.data(channel, "\r\n🔗 Connected to Irosh (Windows Host)\r\n\r\n".into());

        tokio::spawn(async move {
            debug!("PTY reader task started for channel {:?}", channel);
            let _guard = CleanupGuard {
                channel,
                pid: pid.unwrap_or(0),
                shell_state,
                channels: channels_ref,
            };

            let handle_for_task = handle.clone();
            let mut reader = reader;

            let reader_done = CancellationToken::new();

            #[cfg(unix)]
            let reader_future = async {
                if let Some(fd) = maybe_fd {
                    use tokio::io::unix::AsyncFd;
                    struct RawFdWrapper(std::os::unix::io::RawFd);
                    impl std::os::unix::io::AsRawFd for RawFdWrapper {
                        fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
                            self.0
                        }
                    }

                    if let Ok(async_fd) = AsyncFd::new(RawFdWrapper(fd)) {
                        let mut buf = [0u8; 8192];
                        loop {
                            tokio::select! {
                                biased;
                                _ = task_shutdown.cancelled() => {
                                    debug!("PTY reader task cancelled for channel {:?}", channel);
                                    break;
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
                                                    debug!("PTY reader read {} bytes from channel {:?}", n, channel);
                                                    if let Err(e) = handle_for_task.data(channel, buf[..n].to_vec().into()).await {
                                                        warn!("PTY reader failed to send data to channel {:?}: {:?}", channel, e);
                                                        break;
                                                    }
                                                }
                                                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                                    guard.clear_ready();
                                                }
                                                Err(e) => {
                                                    debug!("PTY read error on channel {:?}: {}", channel, e);
                                                    break;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            debug!("AsyncFd error on channel {:?}: {}", channel, e);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            };

            #[cfg(not(unix))]
            let reader_done_cloned = reader_done.clone();

            #[cfg(not(unix))]
            let reader_future = async move {
                let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(1024);
                let reader_done_task = reader_done_cloned;

                info!(
                    "Spawning blocking PTY reader thread for channel {:?}",
                    channel
                );
                tokio::task::spawn_blocking(move || {
                    let mut buf = [0u8; 8192];
                    loop {
                        if reader_done_task.is_cancelled() {
                            info!(
                                "PTY reader thread received cancellation for channel {:?}",
                                channel
                            );
                            break;
                        }
                        match reader.read(&mut buf) {
                            Ok(0) => {
                                info!("PTY reader thread received EOF for channel {:?}", channel);
                                break;
                            }
                            Ok(n) => {
                                info!(
                                    "PTY reader thread read {} bytes for channel {:?}: {}",
                                    n,
                                    channel,
                                    preview_bytes(&buf[..n])
                                );
                                if tx.blocking_send(buf[..n].to_vec()).is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                info!("PTY reader thread error on channel {:?}: {}", channel, e);
                                break;
                            }
                        }
                    }
                });

                while let Some(data) = rx.recv().await {
                    info!(
                        "Forwarding {} PTY bytes to SSH channel {:?}: {}",
                        data.len(),
                        channel,
                        preview_bytes(&data)
                    );
                    if let Err(e) = handle_for_task.data(channel, data.into()).await {
                        warn!("Failed to send PTY data to channel {:?}: {:?}", channel, e);
                        break;
                    }
                }
            };

            let mut child_waiter = tokio::task::spawn_blocking(move || {
                info!(
                    "Waiting for child process {:?} for channel {:?}",
                    pid, channel
                );
                let res = child.wait().ok().map(|s| s.exit_code()).unwrap_or(255);
                info!(
                    "Child process {:?} for channel {:?} exited with code {}",
                    pid, channel, res
                );
                res
            });

            let exit_status = tokio::select! {
                status = &mut child_waiter => {
                    let status = status.unwrap_or(255);
                    // Child exited first. Signal the reader loop to stop.
                    reader_done.cancel();
                    // On Windows (non-unix), the blocking reader thread is stuck on
                    // `reader.read()` which never returns EOF until the ConPTY write
                    // end is closed. Dropping the master PTY handle here closes the
                    // ConPTY session, causing the reader's `read()` to return an
                    // error/EOF and allowing the blocking thread to exit cleanly.
                    //
                    // On Unix the master fd is set to non-blocking and the async
                    // `AsyncFd`-based reader already handles cancellation, so we
                    // do not need to drop the master here.
                    #[cfg(not(unix))]
                    {
                        if let Ok(mut guard) = task_master.lock() {
                            drop(guard.take());
                        }
                    }
                    #[cfg(unix)]
                    {
                        // Suppress unused-variable warning on Unix builds.
                        let _ = &task_master;
                    }
                    status
                }
                _ = reader_future => {
                    // Reader finished (EOF). Wait for child to get exit status.
                    child_waiter.await.unwrap_or(255)
                }
            };

            debug!(
                "PTY task finishing for channel {:?} with exit code {}",
                channel, exit_status
            );

            let _ = handle.exit_status_request(channel, exit_status).await;
            let _ = handle.eof(channel).await;
            let _ = handle.close(channel).await;
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
        info!(
            "Writing {} SSH bytes into PTY channel {:?}: {}",
            data.len(),
            channel,
            preview_bytes(data)
        );
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
            // The master may already have been dropped (e.g. on Windows after
            // the child exited). Silently ignore the resize in that case.
            if let Ok(guard) = process.master.lock() {
                if let Some(master) = guard.as_ref() {
                    let _ = master.resize(size);
                }
            }
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
            // Drop the master PTY handle to ensure any ConPTY session is fully
            // torn down, releasing all associated OS resources.
            if let Ok(mut guard) = process.master.lock() {
                drop(guard.take());
            }
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
            use windows_sys::Win32::System::Console::*;
            let channels = self.lock_channels();
            if let Some(state_entry) = channels.get(&channel)
                && let Some(process) = state_entry.process.as_ref()
                && let Some(pid) = process.pid
            {
                let event = match signal {
                    russh::Sig::INT => Some(CTRL_C_EVENT),
                    russh::Sig::QUIT | russh::Sig::ABRT => Some(CTRL_BREAK_EVENT),
                    _ => None,
                };
                if let Some(event) = event {
                    unsafe {
                        GenerateConsoleCtrlEvent(event, pid);
                    }
                }
            }
        }
    }
}

#[cfg(windows)]
fn windows_command_processor() -> String {
    "powershell.exe".to_string()
}

fn preview_bytes(bytes: &[u8]) -> String {
    const MAX_PREVIEW: usize = 24;
    let preview = &bytes[..bytes.len().min(MAX_PREVIEW)];
    let rendered = preview
        .iter()
        .map(|byte| match byte {
            b'\r' => "\\r".to_string(),
            b'\n' => "\\n".to_string(),
            b'\t' => "\\t".to_string(),
            0x20..=0x7e => (*byte as char).to_string(),
            _ => format!("\\x{byte:02x}"),
        })
        .collect::<String>();

    if bytes.len() > MAX_PREVIEW {
        format!("{rendered}...")
    } else {
        rendered
    }
}

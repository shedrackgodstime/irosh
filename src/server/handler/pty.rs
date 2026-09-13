//! SSH PTY session handler.
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex as StdMutex};

use bytes::{Bytes, BytesMut};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use russh::{ChannelId, server};
#[cfg(windows)]
use tracing::trace;
use tracing::{debug, info, warn};

use crate::error::{Result, ServerError};
use crate::server::transfer::ConnectionShellState;
use crate::session::pty::{default_pty_size, pty_size};

use super::ServerHandler;

use tokio_util::sync::CancellationToken;

/// How long after channel open the reader thread auto-answers the shell's
/// DSR cursor-position query (`ESC [ 6 n`). PowerShell can take a couple of
/// seconds to reach its prompt on slow hosts; after the window closes the
/// client terminal answers any further queries itself.
#[cfg(not(unix))]
const DSR_AUTOREPLY_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

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
    pty_tx: Option<tokio::sync::mpsc::Sender<Bytes>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pid: Option<u32>,
    #[cfg(unix)]
    pgid: Option<libc::pid_t>,
    shutdown: CancellationToken,
    /// Mirror client input back to the client (terminal ECHO). Only true on
    /// Windows cmd.exe sessions: ConPTY does not echo itself, while Unix
    /// kernels and PSReadLine render input on their own.
    server_echo: bool,
}

struct CleanupGuard {
    channel: ChannelId,
    pid: u32,
    shell_state: ConnectionShellState,
    channels: Arc<StdMutex<HashMap<ChannelId, ChannelState>>>,
    idle: Arc<StdMutex<HashMap<ChannelId, std::time::Instant>>>,
}

#[cfg(unix)]
struct RawFdWrapper(std::os::unix::io::RawFd);

#[cfg(unix)]
impl std::os::unix::io::AsRawFd for RawFdWrapper {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        self.0
    }
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
        if let Ok(mut idle) = self.idle.lock() {
            idle.remove(&self.channel);
        }
    }
}

#[derive(Clone)]
struct PtySpec {
    term: String,
    size: PtySize,
    /// Whether the client negotiated terminal ECHO for this channel.
    /// Defaults to false (no pty requested means no echo).
    /// Only consumed on Windows: ConPTY never echoes input itself, so the
    /// server mirrors keystrokes; Unix kernels echo in the line discipline,
    /// so on non-Windows this field is deliberately never read.
    #[cfg_attr(not(windows), allow(dead_code))]
    echo: bool,
}

impl Default for PtySpec {
    fn default() -> Self {
        Self {
            term: "xterm-256color".to_string(),
            size: default_pty_size(),
            echo: false,
        }
    }
}

impl ServerHandler {
    pub(super) fn set_channel_pty(
        &self,
        channel: ChannelId,
        term: &str,
        size: PtySize,
        echo: bool,
        session: &mut server::Session,
    ) -> std::result::Result<(), crate::error::IroshError> {
        let mut channels = self.lock_channels();
        let state_entry = channels.entry(channel).or_default();
        state_entry.pty = PtySpec {
            term: term.to_string(),
            size,
            echo,
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

        let mut builder = build_command(command);

        builder.env("TERM", &state_entry.pty.term);

        // On Windows, ensure we have a decent PATH if running as a service
        #[cfg(windows)]
        {
            use std::collections::HashSet;
            use std::path::PathBuf;

            let mut paths = Vec::new();
            let mut seen = HashSet::new();

            // Helper to add paths without duplicates
            let mut add_path = |p: PathBuf| {
                if seen.insert(p.clone()) {
                    paths.push(p);
                }
            };

            // 1. Current process PATH
            if let Ok(current_path) = std::env::var("PATH") {
                for p in std::env::split_paths(&current_path) {
                    add_path(p);
                }
            }

            // 2. Read from Registry (HKCU) to capture user environment
            let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
            if let Ok(env) = hkcu.open_subkey("Environment") {
                if let Ok(user_path) = env.get_value::<String, _>("Path") {
                    for mut p in std::env::split_paths(&user_path) {
                        // Very basic expansion of %USERPROFILE% if it exists, since
                        // winreg returns REG_EXPAND_SZ unexpanded.
                        let p_str = p.to_string_lossy().to_string();
                        if p_str.contains("%USERPROFILE%") {
                            if let Some(home) = dirs::home_dir() {
                                let expanded =
                                    p_str.replace("%USERPROFILE%", &home.to_string_lossy());
                                p = PathBuf::from(expanded);
                            }
                        }
                        add_path(p);
                    }
                }
            }

            let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
            if let Ok(env) =
                hklm.open_subkey("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment")
            {
                if let Ok(sys_path) = env.get_value::<String, _>("Path") {
                    for p in std::env::split_paths(&sys_path) {
                        add_path(p);
                    }
                }
            }

            // 3. Fallbacks: .cargo/bin and current exe dir
            if let Some(home) = dirs::home_dir() {
                add_path(home.join(".cargo").join("bin"));
            }

            if let Ok(current_exe) = std::env::current_exe() {
                if let Some(exe_dir) = current_exe.parent() {
                    add_path(exe_dir.to_path_buf());
                }
            }

            if let Ok(new_path) = std::env::join_paths(paths) {
                builder.env("PATH", new_path);
            }
        }

        for (key, value) in &state_entry.env {
            builder.env(key, value);
        }

        #[cfg(unix)]
        let pgid = pair.master.process_group_leader();

        let mut child = pair
            .slave
            .spawn_command(builder)
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to spawn command in PTY: {e}"),
            })?;
        let child_pid = child.process_id();
        info!(
            "Spawned PTY child for channel {:?}: command={:?}, pid={:?}",
            channel, command, child_pid
        );
        if command.is_none() {
            info!(
                "Registering PRIMARY shell PID {:?} for session state",
                child_pid
            );
            self.shell_state.set_shell_pid(child_pid);
        } else {
            info!(
                "Exec command PID {:?} started (not registering as primary session PID)",
                child_pid
            );
        }

        let killer = child.clone_killer();

        // Reason: `reader` is mutated on some platforms (e.g. Windows) but not all.
        #[allow(unused_mut)]
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to clone PTY reader: {e}"),
            })?;
        let shutdown = CancellationToken::new();

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to take PTY writer: {e}"),
            })?;

        let (pty_tx, mut pty_rx) = tokio::sync::mpsc::channel::<Bytes>(256);
        // Clone used only by the Windows reader thread to inject automatic
        // replies into the PTY input stream (e.g. the cursor-position report
        // that cmd.exe blocks on before it will run anything).
        #[cfg(not(unix))]
        let pty_tx_reader = pty_tx.clone();

        // Dedicated writer thread: avoids a spawn_blocking + Vec allocation
        // per frame. The thread blocks on the bounded channel and writes
        // directly to the PTY handle. Channel close (all Senders dropped)
        // signals shutdown.
        let writer_channel_id = channel;
        std::thread::Builder::new()
            .name(format!("pty-writer-{channel:?}"))
            .spawn(move || {
                let mut writer = writer;
                while let Some(data) = pty_rx.blocking_recv() {
                    if writer.write_all(&data).is_err() {
                        break;
                    }
                    let _ = writer.flush();
                }
                debug!(
                    "PTY writer thread finished for channel {:?}",
                    writer_channel_id
                );
            })
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to spawn PTY writer thread: {e}"),
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
        // Shell output counts as channel traffic for the idle timeout, so the
        // reader loops refresh the timestamp on every forwarded chunk.
        let idle_for_task = self.idle.clone();
        let idle_timeout_for_task = self.idle_timeout;

        #[cfg(unix)]
        let task_shutdown = shutdown.clone();

        // Clone for the spawned task (Windows only needs it to drop on child exit).
        let task_master = shared_master.clone();

        state_entry.process = Some(RunningPty {
            master: shared_master,
            pty_tx: Some(pty_tx),
            killer,
            pid: child_pid,
            #[cfg(unix)]
            pgid,
            shutdown,
            // ConPTY never echoes input back itself. Mirror keystrokes when
            // the client negotiated ECHO and the shell does not render its
            // own line (cmd.exe). PowerShell/PSReadLine draws input itself,
            // and Unix kernels echo in the line discipline.
            #[cfg(windows)]
            server_echo: state_entry.pty.echo && !windows_shell_self_echoes(),
            #[cfg(unix)]
            server_echo: false,
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
                pid: child_pid.unwrap_or(0),
                shell_state,
                channels: channels_ref,
                idle: idle_for_task.clone(),
            };

            let handle_for_task = handle.clone();
            let mut reader = reader;

            let reader_done = CancellationToken::new();

            #[cfg(unix)]
            let reader_future = async {
                if let Some(fd) = maybe_fd {
                    use tokio::io::unix::AsyncFd;

                    if let Ok(async_fd) = AsyncFd::new(RawFdWrapper(fd)) {
                        // Pooled read buffer: `split_to(n).freeze()` produces an
                        // owned Bytes in O(1) without copying, and the backing
                        // allocation stays alive via `read_buf` across iterations.
                        // Capacity is 2× the read quantum so `resize` after a
                        // split never reallocates.
                        let mut read_buf = BytesMut::with_capacity(8192 * 2);
                        loop {
                            tokio::select! {
                                biased;
                                () = task_shutdown.cancelled() => {
                                    debug!("PTY reader task cancelled for channel {:?}", channel);
                                    break;
                                }
                                res = async_fd.readable() => {
                                    match res {
                                        Ok(mut guard) => {
                                            read_buf.resize(8192, 0);
                                            match reader.read(&mut read_buf[..]) {
                                                Ok(0) => {
                                                    debug!("PTY reader received EOF for channel {:?}", channel);
                                                    break;
                                                }
                                                Ok(n) => {
                                                    guard.retain_ready();
                                                    debug!("PTY reader read {} bytes from channel {:?}", n, channel);
                                                    let chunk = read_buf.split_to(n).freeze();
                                                    if let Err(e) = handle_for_task.data(channel, chunk).await {
                                                        warn!("PTY reader failed to send data to channel {:?}: {:?}", channel, e);
                                                        break;
                                                    }
                                                    if !idle_timeout_for_task.is_zero() {
                                                        Self::touch_idle(&idle_for_task, channel);
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
                // Grab the runtime handle before entering the blocking thread so
                // we can drive async SSH sends from within it. This is explicitly
                // supported inside spawn_blocking threads by Tokio.
                let rt_handle = tokio::runtime::Handle::current();
                let reader_done_task = reader_done_cloned;
                let idle_for_reader = idle_for_task;
                let idle_timeout_for_reader = idle_timeout_for_task;

                info!(
                    "Spawning blocking PTY reader thread for channel {:?}",
                    channel
                );

                // Previously this used an intermediate mpsc channel:
                //   blocking reader -> channel -> async forwarder -> SSH
                // Every 8 KiB chunk passed through two async hops. For large
                // transfers (e.g. 200 MB) that added ~25 000 unnecessary
                // round-trips through the channel.
                //
                // Now we read and send in the same blocking thread using
                // Handle::block_on, eliminating the intermediate queue entirely.
                tokio::task::spawn_blocking(move || {
                    // Pooled read buffer: `split_to(n).freeze()` produces an
                    // owned Bytes in O(1) without copying, and the backing
                    // allocation stays alive via `read_buf` across iterations.
                    // Capacity is 2× the read quantum so `resize` after a
                    // split never reallocates.
                    let mut read_buf = BytesMut::with_capacity(8192 * 2);
                    // DSR state machine for detecting the cursor-position query
                    // `ESC [ 6 n` that cmd.exe issues on startup and blocks on
                    // until a terminal emulator replies. Tracks the last few
                    // input bytes in O(1) state transitions instead of scanning
                    // every output byte with a sliding Vec (which re-shifts the
                    // window on each byte).
                    #[cfg(not(unix))]
                    let mut dsr_state: u8 = 0;
                    // Auto-reply window: shells issue their DSR query at startup
                    // and block until answered. Reply only inside this window —
                    // a late reply injected onto a live prompt lands as literal
                    // keystrokes and corrupts the command line (observed: a
                    // late reply turned `exit` into `xit` on PowerShell).
                    // Later queries are answered by the real client terminal.
                    #[cfg(not(unix))]
                    let reader_started = std::time::Instant::now();
                    loop {
                        if reader_done_task.is_cancelled() {
                            info!(
                                "PTY reader thread received cancellation for channel {:?}",
                                channel
                            );
                            break;
                        }
                        read_buf.resize(8192, 0);
                        match reader.read(&mut read_buf[..]) {
                            Ok(0) => {
                                info!("PTY reader thread received EOF for channel {:?}", channel);
                                break;
                            }
                            Ok(n) => {
                                trace!(
                                    "PTY reader thread read {} bytes for channel {:?}",
                                    n, channel
                                );
                                #[cfg(not(unix))]
                                {
                                    for &b in &read_buf[..n] {
                                        // Any ESC restarts the match from its own
                                        // position, so `ESC [ 6 n` is still detected
                                        // when interleaved with other escape output.
                                        if b == 0x1b {
                                            dsr_state = 1;
                                        } else {
                                            match dsr_state {
                                                1 if b == b'[' => dsr_state = 2,
                                                2 if b == b'6' => dsr_state = 3,
                                                3 if b == b'n' => {
                                                    // Reply with a cursor position report.
                                                    // The row/col don't matter here; the
                                                    // shell only needs a response to unblock.
                                                    if reader_started.elapsed()
                                                        < DSR_AUTOREPLY_WINDOW
                                                    {
                                                        let _ = pty_tx_reader.try_send(
                                                            Bytes::from_static(b"\x1b[1;1R"),
                                                        );
                                                    }
                                                    dsr_state = 0;
                                                }
                                                _ => dsr_state = 0,
                                            }
                                        }
                                    }
                                }
                                // Drive the SSH send directly from this thread.
                                // block_on is safe here because spawn_blocking
                                // threads are not Tokio worker threads.
                                let chunk = read_buf.split_to(n).freeze();
                                if rt_handle
                                    .block_on(handle_for_task.data(channel, chunk))
                                    .is_err()
                                {
                                    break;
                                }
                                if !idle_timeout_for_reader.is_zero() {
                                    Self::touch_idle(&idle_for_reader, channel);
                                }
                            }
                            Err(e) => {
                                info!("PTY reader thread error on channel {:?}: {}", channel, e);
                                break;
                            }
                        }
                    }
                })
                .await
                .unwrap_or_else(|e| warn!("PTY reader task failed on channel {channel:?}: {e}"));
            };

            let mut child_waiter = tokio::task::spawn_blocking(move || {
                info!(
                    "Waiting for child process {:?} for channel {:?}",
                    child_pid, channel
                );
                let res = child.wait().map_or_else(
                    |e| {
                        warn!("Failed to wait for child process {child_pid:?}: {e}");
                        255
                    },
                    |s| s.exit_code(),
                );
                info!(
                    "Child process {:?} for channel {:?} exited with code {}",
                    child_pid, channel, res
                );
                res
            });

            tokio::pin!(reader_future);

            let exit_status = tokio::select! {
                status = &mut child_waiter => {
                    let status = status.unwrap_or(255);

                    // Child exited. On Windows, we still need to force the blocking
                    // reader to exit by dropping the master handle.
                    #[cfg(not(unix))]
                    {
                        if let Ok(mut guard) = task_master.lock() {
                            drop(guard.take());
                        }
                    }
                    #[cfg(unix)]
                    {
                        let _ = &task_master;
                    }

                    // Now wait for the reader future to complete naturally (EOF)
                    // with a small timeout to avoid hanging if the PTY stays open.
                    let _ = tokio::time::timeout(std::time::Duration::from_millis(500), &mut reader_future).await;

                    status
                }
                () = &mut reader_future => {
                    // Reader finished (EOF). Wait for child to get exit status.
                    child_waiter.await.unwrap_or(255)
                }
            };

            // Ensure reader is cancelled if we are still hanging here
            reader_done.cancel();

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

    pub(super) async fn write_channel_data(&self, channel: ChannelId, data: &[u8]) {
        debug!(
            bytes = data.len(),
            ?channel,
            "writing SSH data bytes into PTY channel"
        );
        // Clone the sender out of the lock so the std::sync guard is not held
        // across the await (russh handler futures must be Send).
        let pty_tx = {
            let mut channels = self.lock_channels();
            channels
                .get_mut(&channel)
                .and_then(|state_entry| state_entry.process.as_mut())
                .and_then(|process| process.pty_tx.clone())
        };
        if let Some(pty_tx) = pty_tx {
            let _ = pty_tx.send(Bytes::copy_from_slice(data)).await;
        }
    }

    /// Whether this channel wants server-side input echo (terminal ECHO on a
    /// shell that does not render its own input). Lock is released before any
    /// await performed by the caller.
    pub(super) fn channel_server_echo(&self, channel: ChannelId) -> bool {
        let channels = self.lock_channels();
        channels
            .get(&channel)
            .and_then(|state_entry| state_entry.process.as_ref())
            .is_some_and(|process| process.server_echo)
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
            process.pty_tx.take();
        }
    }

    pub(super) fn close_channel(&self, channel: ChannelId) {
        let mut channels = self.lock_channels();
        if let Some(mut state_entry) = channels.remove(&channel)
            && let Some(mut process) = state_entry.process.take()
        {
            process.shutdown.cancel();
            self.shell_state.clear_shell_pid_if_matches(process.pid);
            process.pty_tx.take();
            let _ = process.killer.kill();
            // Drop the master PTY handle to ensure any ConPTY session is fully
            // torn down, releasing all associated OS resources.
            if let Ok(mut guard) = process.master.lock() {
                drop(guard.take());
            }
        }
    }

    pub(super) fn forward_signal(&self, channel: ChannelId, signal: &russh::Sig) {
        #[cfg(unix)]
        {
            let channels = self.lock_channels();
            if let Some(state_entry) = channels.get(&channel)
                && let Some(process) = state_entry.process.as_ref()
            {
                if let (Some(pgid), Some(sig)) = (process.pgid, crate::session::map_sig(signal)) {
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
            use windows_sys::Win32::System::Console::{
                CTRL_BREAK_EVENT, CTRL_C_EVENT, GenerateConsoleCtrlEvent,
            };
            let channels = self.lock_channels();
            if let Some(state_entry) = channels.get(&channel)
                && let Some(process) = state_entry.process.as_ref()
            {
                let pid = process.pid;
                let event = match signal {
                    russh::Sig::INT => Some(CTRL_C_EVENT),
                    russh::Sig::QUIT | russh::Sig::ABRT => Some(CTRL_BREAK_EVENT),
                    _ => None,
                };

                if let Some(event) = event {
                    info!(
                        "Forwarding signal {:?} to PTY process group (PID {:?})",
                        signal, pid
                    );

                    // Path A: The "Legacy" API.
                    // This explicitly fails when Irosh is running as a Windows Service (Session 0)
                    // because there is no attached console window. We still attempt it as a
                    // best-effort fallback for interactive testing runs.
                    if let Some(pid) = pid {
                        // SAFETY: `GenerateConsoleCtrlEvent` is a documented Win32 API.
                        // `pid` is a valid process ID obtained during PTY allocation.
                        unsafe {
                            GenerateConsoleCtrlEvent(event, pid);
                        }
                    }

                    // Path B: The "Service" Way (Byte Injection).
                    // This is the actual effective mechanism. By injecting `\x03` into the PTY input stream,
                    // ConPTY translates it into a Ctrl+C event for the child process even in headless
                    // service environments.
                    if matches!(signal, russh::Sig::INT) {
                        if let Some(pty_tx) = process.pty_tx.as_ref() {
                            debug!(
                                "Injecting CTRL+C byte (\\x03) into PTY input stream for channel {:?}",
                                channel
                            );
                            let _ = pty_tx.try_send(Bytes::from_static(b"\x03"));
                        }
                    } else if matches!(signal, russh::Sig::QUIT | russh::Sig::ABRT) {
                        if let Some(pty_tx) = process.pty_tx.as_ref() {
                            debug!(
                                "Injecting CTRL+BREAK byte (\\x1c) into PTY input stream for channel {:?}",
                                channel
                            );
                            let _ = pty_tx.try_send(Bytes::from_static(b"\x1c"));
                        }
                    }
                }
            }
        }
    }
}

/// Builds the bytes the server echoes back for client input when
/// `server_echo` is on (cmd.exe sessions: ConPTY never echoes by itself).
///
/// Faithful to TTY ECHO semantics rather than a raw copy:
/// - printable ASCII, space, tab and UTF-8 bytes echo as-is;
/// - `\r` echoes as `\n` (the shell's own `\r\n` supplies the carriage
///   return, avoiding a doubled newline);
/// - escape sequences (arrow keys, Delete, `ESC M`, ...) are swallowed:
///   the shell consumes those keys silently and repaints the line itself,
///   so echoing them would print `^[[A` glyph garbage;
/// - other C0 controls and DEL are swallowed: the shell announces their
///   effect through its own output (erase redraws, `^C` on interrupt).
pub(super) fn filter_echo_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b == 0x1b {
            // Skip an escape sequence. A truncated tail is dropped; the
            // remainder of a split sequence is swallowed on the next call
            // the same way.
            i += 1;
            if i >= data.len() {
                break;
            }
            match data[i] {
                b'[' => {
                    // CSI: parameter/intermediate bytes (0x20-0x3F), then one
                    // final byte (0x40-0x7E). Anything else ends the sequence.
                    i += 1;
                    while i < data.len() {
                        let f = data[i];
                        i += 1;
                        if (0x40..=0x7e).contains(&f) || !(0x20..=0x3f).contains(&f) {
                            break;
                        }
                    }
                }
                b']' => {
                    // OSC: consume until BEL or ST.
                    i += 1;
                    while i < data.len() {
                        let f = data[i];
                        i += 1;
                        if f == 0x07 {
                            break;
                        }
                        if f == 0x1b {
                            if i < data.len() && data[i] == b'\\' {
                                i += 1;
                            }
                            break;
                        }
                    }
                }
                b'(' | b')' | b'#' => {
                    // Two-byte sequences: consume the designator too.
                    i += 1;
                    if i < data.len() {
                        i += 1;
                    }
                }
                // Single-char escape (`ESC M`, `ESC =`, ...): consume it.
                _ => {
                    i += 1;
                }
            }
        } else if b == b'\r' {
            out.push(b'\n');
            i += 1;
        } else if b == b'\n' || b == b'\t' || b == b' ' || b.is_ascii_graphic() || b >= 0x80 {
            out.push(b);
            i += 1;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(windows)]
fn windows_command_processor() -> String {
    use std::sync::OnceLock;
    static SHELL: OnceLock<String> = OnceLock::new();
    SHELL.get_or_init(detect_windows_shell).clone()
}

/// True when the configured Windows shell renders typed input itself
/// (PowerShell/PSReadLine). The server must not echo in that case or every
/// keystroke would appear twice. cmd.exe has no line editor, so it relies
/// on the peer to echo.
#[cfg(windows)]
fn windows_shell_self_echoes() -> bool {
    let exe = windows_command_processor().to_lowercase();
    exe.contains("powershell") || exe.contains("pwsh")
}

fn build_command(command: Option<&str>) -> CommandBuilder {
    if let Some(command) = command {
        #[cfg(unix)]
        {
            let mut command_builder = CommandBuilder::new("sh");
            command_builder.arg("-lc");
            command_builder.arg(command);
            command_builder
        }
        #[cfg(windows)]
        {
            let exe = windows_command_processor();
            let is_powershell = windows_shell_self_echoes();
            let flag = if is_powershell { "-Command" } else { "/C" };

            // Enforce UTF-8 encoding for the remote session to ensure compatibility with irosh output
            let final_command = if is_powershell {
                format!(
                    "$OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {command}"
                )
            } else {
                format!("chcp 65001 >nul && {command}")
            };

            let mut command_builder = CommandBuilder::new(exe);
            command_builder.arg(flag);
            command_builder.arg(final_command);
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
            let exe = windows_command_processor();
            let is_powershell = windows_shell_self_echoes();

            let mut builder = CommandBuilder::new(exe);
            if is_powershell {
                // For PowerShell, we set the output encoding globally for the session.
                builder.arg("-NoExit");
                builder.arg("-Command");
                builder.arg("$OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::OutputEncoding = [System.Text.Encoding]::UTF8;");
            } else {
                // For CMD, we use /K to run chcp and stay open.
                builder.arg("/K");
                builder.arg("chcp 65001 >nul");
            }
            builder
        }
        #[cfg(not(windows))]
        {
            CommandBuilder::new_default_prog()
        }
    }
}

#[cfg(windows)]
fn detect_windows_shell() -> String {
    use std::path::Path;

    // 1. Try to find PowerShell Core (pwsh.exe) in PATH
    if let Ok(output) = std::process::Command::new("where.exe")
        .arg("pwsh.exe")
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            if !path.is_empty() && Path::new(&path).exists() {
                return path;
            }
        }
    }

    // 2. Try to find Windows PowerShell in standard location
    if let Ok(systemroot) = std::env::var("SystemRoot") {
        let ps_path = std::path::PathBuf::from(&systemroot)
            .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        if ps_path.exists() {
            return ps_path.to_string_lossy().into_owned();
        }
    }

    // 3. Fallback to COMSPEC or cmd.exe
    if let Ok(comspec) = std::env::var("COMSPEC") {
        if Path::new(&comspec).is_absolute() && Path::new(&comspec).exists() {
            return comspec;
        }
    }

    // Absolute fallback
    if let Ok(systemroot) = std::env::var("SystemRoot") {
        return std::path::PathBuf::from(systemroot)
            .join(r"System32\cmd.exe")
            .to_string_lossy()
            .into_owned();
    }
    "C:\\Windows\\System32\\cmd.exe".to_string()
}

#[cfg(test)]
mod echo_tests {
    use super::filter_echo_bytes;

    #[test]
    fn printable_passthrough() {
        assert_eq!(filter_echo_bytes(b"echo A~B"), b"echo A~B");
    }

    #[test]
    fn carriage_return_echoes_as_linefeed() {
        assert_eq!(filter_echo_bytes(b"a\rb"), b"a\nb");
    }

    #[test]
    fn arrow_key_sequence_swallowed() {
        assert!(filter_echo_bytes(b"\x1b[A").is_empty());
    }

    #[test]
    fn delete_key_sequence_swallowed_inline() {
        assert_eq!(filter_echo_bytes(b"ab\x1b[3~cd"), b"abcd");
    }

    #[test]
    fn single_char_escape_swallowed() {
        assert!(filter_echo_bytes(b"\x1bM").is_empty());
    }

    #[test]
    fn control_bytes_swallowed() {
        assert!(filter_echo_bytes(b"\x03").is_empty());
        assert!(filter_echo_bytes(b"\x7f").is_empty());
    }

    #[test]
    fn utf8_passthrough() {
        let input = "héllo ~".as_bytes();
        assert_eq!(filter_echo_bytes(input), input);
    }

    #[test]
    fn trailing_lone_esc_swallowed() {
        assert_eq!(filter_echo_bytes(b"a\x1b"), b"a");
    }

    #[test]
    fn multiparam_csi_swallowed() {
        assert!(filter_echo_bytes(b"\x1b[38;5;196m").is_empty());
        assert_eq!(filter_echo_bytes(b"x\x1b[1;31my"), b"xy");
    }

    #[test]
    fn osc_swallowed() {
        assert!(filter_echo_bytes(b"\x1b]0;title\x07").is_empty());
        assert_eq!(filter_echo_bytes(b"a\x1b]0;t\x1b\\b"), b"ab");
    }
}

//! SSH PTY command execution: spawning, lifecycle, and signal forwarding.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex as StdMutex};

use bytes::{Bytes, BytesMut};
use portable_pty::{ChildKiller, MasterPty, native_pty_system};
use russh::{ChannelId, server};
#[cfg(not(unix))]
use tracing::trace;
use tracing::{debug, info, warn};

use tokio_util::sync::CancellationToken;

use crate::error::{Result, ServerError};
use crate::server::transfer::ConnectionShellState;

use super::ServerHandler;
use super::pty::ChannelState;
use super::shell::build_command;
#[cfg(windows)]
use super::shell::windows_shell_self_echoes;

/// How long after channel open the reader thread auto-answers the shell's
/// DSR cursor-position query (`ESC [ 6 n`). PowerShell can take a couple of
/// seconds to reach its prompt on slow hosts; after the window closes the
/// client terminal answers any further queries itself.
#[cfg(not(unix))]
const DSR_AUTOREPLY_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

/// Upper bound on how long the reader task waits for a PTY child to exit after
/// the PTY has reached EOF. A process that detached from the PTY (redirected
/// its stdio and keeps running) would otherwise block teardown indefinitely.
const CHILD_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Shared ownership of the master PTY handle.
///
/// Both `RunningPty` (for resize operations) and the spawned reader task (for
/// closing the ConPTY on Windows when the child exits) need to access the master.
/// Wrapping it in `Arc<StdMutex<Option<...>>>` allows the task to take and drop the
/// handle without requiring `RunningPty` to be moved into the task.
type SharedMaster = Arc<StdMutex<Option<Box<dyn MasterPty + Send>>>>;

pub(super) struct RunningPty {
    /// Shared master PTY handle. Kept here for `resize` and, on Windows, to allow
    /// the reader task to close the ConPTY when the child exits.
    pub(super) master: SharedMaster,
    pub(super) pty_tx: Option<tokio::sync::mpsc::Sender<Bytes>>,
    pub(super) killer: Box<dyn ChildKiller + Send + Sync>,
    pub(super) pid: Option<u32>,
    #[cfg(unix)]
    pub(super) pgid: Option<libc::pid_t>,
    pub(super) shutdown: CancellationToken,
    /// Mirror client input back to the client (terminal ECHO). Only true on
    /// Windows cmd.exe sessions: ConPTY does not echo itself, while Unix
    /// kernels and PSReadLine render input on their own.
    pub(super) server_echo: bool,
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

/// Terminates the process tree backing a PTY channel.
///
/// On Unix the child is a process-group leader, so `killpg` also reaps any
/// grandchildren the shell spawned (e.g. `sleep`). Killing only the direct
/// child would leave those grandchildren holding the PTY slave open and the
/// reader task parked. The direct kill is retained as a fallback for the rare
/// case where no process group was recorded.
#[cfg(unix)]
fn kill_pty_process(process: &mut RunningPty) {
    if let Some(pgid) = process.pgid {
        // SAFETY: `pgid` is the process group leader returned by the PTY
        // master for this child, so it names this child's process group.
        let result = unsafe { libc::killpg(pgid, libc::SIGKILL) };
        if result != 0 {
            warn!(pgid, error = %std::io::Error::last_os_error(), "PTY process-group kill failed");
        }
        #[cfg(test)]
        eprintln!("PTY teardown group={pgid} kill_result={result}");
    }
    // `ChildKiller::kill` only sends `SIGHUP` on Unix, which a shell may
    // catch or ignore. Back it with a direct `SIGKILL` so the child is
    // guaranteed dead even if the group kill above could not run.
    if let Some(pid) = process.pid {
        // SAFETY: `pid` is the child PID returned by the PTY spawn.
        let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        if result != 0 {
            warn!(pid, error = %std::io::Error::last_os_error(), "PTY child kill failed");
        }
        #[cfg(test)]
        eprintln!("PTY teardown child={pid} kill_result={result}");
    }
    let _ = process.killer.kill();
}

#[cfg(not(unix))]
fn kill_pty_process(process: &mut RunningPty) {
    let _ = process.killer.kill();
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

impl ServerHandler {
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

        let mut child = pair
            .slave
            .spawn_command(builder)
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to spawn command in PTY: {e}"),
            })?;
        let child_pid = child.process_id();
        // `portable_pty` makes the child a session leader (`setsid`) before it
        // execs, so the child's PID is also its process-group ID. Deriving the
        // group from the PID is deterministic; querying `tcgetpgrp` here races
        // the child's `setsid`/`TIOCSCTTY` and can yield a stale group.
        #[cfg(unix)]
        let pgid = child_pid.map(|pid| pid as libc::pid_t);
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
                drop(reader);
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
                #[cfg(test)]
                eprintln!("PTY waiter started child={child_pid:?}");
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
                #[cfg(test)]
                eprintln!("PTY waiter finished child={child_pid:?} code={res}");
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
                    // Reader finished (EOF) but the child may still be alive: a
                    // process can detach from the PTY (`exec cmd </dev/null`) and
                    // keep running after the last slave handle closes. OpenSSH
                    // waits indefinitely for such a child; we bound the wait so a
                    // never-exiting detached process cannot wedge the reader task
                    // (and therefore the channel teardown) forever. The child is
                    // left running, matching sshd semantics for a detached node.
                    match tokio::time::timeout(CHILD_WAIT_TIMEOUT, &mut child_waiter).await {
                        Ok(status) => status.unwrap_or(255),
                        Err(_) => {
                            warn!(
                                "Child process {:?} for channel {:?} did not exit within {}s of PTY EOF; closing channel without waiting",
                                child_pid,
                                channel,
                                CHILD_WAIT_TIMEOUT.as_secs()
                            );
                            255
                        }
                    }
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

    pub(super) fn close_channel(&self, channel: ChannelId) {
        let mut channels = self.lock_channels();
        if let Some(mut state_entry) = channels.remove(&channel)
            && let Some(mut process) = state_entry.process.take()
        {
            process.shutdown.cancel();
            self.shell_state.clear_shell_pid_if_matches(process.pid);
            process.pty_tx.take();
            kill_pty_process(&mut process);
            // Drop the master PTY handle to ensure any ConPTY session is fully
            // torn down, releasing all associated OS resources.
            if let Ok(mut guard) = process.master.lock() {
                drop(guard.take());
            }
        }
    }

    /// Tears down every channel tracked by this handler.
    ///
    /// Called once a connection's SSH session has ended. russh does not expose a
    /// "connection closed" hook, so when a peer vanishes abruptly (no
    /// `CHANNEL_CLOSE`, no clean disconnect) the channel state — and with it the
    /// spawned PTY child processes, their reader tasks, and the writer threads —
    /// would otherwise never be reaped. Sweeping here guarantees the connection's
    /// OS resources are released.
    pub(crate) fn terminate_all_channels(&self) {
        let stale: Vec<ChannelId> = self.lock_channels().keys().copied().collect();
        if stale.is_empty() {
            return;
        }
        debug!(
            "Terminating {} lingering PTY channel(s) after session end",
            stale.len()
        );
        for channel in stale {
            self.close_channel(channel);
        }
    }

    /// Returns the PIDs of live PTY processes currently tracked by this handler.
    ///
    /// Test-only observability into channel teardown.
    #[cfg(all(test, unix))]
    pub(crate) fn active_process_pids(&self) -> Vec<u32> {
        self.lock_channels()
            .values()
            .filter_map(|state| state.process.as_ref().and_then(|process| process.pid))
            .collect()
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

#[cfg(all(test, unix))]
mod process_tree_tests {
    use super::*;
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::time::{Duration, Instant};

    /// Builds a `RunningPty` over a real PTY whose shell spawns a grandchild,
    /// then asserts that `kill_pty_process` reaps the whole process group.
    ///
    /// Regression: `close_channel` used to kill only the direct child. A shell
    /// that had spawned background work left that work running and holding the
    /// PTY slave, so the reader task never saw EOF.
    #[test]
    fn kill_pty_process_reaps_grandchildren_in_group() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild.pid");

        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize::default()).unwrap();
        let mut cmd = CommandBuilder::new("sh");
        cmd.arg("-c");
        cmd.arg(format!(
            "sleep 3600 & echo $! > '{}'; wait",
            pid_file.display()
        ));
        let mut child = pair.slave.spawn_command(cmd).unwrap();

        // Wait until the grandchild PID is recorded.
        let grandchild_pid = {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Ok(raw) = std::fs::read_to_string(&pid_file)
                    && let Ok(pid) = raw.trim().parse::<i32>()
                {
                    break pid;
                }
                assert!(Instant::now() < deadline, "grandchild pid never appeared");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        assert_eq!(
            // SAFETY: signal 0 performs only an existence/permission check and
            // does not deliver a signal; `grandchild_pid` is a live pid we read.
            unsafe { libc::kill(grandchild_pid, 0) },
            0,
            "grandchild should be alive before teardown"
        );

        let pgid = pair.master.process_group_leader();
        let master: SharedMaster = Arc::new(StdMutex::new(Some(pair.master)));
        let mut process = RunningPty {
            master,
            pty_tx: None,
            killer: child.clone_killer(),
            pid: child.process_id(),
            pgid,
            shutdown: CancellationToken::new(),
            server_echo: false,
        };

        kill_pty_process(&mut process);

        // Direct child is reaped promptly. Its exit code is platform-dependent
        // for a signal kill, so only the reaping itself is asserted here.
        let _ = child.wait().unwrap();

        // The grandchild is only reaped via the process group. Some platforms do
        // not expose a PTY process group (`process_group_leader()` is `None`);
        // there `kill_pty_process` falls back to killing the direct child alone,
        // so the group assertion only applies when a group was recorded.
        if pgid.is_some() {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                // SAFETY: signal 0 performs only an existence/permission check
                // and does not deliver a signal; `grandchild_pid` is a live pid.
                let alive = unsafe { libc::kill(grandchild_pid, 0) } == 0;
                if !alive {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "grandchild {grandchild_pid} survived process-group kill"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;
use std::sync::{Arc, MutexGuard};

use tokio::process::Command;
use tracing::warn;

use crate::error::{Result, ServerError};
use crate::server::shell_access::{configure_live_shell_context, resolve_process_cwd};

#[derive(Clone, Debug, Default)]
pub(crate) struct ConnectionShellState {
    shell_pid: Arc<StdMutex<Option<u32>>>,
    cached_cwd: Arc<StdMutex<Option<(std::path::PathBuf, std::time::Instant)>>>,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) state_root: PathBuf,
}

impl ConnectionShellState {
    pub(crate) fn new(state_root: PathBuf) -> Self {
        Self {
            shell_pid: Arc::new(StdMutex::new(None)),
            cached_cwd: Arc::new(StdMutex::new(None)),
            state_root,
        }
    }

    pub(crate) fn shell_pid(&self) -> Option<u32> {
        *self.lock_shell_pid()
    }

    pub(crate) fn set_shell_pid(&self, pid: Option<u32>) {
        let mut guard = self.lock_shell_pid();
        tracing::info!("Connection state update: Shell PID registered as {:?}", pid);
        *guard = pid;
    }

    pub(crate) fn clear_shell_pid_if_matches(&self, pid: Option<u32>) {
        let mut guard = self.lock_shell_pid();
        if *guard == pid {
            tracing::info!("Connection state update: Clearing shell PID {:?}", pid);
            *guard = None;
        } else {
            tracing::debug!(
                "Not clearing shell PID; current={:?}, requested_clear={:?}",
                *guard,
                pid
            );
        }
    }

    fn lock_shell_pid(&self) -> MutexGuard<'_, Option<u32>> {
        match self.shell_pid.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("shell pid state mutex poisoned; recovering inner state");
                poisoned.into_inner()
            }
        }
    }
}

/// The context in which a remote operation (like file transfer) is executed.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ShellContext {
    /// An operation tied to a live interactive shell process.
    /// On Linux, this allows joining the shell's namespaces.
    Live { pid: u32 },
    /// An operation executed in the server's own process context.
    Stateless,
}

impl ShellContext {
    /// Returns the context from the current connection state.
    pub(super) fn from_state(shell_state: &ConnectionShellState) -> Self {
        let pid = shell_state.shell_pid();
        match pid {
            Some(pid) => {
                tracing::info!("Transfer context: Live (shell PID {})", pid);
                Self::Live { pid }
            }
            None => {
                tracing::info!("Transfer context: Stateless (no active shell PID)");
                Self::Stateless
            }
        }
    }

    /// Configures a command to run within this context.
    pub(super) fn configure(self, command: &mut Command) {
        if let Self::Live { pid } = self {
            configure_live_shell_context(command, pid);
        }
    }

    /// Resolves the current working directory for this context.
    pub(super) async fn cwd(self, shell_state: &ConnectionShellState) -> Result<PathBuf> {
        match self {
            Self::Live { pid } => {
                // Check cache first
                if let Ok(guard) = shell_state.cached_cwd.lock() {
                    if let Some((path, instant)) = guard.as_ref() {
                        if instant.elapsed() < std::time::Duration::from_secs(2) {
                            return Ok(path.clone());
                        }
                    }
                }

                let fallback_home = self
                    .home_dir(shell_state)
                    .unwrap_or_else(|| PathBuf::from("."));
                let path = resolve_process_cwd(pid, fallback_home).await?;

                // Update cache
                if let Ok(mut guard) = shell_state.cached_cwd.lock() {
                    *guard = Some((path.clone(), std::time::Instant::now()));
                }

                tracing::debug!(
                    "Resolved live shell CWD for PID {}: {}",
                    pid,
                    path.display()
                );
                Ok(path)
            }
            Self::Stateless => {
                let home = self
                    .home_dir(shell_state)
                    .ok_or_else(|| ServerError::ShellError {
                        details: "could not determine server home directory".to_string(),
                    })?;
                tracing::debug!("Resolved stateless CWD (home): {}", home.display());
                Ok(home)
            }
        }
    }

    pub(super) async fn path_exists(self, path: &str) -> Result<bool> {
        #[cfg(target_os = "linux")]
        if let Self::Live { .. } = self {
            let mut command = Command::new("test");
            command.arg("-e").arg(path);
            self.configure(&mut command);

            let status = command
                .status()
                .await
                .map_err(|e| ServerError::ShellError {
                    details: format!("failed to probe remote path existence: {e}"),
                })?;
            return Ok(status.success());
        }

        Ok(tokio::fs::metadata(path).await.is_ok())
    }

    pub(super) async fn path_missing(self, path: &str) -> Result<bool> {
        Ok(!self.path_exists(path).await?)
    }

    pub(super) async fn is_dir(self, path: &str) -> Result<bool> {
        #[cfg(target_os = "linux")]
        if let Self::Live { .. } = self {
            let mut command = Command::new("test");
            command.arg("-d").arg(path);
            self.configure(&mut command);

            let status = command
                .status()
                .await
                .map_err(|e| ServerError::ShellError {
                    details: format!("failed to probe remote path directory status: {e}"),
                })?;
            return Ok(status.success());
        }

        let meta = match tokio::fs::metadata(path).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        Ok(meta.is_dir())
    }

    pub(super) async fn create_dir_all(self, path: &Path) -> Result<bool> {
        #[cfg(target_os = "linux")]
        if let Self::Live { .. } = self {
            let mut command = Command::new("mkdir");
            command.arg("-p").arg(path);
            self.configure(&mut command);
            let status = command.status().await;
            return Ok(status.map(|s| s.success()).unwrap_or(false));
        }

        tokio::fs::create_dir_all(path).await?;
        Ok(true)
    }

    pub(super) async fn remove_file_if_present(self, path: &str) -> Result<()> {
        #[cfg(target_os = "linux")]
        if let Self::Live { .. } = self {
            let mut command = Command::new("rm");
            command.arg("-f").arg(path);
            self.configure(&mut command);
            let _ = command.status().await;
            return Ok(());
        }

        let _ = tokio::fs::remove_file(path).await;
        Ok(())
    }

    pub(super) async fn rename(self, from: &str, to: &str) -> Result<bool> {
        #[cfg(target_os = "linux")]
        if let Self::Live { .. } = self {
            let mut command = Command::new("mv");
            command.arg(from).arg(to);
            self.configure(&mut command);
            let status = command.status().await;
            return Ok(status.map(|s| s.success()).unwrap_or(false));
        }

        tokio::fs::rename(from, to).await?;
        Ok(true)
    }

    pub(super) async fn chmod(self, path: &str, mode: u32) {
        #[cfg(target_os = "linux")]
        if let Self::Live { .. } = self {
            let mut command = Command::new("chmod");
            command.arg(format!("{:o}", mode)).arg(path);
            self.configure(&mut command);
            let _ = command.status().await;
            return;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).await;
        }
        #[cfg(not(unix))]
        {
            let _ = (path, mode);
        }
    }

    fn home_dir(self, _shell_state: &ConnectionShellState) -> Option<PathBuf> {
        #[cfg(unix)]
        {
            std::env::var_os("HOME").map(PathBuf::from)
        }
        #[cfg(windows)]
        {
            // If running as a service, USERPROFILE points to systemprofile.
            // We can infer the actual user home by looking at the state directory.
            let profile = std::env::var_os("USERPROFILE").map(PathBuf::from);
            if let Some(p) = &profile {
                if p.to_string_lossy().to_lowercase().contains("systemprofile") {
                    // We are likely a service. Deriving home from state_root.
                    // State root is usually: C:\Users\Ghost\.irosh\server
                    // We want: C:\Users\Ghost
                    let mut current = _shell_state.state_root.as_path();
                    while let Some(parent) = current.parent() {
                        if current.file_name().and_then(|n| n.to_str()) == Some(".irosh") {
                            return Some(parent.to_path_buf());
                        }
                        current = parent;
                    }
                }
            }
            profile.or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        }
    }
}

impl ShellContext {
    /// Resolves a raw remote path string into an absolute PathBuf.
    ///
    /// If the path is relative, it is resolved against the current working
    /// directory of this context (either the live shell's CWD or the server home).
    pub(crate) async fn resolve_path(
        self,
        raw: &str,
        shell_state: &ConnectionShellState,
    ) -> Result<PathBuf> {
        if raw.trim().is_empty() {
            return Err(ServerError::TransferFailed {
                failure: crate::transport::transfer::TransferFailure::new(
                    crate::transport::transfer::TransferFailureCode::PathInvalid,
                    "transfer path is empty",
                ),
            }
            .into());
        }

        let path = Path::new(raw);
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }

        if raw == "~" {
            return self.home_dir(shell_state).ok_or_else(|| {
                ServerError::ShellError {
                    details: "could not determine server home directory for ~ expansion"
                        .to_string(),
                }
                .into()
            });
        }

        if let Some(home_relative) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
            let home = self
                .home_dir(shell_state)
                .ok_or_else(|| ServerError::ShellError {
                    details: "could not determine server home directory for ~/ expansion"
                        .to_string(),
                })?;
            return Ok(home.join(home_relative));
        }

        // Relative path: resolve against CWD.
        let base = self.cwd(shell_state).await?;
        let full = base.join(path);
        tracing::info!("Resolved remote path: '{}' -> '{}'", raw, full.display());
        Ok(full)
    }
}

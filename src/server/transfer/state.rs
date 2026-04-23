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
}

impl ConnectionShellState {
    pub(crate) fn new() -> Self {
        Self::default()
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
    pub(super) async fn cwd(self) -> Result<PathBuf> {
        match self {
            Self::Live { pid } => {
                let path = resolve_process_cwd(pid).await?;
                tracing::debug!(
                    "Resolved live shell CWD for PID {}: {}",
                    pid,
                    path.display()
                );
                Ok(path)
            }
            Self::Stateless => {
                let home = server_home_dir().ok_or_else(|| ServerError::ShellError {
                    details: "could not determine server home directory".to_string(),
                })?;
                tracing::debug!("Resolved stateless CWD (home): {}", home.display());
                Ok(home)
            }
        }
    }

    pub(super) async fn path_exists(self, path: &str) -> Result<bool> {
        let mut command = Command::new("test");
        command.arg("-e").arg(path);
        self.configure(&mut command);

        let status = command
            .status()
            .await
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to probe remote path existence: {e}"),
            })?;
        Ok(status.success())
    }

    pub(super) async fn path_missing(self, path: &str) -> Result<bool> {
        let mut command = Command::new("test");
        command.arg("!").arg("-e").arg(path);
        self.configure(&mut command);

        let status = command
            .status()
            .await
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to probe remote path absence: {e}"),
            })?;
        Ok(status.success())
    }

    pub(super) async fn create_dir_all(self, path: &Path) -> Result<bool> {
        let mut command = Command::new("mkdir");
        command.arg("-p").arg("--").arg(path);
        self.configure(&mut command);

        let status = command
            .status()
            .await
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to spawn mkdir: {e}"),
            })?;
        Ok(status.success())
    }

    pub(super) async fn remove_file_if_present(self, path: &str) {
        let mut command = Command::new("rm");
        command.arg("-f").arg("--").arg(path);
        self.configure(&mut command);
        let _ = command.status().await;
    }

    pub(super) async fn rename(self, from: &str, to: &str) -> Result<bool> {
        let mut command = Command::new("mv");
        command.arg("-f").arg("--").arg(from).arg(to);
        self.configure(&mut command);

        let status = command
            .status()
            .await
            .map_err(|e| ServerError::ShellError {
                details: format!("failed to spawn mv for atomic rename: {e}"),
            })?;
        Ok(status.success())
    }

    pub(super) async fn chmod(self, path: &str, mode: u32) {
        let mut command = Command::new("chmod");
        command.arg(format!("{:o}", mode)).arg("--").arg(path);
        self.configure(&mut command);
        let _ = command.status().await;
    }
}

impl ShellContext {
    /// Resolves a raw remote path string into an absolute PathBuf.
    ///
    /// If the path is relative, it is resolved against the current working
    /// directory of this context (either the live shell's CWD or the server home).
    pub(crate) async fn resolve_path(self, raw: &str) -> Result<PathBuf> {
        if raw.trim().is_empty() {
            return Err(ServerError::TransferFailed {
                details: "transfer path is empty".to_string(),
            }
            .into());
        }

        let path = Path::new(raw);
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }

        if raw == "~" {
            return server_home_dir().ok_or_else(|| {
                ServerError::ShellError {
                    details: "could not determine server home directory for ~ expansion"
                        .to_string(),
                }
                .into()
            });
        }

        if let Some(home_relative) = raw.strip_prefix("~/") {
            let home = server_home_dir().ok_or_else(|| ServerError::ShellError {
                details: "could not determine server home directory for ~/ expansion".to_string(),
            })?;
            return Ok(home.join(home_relative));
        }

        // Relative path: resolve against CWD.
        let base = self.cwd().await?;
        let full = base.join(path);
        tracing::info!("Resolved remote path: '{}' -> '{}'", raw, full.display());
        Ok(full)
    }
}

fn server_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

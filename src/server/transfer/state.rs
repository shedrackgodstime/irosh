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
        *self.lock_shell_pid() = pid;
    }

    pub(crate) fn clear_shell_pid_if_matches(&self, pid: Option<u32>) {
        let mut guard = self.lock_shell_pid();
        if *guard == pid {
            *guard = None;
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct LiveShellContext {
    pid: u32,
}

impl LiveShellContext {
    pub(super) fn from_state(shell_state: &ConnectionShellState) -> Option<Self> {
        shell_state.shell_pid().map(|pid| Self { pid })
    }

    pub(super) fn from_pid(pid: Option<u32>) -> Option<Self> {
        pid.map(|pid| Self { pid })
    }

    pub(super) fn pid(self) -> u32 {
        self.pid
    }

    pub(super) fn configure(self, command: &mut Command) {
        configure_live_shell_context(command, self.pid);
    }

    pub(super) async fn cwd(self) -> Result<PathBuf> {
        resolve_process_cwd(self.pid).await
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

pub(crate) fn resolve_remote_path(raw: &str) -> Result<PathBuf> {
    if raw.trim().is_empty() {
        return Err(ServerError::TransferFailed {
            details: "transfer path is empty".to_string(),
        }
        .into());
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else if raw == "~" {
        server_home_dir().ok_or_else(|| {
            ServerError::ShellError {
                details: "could not determine server home directory for ~ expansion".to_string(),
            }
            .into()
        })
    } else if let Some(home_relative) = raw.strip_prefix("~/") {
        let home = server_home_dir().ok_or_else(|| ServerError::ShellError {
            details: "could not determine server home directory for ~/ expansion".to_string(),
        })?;
        Ok(home.join(home_relative))
    } else {
        let home = server_home_dir().unwrap_or_else(std::env::temp_dir);
        Ok(home.join(path))
    }
}

fn server_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

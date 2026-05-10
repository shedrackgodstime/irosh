use std::path::PathBuf;

use tokio::process::Command;
use tokio::task;

use crate::error::{IroshError, Result, ServerError};

pub(crate) async fn resolve_process_cwd(pid: u32) -> Result<PathBuf> {
    task::spawn_blocking(move || {
        #[cfg(target_os = "linux")]
        {
            let link = format!("/proc/{pid}/cwd");
            let cwd = std::fs::read_link(&link).map_err(|e| {
                IroshError::Server(ServerError::ProcessQueryFailed {
                    pid,
                    details: format!("failed to read /proc/{pid}/cwd: {e}"),
                    source: e,
                })
            })?;
            Ok(cwd)
        }
        #[cfg(not(target_os = "linux"))]
        {
            // On Windows and other platforms, we don't have an easy /proc/pid/cwd.
            // We attempt to find a sensible fallback.
            // 1. Try to find the home directory of the current user.
            // 2. Fallback to the daemon's current directory if home is not available or is systemprofile.

            let mut fallback = dirs::home_dir();

            #[cfg(windows)]
            {
                if let Some(h) = &fallback {
                    if h.to_string_lossy().to_lowercase().contains("systemprofile") {
                        // We are a service running as LocalSystem.
                        // Try to find if there's a better directory we can use.
                        fallback = None;
                    }
                }
            }

            let result = fallback
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

            tracing::warn!(
                "Reliable CWD query not available for PID {} on this platform; using fallback: {}",
                pid,
                result.display()
            );
            Ok(result)
        }
    })
    .await
    .map_err(|source| {
        IroshError::Server(ServerError::BlockingTaskFailed {
            operation: "resolve process cwd",
            source,
        })
    })?
}

pub(crate) fn configure_live_shell_context(_command: &mut Command, _pid: u32) {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: `pre_exec` is unsafe because it runs in the child process after `fork` but
        // before `exec`. We must only use async-signal-safe functions. `libc::setns`,
        // `libc::open`, and `libc::close` (used by `File`) are generally considered safe
        // in this context on Linux.
        // We pre-format the paths to avoid allocation inside the `pre_exec` closure.
        let mnt_ns = format!("/proc/{_pid}/ns/mnt");
        let user_ns = format!("/proc/{_pid}/ns/user");

        unsafe {
            _command.pre_exec(move || {
                join_linux_namespace(&mnt_ns, "/proc/self/ns/mnt", libc::CLONE_NEWNS)?;
                join_linux_namespace(&user_ns, "/proc/self/ns/user", libc::CLONE_NEWUSER)?;
                Ok(())
            });
        }
    }
}

#[cfg(target_os = "linux")]
fn join_linux_namespace(ns_path: &str, self_path: &str, nstype: i32) -> std::io::Result<()> {
    use std::fs::File;
    use std::os::unix::io::AsRawFd;

    if namespace_matches(ns_path, self_path)? {
        return Ok(());
    }

    let fd = File::open(ns_path)?;
    // SAFETY: The file descriptor is valid as it was just opened. `nstype` is a valid
    // namespace type constant from libc. Joining a namespace is a privileged operation
    // that the child process must be authorized to perform.
    let res = unsafe { libc::setns(fd.as_raw_fd(), nstype) };
    if res != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn namespace_matches(ns_path: &str, self_path: &str) -> std::io::Result<bool> {
    use std::os::linux::fs::MetadataExt;
    let target = std::fs::metadata(ns_path)?;
    let current = std::fs::metadata(self_path)?;
    Ok(target.st_ino() == current.st_ino())
}

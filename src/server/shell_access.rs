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
            Err(IroshError::Server(ServerError::ProcessQueryFailed {
                pid,
                details: "reliable remote cwd queries are not implemented on this platform yet"
                    .to_string(),
                source: std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "unsupported platform",
                ),
            }))
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

pub(crate) fn configure_live_shell_context(command: &mut Command, pid: u32) {
    #[cfg(target_os = "linux")]
    {
        unsafe {
            command.pre_exec(move || {
                let ns_path = format!("/proc/{pid}/ns/mnt");
                let _ = join_linux_namespace(&ns_path, libc::CLONE_NEWNS);

                let ns_path = format!("/proc/{pid}/ns/user");
                let _ = join_linux_namespace(&ns_path, libc::CLONE_NEWUSER);
                Ok(())
            });
        }
    }
    let _ = pid;
}

#[cfg(target_os = "linux")]
fn join_linux_namespace(ns_path: &str, nstype: i32) -> std::io::Result<()> {
    use std::fs::File;
    use std::os::unix::io::AsRawFd;

    if namespace_matches_current(ns_path)? {
        return Ok(());
    }

    let fd = File::open(ns_path)?;
    let res = unsafe { libc::setns(fd.as_raw_fd(), nstype) };
    if res != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn namespace_matches_current(ns_path: &str) -> std::io::Result<bool> {
    use std::os::linux::fs::MetadataExt;
    let target = std::fs::metadata(ns_path)?;
    let current = std::fs::metadata("/proc/self/ns/mnt")?;
    Ok(target.st_ino() == current.st_ino())
}

use std::path::PathBuf;

use tokio::process::Command;
use tokio::task;

use crate::error::{Result, ServerError};

pub(crate) async fn resolve_process_cwd(pid: u32) -> Result<PathBuf> {
    task::spawn_blocking(move || resolve_process_cwd_blocking(pid))
        .await
        .map_err(|source| ServerError::BlockingTaskFailed {
            operation: "reading live shell cwd",
            source,
        })?
}

fn resolve_process_cwd_blocking(pid: u32) -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let link = PathBuf::from(format!("/proc/{pid}/cwd"));
        let cwd = std::fs::read_link(&link).map_err(|e| ServerError::ProcessQueryFailed {
            pid,
            details: format!("failed to read /proc/{pid}/cwd: {e}"),
        })?;
        return Ok(cwd);
    }

    #[allow(unreachable_code)]
    Err(ServerError::ProcessQueryFailed {
        pid,
        details: "reliable remote cwd queries are not implemented on this platform yet".to_string(),
    }
    .into())
}

pub(crate) fn configure_live_shell_context(command: &mut Command, shell_pid: u32) {
    #[cfg(target_os = "linux")]
    // SAFETY: The closure runs in the freshly forked child before exec. We join
    // the live shell's user and mount namespaces so transfer helpers observe the
    // same filesystem view as the interactive shell process they serve.
    unsafe {
        let user_ns_path = format!("/proc/{shell_pid}/ns/user");
        let mount_ns_path = format!("/proc/{shell_pid}/ns/mnt");
        command.pre_exec(move || {
            join_linux_namespace(&user_ns_path, libc::CLONE_NEWUSER)?;
            join_linux_namespace(&mount_ns_path, libc::CLONE_NEWNS)?;
            Ok(())
        });
    }

    #[cfg(not(target_os = "linux"))]
    let _ = (command, shell_pid);
}

#[cfg(target_os = "linux")]
fn join_linux_namespace(path: &str, namespace: libc::c_int) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::io;

    if namespace_matches_current(path)? {
        return Ok(());
    }

    let c_path = CString::new(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid namespace path"))?;

    // SAFETY: `c_path` is a valid NUL-terminated string for the duration of the call.
    let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: `fd` is a live file descriptor referring to the requested namespace.
    let setns_result = unsafe { libc::setns(fd, namespace) };
    let setns_error = if setns_result != 0 {
        Some(io::Error::last_os_error())
    } else {
        None
    };

    // SAFETY: `fd` was returned from `libc::open` above and has not been closed yet.
    let close_result = unsafe { libc::close(fd) };
    if let Some(err) = setns_error {
        return Err(err);
    }
    if close_result != 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn namespace_matches_current(target_ns_path: &str) -> std::io::Result<bool> {
    let target = std::fs::read_link(std::path::Path::new(target_ns_path))?;
    let current_path = if target_ns_path.contains("/ns/user") {
        "/proc/self/ns/user"
    } else if target_ns_path.contains("/ns/mnt") {
        "/proc/self/ns/mnt"
    } else {
        return Ok(false);
    };
    let current = std::fs::read_link(current_path)?;
    Ok(target == current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(target_os = "linux")]
    async fn resolve_process_cwd_matches_current_working_directory() {
        let expected = std::env::current_dir().unwrap();
        let cwd = resolve_process_cwd(std::process::id()).await.unwrap();
        assert_eq!(cwd, expected);
    }

    #[tokio::test]
    #[cfg(target_os = "linux")]
    async fn resolve_process_cwd_returns_error_for_missing_process() {
        let err = resolve_process_cwd(u32::MAX).await.unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn namespace_matches_current_accepts_self_namespaces() {
        assert!(namespace_matches_current("/proc/self/ns/user").unwrap());
        assert!(namespace_matches_current("/proc/self/ns/mnt").unwrap());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn namespace_matches_current_errors_for_missing_namespace_path() {
        let err = namespace_matches_current("/proc/self/ns/does-not-exist").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}

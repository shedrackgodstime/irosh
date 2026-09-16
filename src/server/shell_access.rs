//! Shell process access and namespace handling.
use std::path::PathBuf;

#[cfg(any(target_os = "linux", target_os = "android"))]
use tokio::process::Command;
use tokio::task;
#[cfg(any(target_os = "linux", target_os = "android", windows))]
use tracing::warn;

use crate::error::{IroshError, Result, ServerError};

/// Resolves the current working directory of a shell process with the given PID.
///
/// Returns `Ok(None)` when the directory genuinely cannot be determined (for
/// example the process cannot be opened or inspected on this platform). It never
/// invents a fallback path: callers decide how to surface the unknown case rather
/// than silently resolving transfers against the wrong directory.
///
/// # Errors
///
/// Returns [`IroshError::Server`] wrapping [`ServerError::BlockingTaskFailed`] if the
/// blocking task panics.
#[must_use]
pub(crate) async fn resolve_process_cwd(pid: u32) -> Result<Option<PathBuf>> {
    task::spawn_blocking(move || {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let link = format!("/proc/{pid}/cwd");
            match std::fs::read_link(&link) {
                Ok(cwd) => Ok(Some(cwd)),
                Err(e) => {
                    warn!(%pid, error = %e, "failed to read /proc/{pid}/cwd");
                    Ok(None)
                }
            }
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::{
                CloseHandle, FALSE, HANDLE, NTSTATUS, UNICODE_STRING,
            };
            use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
            use windows_sys::Win32::System::Threading::{
                OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
            };

            #[repr(C)]
            // Reason: Windows FFI struct field names match the Win32 API naming convention.
            #[allow(non_snake_case)]
            struct PROCESS_BASIC_INFORMATION {
                ExitStatus: NTSTATUS,
                PebBaseAddress: *mut std::ffi::c_void,
                AffinityMask: usize,
                BasePriority: i32,
                UniqueProcessId: usize,
                InheritedFromUniqueProcessId: usize,
            }

            // SAFETY: FFI declaration for the Windows ntdll API. This function
            // is available on all Windows targets and follows the standard
            // system calling convention.
            unsafe extern "system" {
                fn NtQueryInformationProcess(
                    ProcessHandle: HANDLE,
                    ProcessInformationClass: u32,
                    ProcessInformation: *mut std::ffi::c_void,
                    ProcessInformationLength: u32,
                    ReturnLength: *mut u32,
                ) -> NTSTATUS;
            }

            // SAFETY: Windows API calls for process and PEB inspection.
            // We ensure validity by checking handle creation, status codes,
            // and using MaybeUninit for external data structures.
            unsafe {
                let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid);
                if handle == 0 as _ {
                    warn!(%pid, "OpenProcess failed, cannot resolve Windows shell CWD");
                    return Ok(None);
                }

                let mut pbi = std::mem::MaybeUninit::<PROCESS_BASIC_INFORMATION>::uninit();
                let mut ret_len = 0;
                let status = NtQueryInformationProcess(
                    handle,
                    0, // ProcessBasicInformation
                    pbi.as_mut_ptr().cast(),
                    u32::try_from(std::mem::size_of::<PROCESS_BASIC_INFORMATION>())
                        .expect("BUG: PROCESS_BASIC_INFORMATION struct fits in u32"),
                    std::ptr::addr_of_mut!(ret_len),
                );

                if status != 0 {
                    warn!(%pid, status, "NtQueryInformationProcess failed, cannot resolve Windows shell CWD");
                    CloseHandle(handle);
                    return Ok(None);
                }

                let pbi = pbi.assume_init();
                let peb_base = pbi.PebBaseAddress;

                #[cfg(target_pointer_width = "64")]
                let proc_params_offset = 0x20;
                #[cfg(target_pointer_width = "32")]
                let proc_params_offset = 0x10;

                let mut proc_params_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
                let ok = ReadProcessMemory(
                    handle,
                    peb_base.add(proc_params_offset),
                    std::ptr::addr_of_mut!(proc_params_ptr).cast(),
                    std::mem::size_of::<*mut std::ffi::c_void>(),
                    std::ptr::null_mut(),
                );

                if ok == FALSE {
                    warn!(%pid, "ReadProcessMemory(PEB->ProcessParameters) failed, cannot resolve Windows shell CWD");
                    CloseHandle(handle);
                    return Ok(None);
                }

                #[cfg(target_pointer_width = "64")]
                let cur_dir_offset = 0x38;
                #[cfg(target_pointer_width = "32")]
                let cur_dir_offset = 0x24;

                let mut unicode_str = std::mem::MaybeUninit::<UNICODE_STRING>::uninit();
                let ok = ReadProcessMemory(
                    handle,
                    proc_params_ptr.add(cur_dir_offset),
                    unicode_str.as_mut_ptr().cast(),
                    std::mem::size_of::<UNICODE_STRING>(),
                    std::ptr::null_mut(),
                );

                if ok == FALSE {
                    warn!(%pid, "ReadProcessMemory(ProcessParameters->CurrentDirectoryName) failed, cannot resolve Windows shell CWD");
                    CloseHandle(handle);
                    return Ok(None);
                }

                let unicode_str = unicode_str.assume_init();
                let mut buffer = vec![0u16; (unicode_str.Length / 2) as usize];
                let ok = ReadProcessMemory(
                    handle,
                    unicode_str.Buffer.cast(),
                    buffer.as_mut_ptr().cast(),
                    unicode_str.Length as usize,
                    std::ptr::null_mut(),
                );

                CloseHandle(handle);

                if ok == FALSE {
                    warn!(%pid, "ReadProcessMemory(Buffer) failed, cannot resolve Windows shell CWD");
                    Ok(None)
                } else {
                    let path_str = String::from_utf16_lossy(&buffer);
                    Ok(Some(PathBuf::from(path_str)))
                }
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "android", windows)))]
        {
            let _ = pid;
            Ok(None)
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

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn configure_live_shell_context(command: &mut Command, pid: u32) {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // SAFETY: `pre_exec` is unsafe because it runs in the child process after `fork` but
        // before `exec`. We must only use async-signal-safe functions. `libc::setns`,
        // `libc::open`, and `libc::close` (used by `File`) are generally considered safe
        // in this context on Linux.
        // We pre-format the paths to avoid allocation inside the `pre_exec` closure.
        let mnt_ns = format!("/proc/{pid}/ns/mnt");
        let user_ns = format!("/proc/{pid}/ns/user");

        // SAFETY: `pre_exec` is used to configure the child process before it starts.
        // We only use async-signal-safe operations (joining namespaces) inside the closure.
        unsafe {
            command.pre_exec(move || {
                join_linux_namespace(&mnt_ns, "/proc/self/ns/mnt", libc::CLONE_NEWNS)?;
                join_linux_namespace(&user_ns, "/proc/self/ns/user", libc::CLONE_NEWUSER)?;
                Ok(())
            });
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        // No-op on non-Linux targets: neither the namespace setup nor the PID is
        // relevant to how the shell command is launched there.
        let _ = (command, pid);
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
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

#[cfg(any(target_os = "linux", target_os = "android"))]
fn namespace_matches(ns_path: &str, self_path: &str) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let target = std::fs::metadata(ns_path)?;
    let current = std::fs::metadata(self_path)?;
    Ok(target.ino() == current.ino())
}

#[cfg(all(test, target_os = "linux"))]
mod linux_cwd_resolution_tests {
    use super::{namespace_matches, resolve_process_cwd};
    use std::process::{Child, Command};

    /// Spawns a child process pinned to a known working directory and kept
    /// alive so its `/proc/<pid>/cwd` symlink can be inspected while it runs.
    fn spawn_pinned_child(dir: &std::path::Path) -> Child {
        Command::new("sleep")
            .arg("3600")
            .current_dir(dir)
            .spawn()
            .expect("failed to spawn pinned child")
    }

    #[test]
    fn proc_read_resolves_the_childrens_working_directory() {
        let dir = std::env::temp_dir().join(format!(
            "irosh-proc-cwd-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let mut child = spawn_pinned_child(&dir);
        let pid = child.id();

        let rt = tokio::runtime::Runtime::new().expect("failed to start runtime");
        let resolved = rt
            .block_on(resolve_process_cwd(pid))
            .expect("resolve_process_cwd should not error")
            .expect("resolve_process_cwd should resolve the working directory");

        let _ = child.kill();
        let _ = child.wait();

        let dir_canon = std::fs::canonicalize(&dir).unwrap();
        let resolved_canon = std::fs::canonicalize(&resolved).unwrap();
        assert_eq!(
            dir_canon,
            resolved_canon,
            "/proc/{pid}/cwd should match the pinned child's directory (resolved = {})",
            resolved.display()
        );
    }

    #[test]
    fn proc_read_returns_none_for_an_unknown_pid() {
        let rt = tokio::runtime::Runtime::new().expect("failed to start runtime");
        let resolved = rt
            .block_on(resolve_process_cwd(u32::MAX))
            .expect("resolve_process_cwd should not error");
        assert!(resolved.is_none());
    }

    #[test]
    fn namespace_matches_agrees_with_self() {
        let matches_self = namespace_matches("/proc/self/ns/mnt", "/proc/self/ns/mnt")
            .expect("reading own mount namespace should not error");
        assert!(matches_self);
    }

    #[test]
    fn namespace_matches_distinguishes_different_namespaces() {
        let differs = namespace_matches("/proc/self/ns/mnt", "/proc/self/ns/user")
            .expect("reading own namespaces should not error");
        assert!(!differs);
    }

    #[test]
    fn join_linux_namespace_same_namespace_is_a_noop() {
        // The real `setns` path is privileged-only; the same-namespace
        // short-circuit is what every unprivileged Live-context transfer takes.
        let result = super::join_linux_namespace(
            "/proc/self/ns/mnt",
            "/proc/self/ns/mnt",
            libc::CLONE_NEWNS,
        );
        assert!(result.is_ok());
    }
}

#[cfg(all(test, windows))]
mod windows_cwd_resolution_tests {
    use super::resolve_process_cwd;
    use std::process::{Child, Command};
    use std::time::Duration;

    /// Spawns a child process pinned to a known working directory and kept
    /// alive so its PEB can be inspected while it runs.
    fn spawn_pinned_child(dir: &std::path::Path) -> Child {
        Command::new("cmd.exe")
            .arg("/c")
            .arg("ping -n 30 127.0.0.1 >nul")
            .current_dir(dir)
            .spawn()
            .expect("failed to spawn pinned child")
    }

    #[test]
    fn peb_read_resolves_the_childrens_working_directory() {
        let dir = std::env::temp_dir().join(format!(
            "irosh-peb-cwd-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let mut child = spawn_pinned_child(&dir);
        let pid = child.id();
        // Give the child time to initialize its environment block before we
        // walk its PEB.
        std::thread::sleep(Duration::from_millis(500));

        let rt = tokio::runtime::Runtime::new().expect("failed to start runtime");
        let resolved = rt
            .block_on(resolve_process_cwd(pid))
            .expect("resolve_process_cwd should not error")
            .expect("resolve_process_cwd should resolve the working directory");

        let _ = child.kill();
        let _ = child.wait();

        let dir_canon = std::fs::canonicalize(&dir).unwrap();
        let resolved_canon = std::fs::canonicalize(&resolved).unwrap();
        assert_eq!(
            dir_canon,
            resolved_canon,
            "PEB CWD should match the pinned child's directory (resolved = {})",
            resolved.display()
        );
    }
}

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
            struct PROCESS_BASIC_INFORMATION {
                ExitStatus: NTSTATUS,
                PebBaseAddress: *mut std::ffi::c_void,
                AffinityMask: usize,
                BasePriority: i32,
                UniqueProcessId: usize,
                InheritedFromUniqueProcessId: usize,
            }

            unsafe extern "system" {
                fn NtQueryInformationProcess(
                    ProcessHandle: HANDLE,
                    ProcessInformationClass: u32,
                    ProcessInformation: *mut std::ffi::c_void,
                    ProcessInformationLength: u32,
                    ReturnLength: *mut u32,
                ) -> NTSTATUS;
            }

            let handle =
                unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid) };
            if handle == 0 as _ {
                return Ok(fallback_windows());
            }

            let mut pbi = std::mem::MaybeUninit::<PROCESS_BASIC_INFORMATION>::uninit();
            let mut ret_len = 0;
            let status = unsafe {
                NtQueryInformationProcess(
                    handle,
                    0, // ProcessBasicInformation
                    pbi.as_mut_ptr() as _,
                    std::mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32,
                    &mut ret_len,
                )
            };

            if status != 0 {
                unsafe { CloseHandle(handle) };
                return Ok(fallback_windows());
            }

            let pbi = unsafe { pbi.assume_init() };
            let peb_base = pbi.PebBaseAddress;

            #[cfg(target_pointer_width = "64")]
            let proc_params_offset = 0x20;
            #[cfg(target_pointer_width = "32")]
            let proc_params_offset = 0x10;

            let mut proc_params_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
            let ok = unsafe {
                ReadProcessMemory(
                    handle,
                    peb_base.add(proc_params_offset),
                    &mut proc_params_ptr as *mut _ as _,
                    std::mem::size_of::<*mut std::ffi::c_void>(),
                    std::ptr::null_mut(),
                )
            };

            if ok == FALSE {
                unsafe { CloseHandle(handle) };
                return Ok(fallback_windows());
            }

            #[cfg(target_pointer_width = "64")]
            let cur_dir_offset = 0x38;
            #[cfg(target_pointer_width = "32")]
            let cur_dir_offset = 0x24;

            let mut unicode_str = std::mem::MaybeUninit::<UNICODE_STRING>::uninit();
            let ok = unsafe {
                ReadProcessMemory(
                    handle,
                    proc_params_ptr.add(cur_dir_offset),
                    unicode_str.as_mut_ptr() as _,
                    std::mem::size_of::<UNICODE_STRING>(),
                    std::ptr::null_mut(),
                )
            };

            if ok == FALSE {
                unsafe { CloseHandle(handle) };
                return Ok(fallback_windows());
            }

            let unicode_str = unsafe { unicode_str.assume_init() };
            let mut buffer = vec![0u16; (unicode_str.Length / 2) as usize];
            let ok = unsafe {
                ReadProcessMemory(
                    handle,
                    unicode_str.Buffer as _,
                    buffer.as_mut_ptr() as _,
                    unicode_str.Length as usize,
                    std::ptr::null_mut(),
                )
            };

            unsafe { CloseHandle(handle) };

            if ok == FALSE {
                Ok(fallback_windows())
            } else {
                let path_str = String::from_utf16_lossy(&buffer);
                Ok(PathBuf::from(path_str))
            }
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            Ok(fallback_windows())
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

#[cfg(not(target_os = "linux"))]
fn fallback_windows() -> PathBuf {
    let mut fallback = dirs::home_dir();
    #[cfg(windows)]
    {
        if let Some(h) = &fallback {
            if h.to_string_lossy().to_lowercase().contains("systemprofile") {
                fallback = None;
            }
        }
    }
    fallback.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
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

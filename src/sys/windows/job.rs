//! Windows Job Object management for ensuring child process cleanup.
//!
//! This module provides a way to group all child processes (like PTY shells)
//! into a Windows Job Object that automatically terminates them when the
//! main Irosh process exits.

use std::ptr::null_mut;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Threading::*;

pub struct JobObject {
    handle: HANDLE,
}

// SAFETY: `JobObject` wraps a Win32 job object handle created by `CreateJobObjectW`.
// The handle is only accessed through synchronized Win32 job APIs
// (`AssignProcessToJobObject`, `SetInformationJobObject`, `CloseHandle`), which
// are safe to call from any thread for a given job handle.
unsafe impl Send for JobObject {}
unsafe impl Sync for JobObject {}

impl JobObject {
    /// Creates a new job object and configures it to kill all assigned
    /// processes when the job handle is closed.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying Win32 `CreateJobObjectW` or
    /// `SetInformationJobObject` calls fail.
    pub fn new() -> std::io::Result<Self> {
        // SAFETY: Win32 API calls for job object creation and configuration.
        // We validate the handle against INVALID_HANDLE_VALUE and check all
        // return codes. The `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` is
        // zero-initialized via `std::mem::zeroed`.
        unsafe {
            let handle = CreateJobObjectW(null_mut(), null_mut());
            if handle == INVALID_HANDLE_VALUE || handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }

            let mut info = std::mem::zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            let res = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );

            if res == 0 {
                CloseHandle(handle);
                return Err(std::io::Error::last_os_error());
            }

            Ok(Self { handle })
        }
    }

    /// Assigns a process to this job object.
    fn assign_process(&self, process_handle: HANDLE) -> std::io::Result<()> {
        // SAFETY: `self.handle` is a valid job object handle created by `JobObject::new`.
        // `process_handle` must be a valid process handle provided by the caller.
        // `AssignProcessToJobObject` is a documented Win32 API.
        unsafe {
            if AssignProcessToJobObject(self.handle, process_handle) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        // SAFETY: `self.handle` is a valid job object handle created by `JobObject::new`.
        // `CloseHandle` is safe to call in Drop as long as the handle is valid.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// A global job object that is initialized once and used to track all child processes.
///
/// On Windows, this ensures that even if Irosh is killed via Task Manager or crashes,
/// any spawned PTY shells are also terminated by the OS.
pub static GLOBAL_JOB: std::sync::OnceLock<JobObject> = std::sync::OnceLock::new();

/// Initializes the global job object.
///
/// # Errors
///
/// Returns an error if `JobObject::new` fails to create the underlying
/// Win32 job object or configure it.
pub fn init_global_job() -> std::io::Result<()> {
    if GLOBAL_JOB.get().is_none() {
        let job = JobObject::new()?;
        let _ = GLOBAL_JOB.set(job);
    }
    Ok(())
}

/// Assigns the current process to the global job object.
/// This will cause all *future* children to be automatically part of the job.
///
/// # Errors
///
/// Returns an error if `init_global_job` fails or if the underlying
/// `AssignProcessToJobObject` call fails.
pub fn assign_current_process_to_job() -> std::io::Result<()> {
    init_global_job()?;
    if let Some(job) = GLOBAL_JOB.get() {
        // SAFETY: `GetCurrentProcess` returns a pseudo-handle to the current process
        // (always valid, no need to close). `job` was initialized by `init_global_job`.
        unsafe {
            job.assign_process(GetCurrentProcess())?;
        }
    }
    Ok(())
}

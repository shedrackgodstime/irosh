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

impl JobObject {
    /// Creates a new job object and configures it to kill all assigned
    /// processes when the job handle is closed.
    pub fn new() -> std::io::Result<Self> {
        unsafe {
            let handle = CreateJobObjectW(null_mut(), null_mut());
            if handle == INVALID_HANDLE_VALUE || handle == 0 {
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
    pub fn assign_process(&self, process_handle: HANDLE) -> std::io::Result<()> {
        unsafe {
            if AssignProcessToJobObject(self.handle, process_handle) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }

    /// Returns the raw handle for this job object.
    #[allow(dead_code)]
    pub fn handle(&self) -> HANDLE {
        self.handle
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
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
pub fn init_global_job() -> std::io::Result<()> {
    if GLOBAL_JOB.get().is_none() {
        let job = JobObject::new()?;
        let _ = GLOBAL_JOB.set(job);
    }
    Ok(())
}

/// Assigns the current process to the global job object.
/// This will cause all *future* children to be automatically part of the job.
pub fn assign_current_process_to_job() -> std::io::Result<()> {
    init_global_job()?;
    if let Some(job) = GLOBAL_JOB.get() {
        unsafe {
            job.assign_process(GetCurrentProcess())?;
        }
    }
    Ok(())
}

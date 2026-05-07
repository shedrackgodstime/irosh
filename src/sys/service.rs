//! Platform-agnostic service management interface.

use crate::error::Result;
use std::path::PathBuf;

/// The current state of the irosh background service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceStatus {
    /// The service is installed and currently running.
    Active(String),
    /// The service is installed but not running.
    Inactive,
    /// The service is not installed.
    NotFound,
    /// The service state could not be determined.
    Unknown,
}

/// Actions that can be performed on the background service.
pub enum ServiceAction {
    Install,
    Uninstall,
    Start,
    Stop,
}

/// Performs a service management action.
pub async fn handle_service(action: ServiceAction, state: Option<PathBuf>) -> Result<()> {
    #[cfg(unix)]
    return crate::sys::unix::service::handle_service(action, state).await;

    #[cfg(windows)]
    return crate::sys::windows::service::handle_service(action, state).await;

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (action, state);
        Err(crate::error::IroshError::PlatformNotSupported)
    }
}

/// Displays the service logs.
pub async fn view_logs(follow: bool) -> Result<()> {
    #[cfg(unix)]
    return crate::sys::unix::service::view_logs(follow).await;

    #[cfg(windows)]
    return crate::sys::windows::service::view_logs(follow).await;

    #[cfg(not(any(unix, windows)))]
    {
        let _ = follow;
        Err(crate::error::IroshError::PlatformNotSupported)
    }
}

/// Queries the OS service manager for the service status.
pub async fn query_service_status() -> ServiceStatus {
    #[cfg(unix)]
    return crate::sys::unix::service::query_service_status().await;

    #[cfg(windows)]
    return crate::sys::windows::service::query_service_status().await;

    #[cfg(not(any(unix, windows)))]
    ServiceStatus::Unknown
}

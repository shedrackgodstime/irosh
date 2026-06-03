//! Platform-agnostic service management interface.

use crate::error::Result;
use std::path::PathBuf;

/// The current state of the irosh background service.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
#[non_exhaustive]
pub enum ServiceAction {
    /// Install the service for automatic startup.
    Install,
    /// Remove the service from the system.
    Uninstall,
    /// Start the service immediately.
    Start,
    /// Stop the running service.
    Stop,
}

/// Performs a service management action.
///
/// # Errors
///
/// Returns `PlatformNotSupported` on platforms that are not Unix or Windows.
#[must_use]
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
///
/// # Errors
///
/// Returns `PlatformNotSupported` on platforms that are not Unix or Windows.
#[must_use]
pub async fn view_logs(follow: bool, state: Option<PathBuf>) -> Result<()> {
    #[cfg(unix)]
    return crate::sys::unix::service::view_logs(follow, state).await;

    #[cfg(windows)]
    return crate::sys::windows::service::view_logs(follow, state).await;

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (follow, state);
        Err(crate::error::IroshError::PlatformNotSupported)
    }
}

/// Queries the OS service manager for the service status.
#[must_use]
pub async fn query_service_status(state: Option<PathBuf>) -> ServiceStatus {
    #[cfg(unix)]
    return crate::sys::unix::service::query_service_status(state).await;

    #[cfg(windows)]
    return crate::sys::windows::service::query_service_status(state).await;

    #[cfg(not(any(unix, windows)))]
    {
        let _ = state;
        ServiceStatus::Unknown
    }
}

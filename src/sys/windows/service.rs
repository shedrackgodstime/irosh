//! Windows implementation of service management (Service Control Manager).

use crate::error::{Result, ServerError};
use crate::sys::service::{ServiceAction, ServiceStatus};
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{error, info, warn};

use std::ffi::OsString;
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState,
        ServiceStatus as WinServiceStatus, ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_manager::{ServiceManager, ServiceManagerAccess},
};

const SERVICE_NAME: &str = "irosh";
const SERVICE_DISPLAY_NAME: &str = "Irosh P2P SSH Service";

pub async fn query_service_status(state: Option<PathBuf>) -> ServiceStatus {
    let _ = state; // Windows uses a global service name for now
    let manager = match ServiceManager::local_computer(
        None::<&std::ffi::OsStr>,
        ServiceManagerAccess::CONNECT,
    ) {
        Ok(m) => m,
        Err(_) => return ServiceStatus::Unknown,
    };

    let service = match manager.open_service(
        SERVICE_NAME,
        windows_service::service::ServiceAccess::QUERY_STATUS,
    ) {
        Ok(s) => s,
        Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(1060) => {
            return ServiceStatus::NotFound;
        } // ERROR_SERVICE_DOES_NOT_EXIST
        Err(_) => return ServiceStatus::Unknown,
    };

    match service.query_status() {
        Ok(status) => match status.current_state {
            ServiceState::Running => ServiceStatus::Active("SCM".to_string()),
            ServiceState::Stopped => ServiceStatus::Inactive,
            ServiceState::StartPending
            | ServiceState::StopPending
            | ServiceState::ContinuePending
            | ServiceState::PausePending
            | ServiceState::Paused => ServiceStatus::Active("SCM (transitioning)".to_string()),
        },
        Err(_) => ServiceStatus::Unknown,
    }
}

/// Performs a service management action on Windows via the Service Control Manager.
///
/// # Errors
///
/// Returns an error if the SCM cannot be opened, the service cannot be
/// created, started, stopped, or deleted, or if file system operations fail.
#[must_use]
pub async fn handle_service(action: ServiceAction, state: Option<PathBuf>) -> Result<()> {
    match action {
        ServiceAction::Install => install_service(state).await,
        ServiceAction::Uninstall => uninstall_service(),
        ServiceAction::Start => start_service(),
        ServiceAction::Stop => stop_service(state).await,
    }
}

async fn install_service(state: Option<PathBuf>) -> Result<()> {
    let exe_path = std::env::current_exe().map_err(|e| ServerError::ServiceManagement {
        details: format!("failed to get current executable path: {}", e),
    })?;

    // Perform SCM installation in a synchronous scope to ensure non-Send types are dropped
    {
        let manager = ServiceManager::local_computer(
            None::<&std::ffi::OsStr>,
            ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
        )
        .map_err(|e| {
            let details = if format!("{:?}", e).contains("Access is denied") {
                "failed to open SCM: Access denied. Please run this command as Administrator."
                    .to_string()
            } else {
                format!("failed to open SCM: {}", e)
            };
            ServerError::ServiceManagement { details }
        })?;

        let state_dir = state.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".irosh").join("server"))
                .unwrap_or_else(|| PathBuf::from(".irosh").join("server"))
        });

        let service_info = windows_service::service::ServiceInfo {
            name: SERVICE_NAME.to_string().into(),
            display_name: SERVICE_DISPLAY_NAME.to_string().into(),
            service_type: windows_service::service::ServiceType::OWN_PROCESS,
            start_type: windows_service::service::ServiceStartType::AutoStart,
            error_control: windows_service::service::ServiceErrorControl::Normal,
            executable_path: exe_path,
            dependencies: vec![],
            account_name: None,
            account_password: None,
            launch_arguments: vec!["host".into(), "--state".into(), state_dir.into_os_string()],
        };

        let _service = manager
            .create_service(
                &service_info,
                windows_service::service::ServiceAccess::START,
            )
            .map_err(|e| ServerError::ServiceManagement {
                details: format!("failed to create service: {}", e),
            })?;
    }

    info!("Service '{}' installed successfully.", SERVICE_NAME);
    start_service()?;
    Ok(())
}

fn uninstall_service() -> Result<()> {
    let manager =
        ServiceManager::local_computer(None::<&std::ffi::OsStr>, ServiceManagerAccess::CONNECT)
            .map_err(|e| {
                let details = if format!("{:?}", e).contains("Access is denied") {
                    "failed to open SCM: Access denied. Please run this command as Administrator."
                        .to_string()
                } else {
                    format!("failed to open SCM: {}", e)
                };
                ServerError::ServiceManagement { details }
            })?;

    let service = manager
        .open_service(
            SERVICE_NAME,
            windows_service::service::ServiceAccess::DELETE
                | windows_service::service::ServiceAccess::STOP,
        )
        .map_err(|e| ServerError::ServiceManagement {
            details: format!("failed to open service: {}", e),
        })?;

    let _ = service.stop();
    service
        .delete()
        .map_err(|e| ServerError::ServiceManagement {
            details: format!("failed to delete service: {}", e),
        })?;

    info!("Service '{}' uninstalled.", SERVICE_NAME);
    Ok(())
}

fn start_service() -> Result<()> {
    let manager =
        ServiceManager::local_computer(None::<&std::ffi::OsStr>, ServiceManagerAccess::CONNECT)
            .map_err(|e| {
                let details = if format!("{:?}", e).contains("Access is denied") {
                    "failed to open SCM: Access denied. Please run this command as Administrator."
                        .to_string()
                } else {
                    format!("failed to open SCM: {}", e)
                };
                ServerError::ServiceManagement { details }
            })?;

    let service = manager
        .open_service(SERVICE_NAME, windows_service::service::ServiceAccess::START)
        .map_err(|e| ServerError::ServiceManagement {
            details: format!("failed to open service: {}", e),
        })?;

    service
        .start::<&std::ffi::OsStr>(&[])
        .map_err(|e| ServerError::ServiceManagement {
            details: format!("failed to start service: {}", e),
        })?;

    info!("Service '{}' started.", SERVICE_NAME);
    Ok(())
}

async fn stop_service(state: Option<PathBuf>) -> Result<()> {
    // Try IPC shutdown first for grace
    let state_dir = state.clone().unwrap_or_else(|| {
        dirs::home_dir()
            .map(|h| h.join(".irosh").join("server"))
            .unwrap_or_else(|| PathBuf::from(".irosh").join("server"))
    });

    let client = crate::IpcClient::new(&state_dir);
    if let Ok(res) = client.send(crate::server::ipc::IpcCommand::Shutdown).await {
        if matches!(res, crate::server::ipc::IpcResponse::Ok) {
            info!("Graceful shutdown requested via IPC.");
            return Ok(());
        }
    }

    // Fallback to SCM stop (Synchronous block to ensure non-Send handles are dropped)
    {
        let manager = ServiceManager::local_computer(
            None::<&std::ffi::OsStr>,
            ServiceManagerAccess::CONNECT,
        )
        .map_err(|e| {
            let details = if format!("{:?}", e).contains("Access is denied") {
                "failed to open SCM: Access denied. Please run this command as Administrator."
                    .to_string()
            } else {
                format!("failed to open SCM: {}", e)
            };
            ServerError::ServiceManagement { details }
        })?;

        let service = manager
            .open_service(SERVICE_NAME, windows_service::service::ServiceAccess::STOP)
            .map_err(|e| ServerError::ServiceManagement {
                details: format!("failed to open service: {}", e),
            })?;

        service.stop().map_err(|e| ServerError::ServiceManagement {
            details: format!("failed to stop service: {}", e),
        })?;
    }

    info!("Service '{}' stopped.", SERVICE_NAME);
    Ok(())
}

/// Displays the irosh service logs from the daemon log file.
///
/// # Errors
///
/// Returns an error if the daemon log file cannot be opened for reading.
#[must_use]
pub async fn view_logs(follow: bool, state: Option<PathBuf>) -> Result<()> {
    let state_dir = state.unwrap_or_else(|| {
        dirs::home_dir()
            .map(|h| h.join(".irosh").join("server"))
            .unwrap_or_else(|| PathBuf::from(".irosh").join("server"))
    });
    let log_path = state_dir.join("daemon.log");

    if !log_path.exists() {
        info!("No log file found at {}.", log_path.display());
        return Ok(());
    }

    let file = File::open(&log_path)
        .await
        .map_err(|e| ServerError::ServiceManagement {
            details: format!("failed to open log file: {}", e),
        })?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                if !follow {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Ok(_) => {
                tracing::info!("{}", line);
            }
            Err(e) => {
                warn!("Error reading logs: {}", e);
                break;
            }
        }
    }

    Ok(())
}

// --- Service Dispatcher Logic ---

define_windows_service!(ffi_service_main, irosh_service_main);

/// Starts the Windows service dispatcher for irosh.
///
/// Registers the main service entry point and blocks until the service
/// receives a stop signal. Only relevant on Windows platforms.
///
/// # Errors
///
/// Returns a `windows_service::Error` if the service dispatcher fails to
/// start or register the service entry point.
#[must_use]
pub fn run_service() -> std::result::Result<(), windows_service::Error> {
    windows_service::service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

fn irosh_service_main(arguments: Vec<OsString>) {
    if let Err(e) = irosh_service_run(arguments) {
        error!("Service execution failed: {:?}", e);
    }
}

fn irosh_service_run(_arguments: Vec<OsString>) -> Result<()> {
    // Ensure all children (PTYs) are cleaned up if we exit unexpectedly
    let _ = crate::sys::windows::job::assign_current_process_to_job();

    let (tx, rx) = std::sync::mpsc::channel();

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                let _ = tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle =
        service_control_handler::register(SERVICE_NAME, event_handler).map_err(|e| {
            ServerError::ServiceManagement {
                details: format!("failed to register service handler: {}", e),
            }
        })?;

    status_handle
        .set_service_status(WinServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: std::time::Duration::default(),
            process_id: None,
        })
        .map_err(|e| ServerError::ServiceManagement {
            details: format!("failed to set service status: {}", e),
        })?;

    // Start a tokio runtime for the server
    let rt = tokio::runtime::Runtime::new().map_err(|e| ServerError::ServiceManagement {
        details: format!("failed to start tokio runtime: {}", e),
    })?;

    rt.block_on(async {
        // Parse arguments to find state directory.
        let mut state_dir = None;
        let env_args: Vec<std::ffi::OsString> = std::env::args_os().collect();
        let mut i = 1;
        while i < env_args.len() {
            if (env_args[i] == "--state" || env_args[i] == "-s") && i + 1 < env_args.len() {
                state_dir = Some(PathBuf::from(&env_args[i + 1]));
                break;
            }
            i += 1;
        }

        let state_root = state_dir.unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".irosh").join("server"))
                .unwrap_or_else(|| PathBuf::from(".irosh").join("server"))
        });

        // Catch panics and log them to the daemon log
        std::panic::set_hook(Box::new(move |info| {
            error!("SERVICE PANIC: {}", info);
        }));

        // Initialize file logging for the service in the user root for better visibility
        let log_path = state_root.join("daemon.log");
        if let Ok(file) = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
        {
            let _ = tracing_subscriber::fmt()
                .with_writer(file)
                .with_env_filter("irosh=debug,info")
                .try_init();
        }

        info!(
            "Service starting with state directory: {}",
            state_root.display()
        );
        info!("Command line: {:?}", env_args);

        // On Windows, when running as LocalSystem, the HOME and USERPROFILE
        // environment variables point to the system profile. We want them to
        // point to the user's home directory so that path resolution (e.g. ~/)
        // works correctly. We can infer this from the state directory.
        // If state_root is C:\Users\Ghost\.irosh\server, then home is C:\Users\Ghost.
        if let Some(parent) = state_root.parent() {
            // C:\Users\Ghost\.irosh
            if let Some(home) = parent.parent() {
                // C:\Users\Ghost
                let home_str = home.to_string_lossy();
                info!("Mapping service home to: {}", home_str);
                // SAFETY: We are in the single-threaded initialization phase of the service.
                unsafe {
                    std::env::set_var("HOME", &*home_str);
                    std::env::set_var("USERPROFILE", &*home_str);
                }
            }
        }

        let state = crate::config::StateConfig::new(state_root);
        let config = crate::storage::load_config(&state).unwrap_or_default();

        let mut options = crate::ServerOptions::new(state);
        if let Some(relay_url) = &config.relay_url {
            if let Ok(mode) = crate::transport::iroh::parse_relay_mode(relay_url) {
                options = options.relay_mode(mode, Some(relay_url.clone()));
            }
        }
        if let Some(secret) = &config.stealth_secret {
            options = options.secret(secret);
        }

        let (_, server) =
            crate::Server::bind(options)
                .await
                .map_err(|e| ServerError::ServiceManagement {
                    details: format!("server bind failed: {}", e),
                })?;

        let shutdown = server.shutdown_handle();

        // Use a blocking recv on the standard channel in a spawned blocking task
        // to avoid blocking the main async loop.
        tokio::select! {
            _ = tokio::task::spawn_blocking(move || rx.recv()) => {
                info!("Service stop requested via SCM.");
                shutdown.close().await;
            }
            res = server.run() => {
                if let Err(e) = res {
                    error!("Server run loop exited with error: {}", e);
                }
            }
        }

        Ok::<(), ServerError>(())
    })?;

    status_handle
        .set_service_status(WinServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: std::time::Duration::default(),
            process_id: None,
        })
        .map_err(|e| ServerError::ServiceManagement {
            details: format!("failed to set service status: {}", e),
        })?;

    Ok(())
}

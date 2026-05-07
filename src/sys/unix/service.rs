//! Unix implementation of service management (systemd and launchd).

use crate::error::{IroshError, Result, ServerError};
use crate::sys::service::{ServiceAction, ServiceStatus};
use std::path::PathBuf;
use tracing::info;

pub async fn query_service_status() -> ServiceStatus {
    #[cfg(target_os = "linux")]
    return query_status_linux().await;

    #[cfg(target_os = "macos")]
    return query_status_macos().await;

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    ServiceStatus::Unknown
}

#[cfg(target_os = "linux")]
async fn query_status_linux() -> ServiceStatus {
    let user_home = dirs::home_dir().unwrap_or_default();
    let service_file = user_home.join(".config/systemd/user/irosh.service");
    let exists = service_file.exists();

    let output = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "irosh"])
        .output();

    match output {
        Ok(out) => {
            let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
            match state.as_str() {
                "active" => ServiceStatus::Active("systemd".to_string()),
                "inactive" | "failed" | "deactivating" => ServiceStatus::Inactive,
                _ => {
                    if exists {
                        ServiceStatus::Inactive
                    } else {
                        ServiceStatus::NotFound
                    }
                }
            }
        }
        Err(_) => {
            if exists {
                ServiceStatus::Inactive
            } else {
                ServiceStatus::Unknown
            }
        }
    }
}

#[cfg(target_os = "macos")]
async fn query_status_macos() -> ServiceStatus {
    let uid = match std::process::Command::new("id").arg("-u").output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => return ServiceStatus::Unknown,
    };
    let target = format!("gui/{}/dev.irosh.server", uid);

    let output = std::process::Command::new("launchctl")
        .args(["print", &target])
        .output();

    match output {
        Ok(out) if out.status.success() => ServiceStatus::Active("launchd".to_string()),
        Ok(_) => {
            let plist = dirs::home_dir()
                .map(|h| h.join("Library/LaunchAgents/dev.irosh.server.plist"))
                .filter(|p| p.exists());
            if plist.is_some() {
                ServiceStatus::Inactive
            } else {
                ServiceStatus::NotFound
            }
        }
        Err(_) => ServiceStatus::Unknown,
    }
}

pub async fn handle_service(action: ServiceAction, state: Option<PathBuf>) -> Result<()> {
    #[cfg(target_os = "linux")]
    return handle_service_linux(action, state).await;

    #[cfg(target_os = "macos")]
    return handle_service_macos(action, state).await;

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (action, state);
        Err(IroshError::PlatformNotSupported)
    }
}

#[cfg(target_os = "linux")]
async fn handle_service_linux(action: ServiceAction, state: Option<PathBuf>) -> Result<()> {
    let exe = std::env::current_exe().map_err(|e| ServerError::ServiceManagement {
        details: format!("failed to get current exe path: {}", e),
    })?;
    let user_home = dirs::home_dir().ok_or_else(|| ServerError::ServiceManagement {
        details: "could not find home directory".to_string(),
    })?;
    let service_dir = user_home.join(".config/systemd/user");
    let service_file = service_dir.join("irosh.service");

    match action {
        ServiceAction::Install => {
            std::fs::create_dir_all(&service_dir).map_err(|e| ServerError::ServiceManagement {
                details: format!("failed to create service directory: {}", e),
            })?;

            let state_arg = if let Some(p) = state {
                format!(" --state {}", p.display())
            } else {
                "".to_string()
            };

            let unit = format!(
                r#"[Unit]
Description=irosh P2P SSH Server
After=network.target

[Service]
ExecStart={} host {}
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
"#,
                exe.display(),
                state_arg
            );

            std::fs::write(&service_file, unit).map_err(|e| ServerError::ServiceManagement {
                details: format!("failed to write service file: {}", e),
            })?;
            info!("Service unit installed to {}", service_file.display());

            std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status()
                .map_err(|e| ServerError::ServiceManagement {
                    details: format!("systemctl daemon-reload failed: {}", e),
                })?;
            std::process::Command::new("systemctl")
                .args(["--user", "enable", "irosh"])
                .status()
                .map_err(|e| ServerError::ServiceManagement {
                    details: format!("systemctl enable failed: {}", e),
                })?;
            std::process::Command::new("systemctl")
                .args(["--user", "restart", "irosh"])
                .status()
                .map_err(|e| ServerError::ServiceManagement {
                    details: format!("systemctl restart failed: {}", e),
                })?;

            info!("Service enabled and started in the background.");
        }
        ServiceAction::Uninstall => {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "stop", "irosh"])
                .status();
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "disable", "irosh"])
                .status();
            if service_file.exists() {
                std::fs::remove_file(&service_file).map_err(|e| {
                    ServerError::ServiceManagement {
                        details: format!("failed to remove service file: {}", e),
                    }
                })?;
                info!("Service uninstalled.");
            }
        }
        ServiceAction::Start => {
            std::process::Command::new("systemctl")
                .args(["--user", "start", "irosh"])
                .status()
                .map_err(|e| ServerError::ServiceManagement {
                    details: format!("systemctl start failed: {}", e),
                })?;
            info!("Service started.");
        }
        ServiceAction::Stop => {
            std::process::Command::new("systemctl")
                .args(["--user", "stop", "irosh"])
                .status()
                .map_err(|e| ServerError::ServiceManagement {
                    details: format!("systemctl stop failed: {}", e),
                })?;
            info!("Service stopped.");
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
async fn handle_service_macos(action: ServiceAction, state: Option<PathBuf>) -> Result<()> {
    let exe = std::env::current_exe().map_err(|e| ServerError::ServiceManagement {
        details: format!("failed to get current exe path: {}", e),
    })?;
    let user_home = dirs::home_dir().ok_or_else(|| ServerError::ServiceManagement {
        details: "could not find home directory".to_string(),
    })?;
    let service_dir = user_home.join("Library/LaunchAgents");
    let label = "dev.irosh.server";
    let service_file = service_dir.join(format!("{}.plist", label));
    let uid = current_uid().map_err(|e| ServerError::ServiceManagement {
        details: format!("failed to get current uid: {}", e),
    })?;
    let domain = format!("gui/{uid}");
    let service_target = format!("{domain}/{label}");

    match action {
        ServiceAction::Install => {
            std::fs::create_dir_all(&service_dir).map_err(|e| ServerError::ServiceManagement {
                details: format!("failed to create LaunchAgents directory: {}", e),
            })?;
            std::fs::create_dir_all(user_home.join("Library/Logs")).map_err(|e| {
                ServerError::ServiceManagement {
                    details: format!("failed to create Logs directory: {}", e),
                }
            })?;
            let plist = build_launchd_plist(&exe, &state, &user_home);
            std::fs::write(&service_file, plist).map_err(|e| ServerError::ServiceManagement {
                details: format!("failed to write plist file: {}", e),
            })?;

            let _ = std::process::Command::new("launchctl")
                .args(["bootout", &service_target])
                .status();

            std::process::Command::new("launchctl")
                .args(["bootstrap", &domain, &service_file.display().to_string()])
                .status()
                .map_err(|e| ServerError::ServiceManagement {
                    details: format!("launchctl bootstrap failed: {}", e),
                })?;

            info!("LaunchAgent installed to {}", service_file.display());
            info!("Service loaded and started with launchd.");
        }
        ServiceAction::Uninstall => {
            let _ = std::process::Command::new("launchctl")
                .args(["bootout", &service_target])
                .status();
            if service_file.exists() {
                std::fs::remove_file(&service_file).map_err(|e| {
                    ServerError::ServiceManagement {
                        details: format!("failed to remove plist file: {}", e),
                    }
                })?;
                info!("LaunchAgent removed.");
            }
        }
        ServiceAction::Start => {
            if !service_file.exists() {
                return Err(IroshError::Server(ServerError::ServiceManagement {
                    details: "LaunchAgent is not installed.".to_string(),
                }));
            }

            let _ = std::process::Command::new("launchctl")
                .args(["bootstrap", &domain, &service_file.display().to_string()])
                .status();

            std::process::Command::new("launchctl")
                .args(["kickstart", "-k", &service_target])
                .status()
                .map_err(|e| ServerError::ServiceManagement {
                    details: format!("launchctl kickstart failed: {}", e),
                })?;

            info!("LaunchAgent started.");
        }
        ServiceAction::Stop => {
            std::process::Command::new("launchctl")
                .args(["bootout", &service_target])
                .status()
                .map_err(|e| ServerError::ServiceManagement {
                    details: format!("launchctl bootout failed: {}", e),
                })?;
            info!("LaunchAgent stopped.");
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn build_launchd_plist(
    exe: &std::path::Path,
    state: &Option<PathBuf>,
    user_home: &std::path::Path,
) -> String {
    let mut args_xml = format!(
        "<string>{}</string>\n    <string>host</string>",
        xml_escape(exe.to_string_lossy().as_ref())
    );
    if let Some(state_dir) = state {
        args_xml.push_str(&format!(
            "\n    <string>--state</string>\n    <string>{}</string>",
            xml_escape(state_dir.to_string_lossy().as_ref())
        ));
    }

    let stdout_path = user_home
        .join("Library/Logs/irosh.log")
        .display()
        .to_string();
    let stderr_path = user_home
        .join("Library/Logs/irosh.err.log")
        .display()
        .to_string();

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>dev.irosh.server</string>
  <key>ProgramArguments</key>
  <array>
    {}
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
        args_xml, stdout_path, stderr_path
    )
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "macos")]
fn current_uid() -> Result<String> {
    let output = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map_err(|e| ServerError::ServiceManagement {
            details: format!("failed to run id command: {}", e),
        })?;
    if !output.status.success() {
        return Err(IroshError::Server(ServerError::ServiceManagement {
            details: "id -u command failed".to_string(),
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub async fn view_logs(follow: bool) -> Result<()> {
    #[cfg(target_os = "linux")]
    return view_logs_linux(follow).await;

    #[cfg(target_os = "macos")]
    return view_logs_macos(follow).await;

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Err(IroshError::PlatformNotSupported)
}

#[cfg(target_os = "linux")]
async fn view_logs_linux(follow: bool) -> Result<()> {
    let mut args = vec!["--user", "-u", "irosh"];
    if follow {
        args.push("-f");
    }

    let status = std::process::Command::new("journalctl")
        .args(&args)
        .status()
        .map_err(|e| ServerError::ServiceManagement {
            details: format!("failed to run journalctl: {}", e),
        })?;

    if !status.success() {
        return Err(IroshError::Server(ServerError::ServiceManagement {
            details: "journalctl failed".to_string(),
        }));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
async fn view_logs_macos(follow: bool) -> Result<()> {
    let user_home = dirs::home_dir().ok_or_else(|| ServerError::ServiceManagement {
        details: "could not find home directory".to_string(),
    })?;
    let log_file = user_home.join("Library/Logs/irosh.log");

    if !log_file.exists() {
        return Err(IroshError::Server(ServerError::ServiceManagement {
            details: "log file not found. Is the service running?".to_string(),
        }));
    }

    let mut args = vec![log_file.to_string_lossy().to_string()];
    let cmd = if follow {
        args.insert(0, "-f".to_string());
        "tail"
    } else {
        "cat"
    };

    let status = std::process::Command::new(cmd)
        .args(&args)
        .status()
        .map_err(|e| ServerError::ServiceManagement {
            details: format!("failed to run {}: {}", cmd, e),
        })?;

    if !status.success() {
        return Err(IroshError::Server(ServerError::ServiceManagement {
            details: format!("{} failed", cmd),
        }));
    }
    Ok(())
}

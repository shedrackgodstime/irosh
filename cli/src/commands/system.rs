use crate::Args as GlobalArgs;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args, Debug, Clone)]
pub struct SystemArgs {
    #[command(subcommand)]
    pub action: SystemAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SystemAction {
    /// Install the server as a background service.
    Install,
    /// Remove the background service.
    Uninstall,
    /// Start the background service.
    Start,
    /// Stop the background service.
    Stop,
    /// Show the background service status.
    Status,
}

pub async fn exec(system_args: SystemArgs, global_args: &GlobalArgs) -> Result<()> {
    match system_args.action {
        SystemAction::Install => handle_service(ServiceAction::Install, &global_args.state).await?,
        SystemAction::Uninstall => {
            handle_service(ServiceAction::Uninstall, &global_args.state).await?
        }
        SystemAction::Start => handle_service(ServiceAction::Start, &global_args.state).await?,
        SystemAction::Stop => handle_service(ServiceAction::Stop, &global_args.state).await?,
        SystemAction::Status => handle_service(ServiceAction::Status, &global_args.state).await?,
    }
    Ok(())
}

// Logic below is adapted from cli-old/bin/server.rs

enum ServiceAction {
    Install,
    Uninstall,
    Start,
    Stop,
    Status,
}

/// The current state of the irosh background service on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceStatus {
    /// The service is installed and currently running.
    Active(String),
    /// The service is installed but not running.
    Inactive,
    /// The service is not installed.
    NotFound,
    /// The service state could not be determined (e.g. OS not supported).
    Unknown,
}

/// Queries the OS service manager for the irosh background service status.
///
/// Returns a structured [`ServiceStatus`] that can be used by other commands
/// without printing to stdout. This is the machine-readable equivalent of
/// `irosh system status`.
pub async fn query_service_status() -> ServiceStatus {
    #[cfg(target_os = "linux")]
    return query_status_linux().await;

    #[cfg(target_os = "macos")]
    return query_status_macos().await;

    #[cfg(target_os = "windows")]
    return query_status_windows().await;

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    ServiceStatus::Unknown
}

#[cfg(target_os = "linux")]
async fn query_status_linux() -> ServiceStatus {
    let output = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "irosh"])
        .output();

    match output {
        Ok(out) => {
            let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
            match state.as_str() {
                "active" => ServiceStatus::Active("systemd".to_string()),
                "inactive" | "failed" => ServiceStatus::Inactive,
                _ => ServiceStatus::NotFound,
            }
        }
        Err(_) => ServiceStatus::Unknown,
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
            // Check if the plist file exists even if not loaded.
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

#[cfg(target_os = "windows")]
async fn query_status_windows() -> ServiceStatus {
    let output = std::process::Command::new("schtasks")
        .args(["/query", "/tn", "irosh", "/fo", "LIST"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            if text.contains("Running") {
                ServiceStatus::Active("Task Scheduler".to_string())
            } else {
                ServiceStatus::Inactive
            }
        }
        Ok(_) => ServiceStatus::NotFound,
        Err(_) => ServiceStatus::Unknown,
    }
}

#[cfg(target_os = "macos")]
const MACOS_LAUNCHD_LABEL: &str = "dev.irosh.server";
#[cfg(target_os = "macos")]
const MACOS_LAUNCHD_FILE: &str = "dev.irosh.server.plist";

async fn handle_service(action: ServiceAction, state: &Option<PathBuf>) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        handle_service_windows(action, state).await
    }

    #[cfg(target_os = "linux")]
    {
        handle_service_linux(action, state).await
    }

    #[cfg(target_os = "macos")]
    {
        handle_service_macos(action, state).await
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = (action, state);
        Err(anyhow::anyhow!(
            "Service management is only supported on Linux (systemd), macOS (launchd), and Windows (Task Scheduler)"
        ))
    }
}

#[cfg(target_os = "linux")]
async fn handle_service_linux(action: ServiceAction, state: &Option<PathBuf>) -> Result<()> {
    let exe = std::env::current_exe()?;
    let user_home = dirs::home_dir().context("could not find home directory")?;
    let service_dir = user_home.join(".config/systemd/user");
    let service_file = service_dir.join("irosh.service");

    match action {
        ServiceAction::Install => {
            std::fs::create_dir_all(&service_dir)?;

            let state_arg = if let Some(p) = state {
                format!(" --state {}", p.display())
            } else {
                "".to_string()
            };

            // Use 'host' subcommand in the service
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

            std::fs::write(&service_file, unit)?;
            println!("✅ Service unit installed to {}", service_file.display());

            std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status()?;
            std::process::Command::new("systemctl")
                .args(["--user", "enable", "irosh"])
                .status()?;
            std::process::Command::new("systemctl")
                .args(["--user", "start", "irosh"])
                .status()?;

            println!("🚀 Service enabled and started in the background.");
        }
        ServiceAction::Uninstall => {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "stop", "irosh"])
                .status();
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "disable", "irosh"])
                .status();
            if service_file.exists() {
                std::fs::remove_file(&service_file)?;
                println!("🗑️ Service uninstalled.");
            }
        }
        ServiceAction::Start => {
            std::process::Command::new("systemctl")
                .args(["--user", "start", "irosh"])
                .status()?;
            println!("▶️ Service started.");
        }
        ServiceAction::Stop => {
            std::process::Command::new("systemctl")
                .args(["--user", "stop", "irosh"])
                .status()?;
            println!("⏹️ Service stopped.");
        }
        ServiceAction::Status => {
            std::process::Command::new("systemctl")
                .args(["--user", "status", "irosh"])
                .status()?;
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
async fn handle_service_windows(action: ServiceAction, state: &Option<PathBuf>) -> Result<()> {
    let exe = std::env::current_exe()?;
    let exe_str = exe.display().to_string();

    let state_arg = if let Some(p) = state {
        format!("/state \"{}\"", p.display())
    } else {
        String::new()
    };

    let task_name = "irosh";

    match action {
        ServiceAction::Install => {
            let task_xml = format!(
                r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>
    <Boot />
    <Logon />
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>true</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{}</Command>
      <Arguments>host {}</Arguments>
    </Exec>
  </Actions>
</Task>"#,
                exe_str, state_arg
            );

            let temp_dir = std::env::temp_dir();
            let xml_path = temp_dir.join("irosh-task.xml");

            let mut file = std::fs::File::create(&xml_path)?;
            use std::io::Write;
            file.write_all(&[0xFF, 0xFE])?; // UTF-16 LE BOM
            for c in task_xml.encode_utf16() {
                let bytes = c.to_le_bytes();
                file.write_all(&bytes)?;
            }

            std::process::Command::new("schtasks")
                .args([
                    "/create",
                    "/tn",
                    task_name,
                    "/xml",
                    &xml_path.display().to_string(),
                    "/f",
                ])
                .status()?;

            std::fs::remove_file(&xml_path)?;

            println!("✅ Windows Task Scheduler task created: {}", task_name);
            println!("🚀 Service will start on next login or boot.");
        }
        ServiceAction::Uninstall => {
            let _ = std::process::Command::new("schtasks")
                .args(["/delete", "/tn", task_name, "/f"])
                .status();
            println!("🗑️ Task Scheduler task removed.");
        }
        ServiceAction::Start => {
            std::process::Command::new("schtasks")
                .args(["/run", "/tn", task_name])
                .status()?;
            println!("▶️ Task started.");
        }
        ServiceAction::Stop => {
            std::process::Command::new("taskkill")
                .args(["/IM", "irosh.exe", "/F"])
                .status()?;
            println!("⏹️ Task stopped.");
        }
        ServiceAction::Status => {
            let output = std::process::Command::new("schtasks")
                .args(["/query", "/tn", task_name])
                .output()?;

            if output.status.success() {
                println!("✅ Task '{}' exists:", task_name);
                println!("{}", String::from_utf8_lossy(&output.stdout));
            } else {
                println!("❌ Task '{}' not found.", task_name);
            }
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
async fn handle_service_macos(action: ServiceAction, state: &Option<PathBuf>) -> Result<()> {
    let exe = std::env::current_exe()?;
    let user_home = dirs::home_dir().context("could not find home directory")?;
    let service_dir = user_home.join("Library/LaunchAgents");
    let service_file = service_dir.join(MACOS_LAUNCHD_FILE);
    let label = MACOS_LAUNCHD_LABEL;
    let uid = current_uid()?;
    let domain = format!("gui/{uid}");
    let service_target = format!("{domain}/{label}");

    match action {
        ServiceAction::Install => {
            std::fs::create_dir_all(&service_dir)?;
            std::fs::create_dir_all(user_home.join("Library/Logs"))?;
            let plist = build_launchd_plist(&exe, state, &user_home);
            std::fs::write(&service_file, plist)?;

            let _ = std::process::Command::new("launchctl")
                .args(["bootout", &service_target])
                .status();

            std::process::Command::new("launchctl")
                .args(["bootstrap", &domain, &service_file.display().to_string()])
                .status()?;

            println!("✅ LaunchAgent installed to {}", service_file.display());
            println!("🚀 Service loaded and started with launchd.");
        }
        ServiceAction::Uninstall => {
            let _ = std::process::Command::new("launchctl")
                .args(["bootout", &service_target])
                .status();
            if service_file.exists() {
                std::fs::remove_file(&service_file)?;
                println!("🗑️ LaunchAgent removed.");
            }
        }
        ServiceAction::Start => {
            if !service_file.exists() {
                return Err(anyhow::anyhow!(
                    "LaunchAgent is not installed. Run 'irosh system install' first."
                ));
            }

            let _ = std::process::Command::new("launchctl")
                .args(["bootstrap", &domain, &service_file.display().to_string()])
                .status();

            std::process::Command::new("launchctl")
                .args(["kickstart", "-k", &service_target])
                .status()?;

            println!("▶️ LaunchAgent started.");
        }
        ServiceAction::Stop => {
            std::process::Command::new("launchctl")
                .args(["bootout", &service_target])
                .status()?;
            println!("⏹️ LaunchAgent stopped.");
        }
        ServiceAction::Status => {
            if !service_file.exists() {
                println!("❌ LaunchAgent is not installed.");
                return Ok(());
            }

            let output = std::process::Command::new("launchctl")
                .args(["print", &service_target])
                .output()?;

            if output.status.success() {
                println!("✅ LaunchAgent '{}' is installed:", label);
                println!("{}", String::from_utf8_lossy(&output.stdout));
            } else {
                println!(
                    "ℹ️ LaunchAgent '{}' is installed but not currently loaded.",
                    label
                );
            }
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
    let mut args = vec![
        plist_string(exe.display().to_string()),
        plist_string("host"),
    ];
    if let Some(state_dir) = state {
        args.push(plist_string("--state"));
        args.push(plist_string(state_dir.display().to_string()));
    }

    let stdout_path = plist_string(
        user_home
            .join("Library/Logs/irosh.log")
            .display()
            .to_string(),
    );
    let stderr_path = plist_string(
        user_home
            .join("Library/Logs/irosh.err.log")
            .display()
            .to_string(),
    );

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{MACOS_LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    {}
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{stdout_path}</string>
  <key>StandardErrorPath</key>
  <string>{stderr_path}</string>
</dict>
</plist>
"#,
        args.join("\n    ")
    )
}

#[cfg(target_os = "macos")]
fn plist_string(value: impl AsRef<str>) -> String {
    format!("<string>{}</string>", xml_escape(value.as_ref()))
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
    let output = std::process::Command::new("id").arg("-u").output()?;
    if !output.status.success() {
        return Err(anyhow::anyhow!("failed to resolve current user id"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

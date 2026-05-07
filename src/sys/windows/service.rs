//! Windows implementation of service management (Task Scheduler).

use crate::error::{Result, ServerError};
use crate::sys::service::{ServiceAction, ServiceStatus};
use std::path::PathBuf;
use tracing::info;

pub async fn query_service_status() -> ServiceStatus {
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

pub async fn handle_service(action: ServiceAction, state: Option<PathBuf>) -> Result<()> {
    let exe = std::env::current_exe().map_err(|e| ServerError::ServiceManagement {
        details: format!("failed to get current exe path: {}", e),
    })?;
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

            {
                use std::io::Write;
                let mut file = std::fs::File::create(&xml_path).map_err(|e| {
                    ServerError::ServiceManagement {
                        details: format!("failed to create temp xml file: {}", e),
                    }
                })?;
                file.write_all(&[0xFF, 0xFE])
                    .map_err(|e| ServerError::ServiceManagement {
                        details: format!("failed to write BOM to xml file: {}", e),
                    })?; // UTF-16 LE BOM
                for c in task_xml.encode_utf16() {
                    let bytes = c.to_le_bytes();
                    file.write_all(&bytes)
                        .map_err(|e| ServerError::ServiceManagement {
                            details: format!("failed to write UTF-16 content to xml file: {}", e),
                        })?;
                }
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
                .status()
                .map_err(|e| ServerError::ServiceManagement {
                    details: format!("schtasks /create failed: {}", e),
                })?;

            let _ = std::fs::remove_file(&xml_path);

            info!("Windows Task Scheduler task created: {}", task_name);
            info!("Service will start on next login or boot.");
        }
        ServiceAction::Uninstall => {
            let _ = std::process::Command::new("schtasks")
                .args(["/delete", "/tn", task_name, "/f"])
                .status();
            info!("Task Scheduler task removed.");
        }
        ServiceAction::Start => {
            std::process::Command::new("schtasks")
                .args(["/run", "/tn", task_name])
                .status()
                .map_err(|e| ServerError::ServiceManagement {
                    details: format!("schtasks /run failed: {}", e),
                })?;
            info!("Task started.");
        }
        ServiceAction::Stop => {
            std::process::Command::new("taskkill")
                .args(["/IM", "irosh.exe", "/F"])
                .status()
                .map_err(|e| ServerError::ServiceManagement {
                    details: format!("taskkill failed: {}", e),
                })?;
            info!("Task stopped.");
        }
    }

    Ok(())
}

pub async fn view_logs(_follow: bool) -> Result<()> {
    info!("Direct log viewing for Windows Task Scheduler is not yet implemented.");
    info!("You can view task history in the Windows Task Scheduler GUI (taskschd.msc).");
    Ok(())
}

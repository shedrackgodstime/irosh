use crate::commands::SystemAction;
use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::sys::service::{self, ServiceAction, ServiceStatus};

pub async fn exec(action: SystemAction, ctx: &CliContext) -> Result<()> {
    let state_root = ctx.server_state_root()?;

    // Isolate the core action execution so we can catch permission errors
    let run_action = async {
        match &action {
            SystemAction::Install => {
                Ui::p2p("Installing background service...");
                service::handle_service(ServiceAction::Install, Some(state_root.clone())).await?;
                Ui::success("Service installed and started.");
                Ui::info("Run 'irosh system status' to verify.");
            }
            SystemAction::Uninstall => {
                if !ctx.args.yes && !Ui::soft_confirm("Uninstall the background service?") {
                    return Ok(());
                }
                service::handle_service(ServiceAction::Uninstall, Some(state_root.clone())).await?;
                Ui::success("Service uninstalled.");
            }
            SystemAction::Start => {
                service::handle_service(ServiceAction::Start, Some(state_root.clone())).await?;
                Ui::success("Service started.");
            }
            SystemAction::Stop => {
                service::handle_service(ServiceAction::Stop, Some(state_root.clone())).await?;
                Ui::success("Service stopped.");
            }
            SystemAction::Restart => {
                service::handle_service(ServiceAction::Stop, Some(state_root.clone())).await?;
                service::handle_service(ServiceAction::Start, Some(state_root.clone())).await?;
                Ui::success("Service restarted.");
            }
            SystemAction::Status => {
                let status = service::query_service_status(Some(state_root.clone())).await;

                if ctx.args.json {
                    #[derive(serde::Serialize)]
                    struct SystemStatusResponse {
                        state: &'static str,
                        manager: Option<String>,
                        message: &'static str,
                    }

                    let response = match status {
                        ServiceStatus::Active(ref manager) => SystemStatusResponse {
                            state: "active",
                            manager: Some(manager.clone()),
                            message: "Service is running.",
                        },
                        ServiceStatus::Inactive => SystemStatusResponse {
                            state: "inactive",
                            manager: None,
                            message: "Service is installed but not running.",
                        },
                        ServiceStatus::NotFound => SystemStatusResponse {
                            state: "not_installed",
                            manager: None,
                            message: "Service is not installed.",
                        },
                        ServiceStatus::Unknown => SystemStatusResponse {
                            state: "unknown",
                            manager: None,
                            message: "Service status is unknown.",
                        },
                    };
                    crate::output::print_success(response);
                    return Ok(());
                }

                eprintln!("\n  System Service Status");
                eprintln!("  ----------------------------------------------------");
                match status {
                    ServiceStatus::Active(manager) => {
                        eprintln!("  Status:    ACTIVE");
                        eprintln!("  Manager:   {}", manager);
                    }
                    ServiceStatus::Inactive => {
                        eprintln!("  Status:    INACTIVE");
                        eprintln!("  Notice:    Service is installed but not running.");
                    }
                    ServiceStatus::NotFound => {
                        eprintln!("  Status:    NOT INSTALLED");
                        eprintln!(
                            "  Action:    Run 'irosh system install' to enable background tasks."
                        );
                    }
                    ServiceStatus::Unknown => {
                        eprintln!("  Status:    UNKNOWN");
                    }
                }
                eprintln!("  ----------------------------------------------------\n");
            }
            SystemAction::Logs { follow } => {
                irosh::sys::service::view_logs(*follow, Some(state_root.clone())).await?;
            }
        }
        Ok::<(), anyhow::Error>(())
    };

    if let Err(e) = run_action.await {
        let err_str = e.to_string();
        let is_access_error = cfg!(windows)
            && (err_str.contains("Access denied") || err_str.contains("IO error in winapi call"));

        // Check if we already tried to elevate to prevent infinite UAC loops
        let already_elevated = std::env::var("IROSH_AUTO_ELEVATED").is_ok();

        if is_access_error && !already_elevated {
            Ui::p2p("Administrator privileges required. Requesting elevation...");

            let exe = std::env::current_exe()?;
            let mut args: Vec<String> = std::env::args().skip(1).collect();

            // Auto-append --yes to prevent the new elevated window from blocking on prompts
            if !args.contains(&"--yes".to_string()) && !args.contains(&"-y".to_string()) {
                args.push("--yes".to_string());
            }

            let args_str = args
                .iter()
                .map(|a| format!("'{}'", a))
                .collect::<Vec<_>>()
                .join(", ");

            let mut cmd = std::process::Command::new("powershell");
            // Pass the environment variable to the elevated process
            cmd.env("IROSH_AUTO_ELEVATED", "1")
                .arg("-NoProfile")
                .arg("-WindowStyle")
                .arg("Hidden")
                .arg("-Command")
                .arg(format!(
                    "Start-Process -FilePath '{}' -ArgumentList {} -Verb RunAs -Wait",
                    exe.display(),
                    args_str
                ));

            let status = cmd.status()?;
            if !status.success() {
                anyhow::bail!("Elevated action was cancelled or failed.");
            }

            // The elevated process succeeded. Print the final success message in the original terminal for seamless UX.
            match action {
                SystemAction::Install => Ui::success("Service installed and started."),
                SystemAction::Uninstall => Ui::success("Service uninstalled."),
                SystemAction::Start => Ui::success("Service started."),
                SystemAction::Stop => Ui::success("Service stopped."),
                SystemAction::Restart => Ui::success("Service restarted."),
                _ => {}
            }
            return Ok(());
        }
        return Err(e);
    }

    Ok(())
}

use crate::commands::SystemAction;
use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::sys::service::{self, ServiceAction, ServiceStatus};

#[must_use]
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

                // Try to get live info from the daemon via IPC if it might be running
                let mut daemon_info = None;
                if matches!(status, ServiceStatus::Active(_) | ServiceStatus::Unknown) {
                    let client = irosh::client::ipc::IpcClient::new(&state_root);
                    if let Ok(irosh::server::ipc::IpcResponse::Status(info)) =
                        client.send(irosh::server::ipc::IpcCommand::GetStatus).await
                    {
                        daemon_info = Some(info);
                    }
                }

                if ctx.args.json {
                    #[derive(serde::Serialize)]
                    struct SystemStatusResponse {
                        state: &'static str,
                        manager: Option<String>,
                        daemon: Option<irosh::server::ipc::DaemonStatus>,
                        message: &'static str,
                    }

                    let response = match status {
                        ServiceStatus::Active(ref manager) => SystemStatusResponse {
                            state: "active",
                            manager: Some(manager.clone()),
                            daemon: daemon_info,
                            message: "Service is running.",
                        },
                        ServiceStatus::Inactive => SystemStatusResponse {
                            state: "inactive",
                            manager: None,
                            daemon: None,
                            message: "Service is installed but not running.",
                        },
                        ServiceStatus::NotFound => SystemStatusResponse {
                            state: "not_installed",
                            manager: None,
                            daemon: None,
                            message: "Service is not installed.",
                        },
                        ServiceStatus::Unknown => SystemStatusResponse {
                            state: "unknown",
                            manager: None,
                            daemon: daemon_info,
                            message: "Service status is unknown.",
                        },
                        _ => unreachable!(),
                    };
                    crate::output::print_success(response);
                    return Ok(());
                }

                Ui::header("System Service Status");
                match status {
                    ServiceStatus::Active(manager) => {
                        Ui::status("Service", "ACTIVE", Some(&manager));
                        if let Some(info) = daemon_info {
                            Ui::machine_identity(
                                &info.endpoint_id,
                                "Daemon Live",
                                &info.ticket,
                                "Background Hosting",
                            );
                            Ui::status("Active Sessions", &info.active_sessions.to_string(), None);
                            if info.wormhole_active {
                                Ui::status("Wormhole", "ENABLED", info.wormhole_code.as_deref());
                            } else {
                                Ui::status("Wormhole", "DISABLED", None);
                            }

                            if !info.sessions.is_empty() {
                                Ui::session_table(&info.sessions);
                            }
                        } else {
                            Ui::warn(
                                "IPC Connectivity",
                                "Could not connect to daemon IPC. The service might be starting or restricted.",
                            );
                        }
                    }
                    ServiceStatus::Inactive => {
                        Ui::status("Service", "INACTIVE", None);
                        Ui::info("Notice: Service is installed but not running.");
                    }
                    ServiceStatus::NotFound => {
                        Ui::status("Service", "NOT INSTALLED", None);
                        Ui::info("Action: Run 'irosh system install' to enable background tasks.");
                    }
                    ServiceStatus::Unknown => {
                        Ui::status("Service", "UNKNOWN", None);
                    }
                    _ => unreachable!(),
                }
                Ui::blank();
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
                .map(|a| format!("'{a}'"))
                .collect::<Vec<_>>()
                .join(", ");

            let status = tokio::task::spawn_blocking(move || {
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
                cmd.status()
            })
            .await
            .map_err(|e| anyhow::anyhow!("Blocking task failed: {e}"))?
            .map_err(|e| anyhow::anyhow!("Command failed: {e}"))?;
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

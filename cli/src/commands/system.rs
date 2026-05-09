use crate::commands::SystemAction;
use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::sys::service::{self, ServiceAction, ServiceStatus};

pub async fn exec(action: SystemAction, ctx: &CliContext) -> Result<()> {
    let state_root = ctx.server_state_root()?;

    match action {
        SystemAction::Install => {
            Ui::p2p("Installing background service...");
            service::handle_service(ServiceAction::Install, Some(state_root)).await?;
            Ui::success("Service installed and started.");
            Ui::info("Run 'irosh system status' to verify.");
        }
        SystemAction::Uninstall => {
            if Ui::soft_confirm("Uninstall the background service?") {
                service::handle_service(ServiceAction::Uninstall, Some(state_root)).await?;
                Ui::success("Service uninstalled.");
            }
        }
        SystemAction::Start => {
            service::handle_service(ServiceAction::Start, Some(state_root)).await?;
            Ui::success("Service started.");
        }
        SystemAction::Stop => {
            service::handle_service(ServiceAction::Stop, Some(state_root)).await?;
            Ui::success("Service stopped.");
        }
        SystemAction::Restart => {
            service::handle_service(ServiceAction::Stop, Some(state_root.clone())).await?;
            service::handle_service(ServiceAction::Start, Some(state_root)).await?;
            Ui::success("Service restarted.");
        }
        SystemAction::Status => {
            let status = service::query_service_status().await;

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
            irosh::sys::service::view_logs(follow).await?;
        }
    }
    Ok(())
}

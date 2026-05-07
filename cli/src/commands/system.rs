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
            println!("\n  \x1b[1;35m⚙️  System Service Status\x1b[0m");
            println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m");
            match status {
                ServiceStatus::Active(manager) => {
                    println!("  Status:    \x1b[1;32m● ACTIVE\x1b[0m");
                    println!("  Manager:   {}", manager);
                }
                ServiceStatus::Inactive => {
                    println!("  Status:    \x1b[1;33m○ INACTIVE\x1b[0m");
                    println!("  Notice:    Service is installed but not running.");
                }
                ServiceStatus::NotFound => {
                    println!("  Status:    \x1b[1;31m◌ NOT INSTALLED\x1b[0m");
                    println!("  Action:    Run 'irosh system install' to enable background tasks.");
                }
                ServiceStatus::Unknown => {
                    println!("  Status:    \x1b[38;5;244m? UNKNOWN\x1b[0m");
                }
            }
            println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m\n");
        }
        SystemAction::Logs { follow } => {
            irosh::sys::service::view_logs(follow).await?;
        }
    }
    Ok(())
}

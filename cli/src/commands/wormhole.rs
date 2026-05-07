use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::{IpcClient, IpcCommand, IpcResponse, Server, ServerOptions, StateConfig};
use std::path::Path;

pub async fn exec(
    mut code: Option<String>,
    passwd: bool,
    persistent: bool,
    ctx: &CliContext,
) -> Result<()> {
    let state_root = ctx.server_state_root()?;
    let state = ctx.server_state()?;

    let ipc_client = IpcClient::new(state_root.clone());

    // Check if daemon is running
    let daemon_running = ipc_client.send(IpcCommand::GetStatus).await.is_ok();

    match code.as_deref() {
        Some("status") => {
            if !daemon_running {
                anyhow::bail!("Daemon is not running. Run 'irosh system start'.");
            }
            return handle_status(&ipc_client).await;
        }
        Some("disable") | Some("stop") => {
            if !daemon_running {
                anyhow::bail!("Daemon is not running.");
            }
            return handle_disable(&ipc_client).await;
        }
        Some("enable") => {
            // Treat "enable" as a request to start a wormhole with a random code
            code = None;
        }
        _ => {}
    }

    let has_node_password = irosh::storage::load_shadow_file(&state)?.is_some();
    let vault = irosh::storage::load_all_authorized_clients(&state)?;

    // Initiation Rules (Discovery Security Guard):
    if passwd {
        // Rule 4: --passwd flag -> prompt and hash exactly like a permanent password.
    } else if has_node_password {
        // Rule 3: Node password is set -> Allowed (guarded by node password).
    } else if vault.is_empty() {
        // Rule 1: Vault is empty -> Allowed (Bootstrap Phase), but warn.
        Ui::security("Security Notice:");
        Ui::info("      Your vault is empty and no password is set.");
        Ui::info("      The first device to discover this code will become the permanent owner.");
        if !Ui::soft_confirm("Continue anyway?") {
            anyhow::bail!("Wormhole cancelled for security.");
        }
    } else {
        // Rule 2: Vault NOT empty and no password set -> BLOCKED.
        Ui::error(
            "Wormhole is blocked: Your server has trusted devices but no Node Password is set.",
        );
        Ui::info(
            "Tip: Set a Node Password ('irosh passwd set') or use '--passwd' to issue a one-time invite.",
        );
        anyhow::bail!("Security initiation block.");
    }

    // If the user passed --passwd, prompt and hash the password now.
    // This is identical treatment to `irosh passwd set`.
    let password_hash: Option<String> = if passwd {
        match Ui::password_input("Enter wormhole session password (one-time use)") {
            Some(pw) if !pw.is_empty() => {
                let hash = irosh::auth::hash_password(&pw)?;
                Ui::success("Wormhole password set (will be destroyed after first use).");
                Some(hash)
            }
            _ => {
                anyhow::bail!("No password entered. Wormhole cancelled.");
            }
        }
    } else {
        None
    };

    if daemon_running && ctx.args.state.is_none() {
        handle_enable_daemon(&ipc_client, code, password_hash, persistent).await
    } else {
        handle_foreground_wormhole(&state_root, code, password_hash, persistent).await
    }
}

async fn handle_status(client: &IpcClient) -> Result<()> {
    match client.send(IpcCommand::GetStatus).await? {
        IpcResponse::Status {
            wormhole_active,
            wormhole_code,
            active_sessions,
        } => {
            if wormhole_active {
                Ui::success(&format!(
                    "Wormhole active! Code: {}",
                    wormhole_code.unwrap_or_default()
                ));
            } else {
                Ui::info("No active wormhole.");
            }
            Ui::info(&format!("Active sessions: {}", active_sessions));
        }
        _ => anyhow::bail!("Unexpected response from daemon"),
    }
    Ok(())
}

async fn handle_disable(client: &IpcClient) -> Result<()> {
    match client.send(IpcCommand::DisableWormhole).await? {
        IpcResponse::Ok => Ui::success("Wormhole disabled."),
        IpcResponse::Error(e) => Ui::error(&format!("Failed to disable: {}", e)),
        _ => anyhow::bail!("Unexpected response from daemon"),
    }
    Ok(())
}

async fn handle_enable_daemon(
    client: &IpcClient,
    code: Option<String>,
    password: Option<String>,
    persistent: bool,
) -> Result<()> {
    let final_code = code.unwrap_or_else(irosh::transport::wormhole::generate_code);

    match client
        .send(IpcCommand::EnableWormhole {
            code: final_code.clone(),
            password,
            persistent,
        })
        .await?
    {
        IpcResponse::Ok => {
            Ui::success(&format!(
                "Wormhole active in background! Code: {}",
                final_code
            ));
            Ui::info(&format!("Run 'irosh {}' on the other machine.", final_code));
        }
        IpcResponse::Error(e) => Ui::error(&format!("Daemon rejected: {}", e)),
        _ => anyhow::bail!("Unexpected response from daemon"),
    }
    Ok(())
}

async fn handle_foreground_wormhole(
    state_root: &Path,
    code: Option<String>,
    password: Option<String>,
    persistent: bool,
) -> Result<()> {
    let state = StateConfig::new(state_root.to_path_buf());
    let final_code = code.unwrap_or_else(irosh::transport::wormhole::generate_code);

    Ui::p2p("Starting temporary wormhole server...");

    let options = ServerOptions::new(state)
        .disable_ipc()
        .shutdown_on_wormhole_success();
    let (_ready, server) = Server::bind(options).await?;
    let control = server.control_handle();

    Ui::success(&format!(
        "Wormhole active (Foreground)! Code: {}",
        final_code
    ));
    Ui::info(&format!("Run 'irosh {}' on the other machine.", final_code));
    Ui::info("Waiting for peer... (Ctrl+C to cancel)");

    let (tx, _) = tokio::sync::oneshot::channel();
    control
        .send(irosh::InternalCommand::EnableWormhole {
            code: final_code,
            password,
            persistent,
            tx,
        })
        .await
        .map_err(|_| anyhow::anyhow!("Server channel closed"))?;

    server.run().await?;
    Ok(())
}

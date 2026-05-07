use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::{Server, ServerOptions};

pub async fn exec(secret: Option<String>, ctx: &CliContext) -> Result<()> {
    let state = ctx.server_state()?;

    // Check if vault is empty to show security notice
    let vault = irosh::storage::load_all_authorized_clients(&state)?;
    if vault.is_empty() {
        Ui::warn(
            "Vault is empty",
            "The first device to connect will be permanently trusted.\nTip: Run 'irosh passwd set' now to require a password instead.",
        );
    }

    let config = irosh::storage::load_config(&state)?;
    let mut options = ServerOptions::new(state);

    // Apply global config overrides
    if let Some(secret) = secret.or(config.stealth_secret) {
        options = options.secret(secret);
    }

    if let Some(relay_url) = &config.relay_url {
        let mode = irosh::transport::iroh::parse_relay_mode(relay_url)?;
        options = options.relay_mode(mode, Some(relay_url.clone()));
    }

    let (ready, server) = match Server::bind(options).await {
        Ok(res) => res,
        Err(e) => {
            // Check for Identity Conflict (Double Instance)
            // If the background service is running, the secret key will be locked.
            // We can also check if the IPC socket exists as a hint.
            let daemon_running = irosh::IpcClient::new(ctx.server_state_root()?)
                .send(irosh::IpcCommand::GetStatus)
                .await
                .is_ok();

            if daemon_running {
                Ui::error(
                    "Failed to start: The Irosh daemon is already running in the background.",
                );
                Ui::info("Use 'irosh system status' or 'irosh system logs' to view activity.");
                Ui::info(
                    "Tip: You can use 'irosh wormhole' to pair new devices without stopping the daemon.",
                );
                anyhow::bail!("Identity conflict.");
            } else {
                return Err(e.into());
            }
        }
    };

    Ui::p2p("Server is starting...");
    Ui::success(&format!("Server listening! Ticket: {}", ready.ticket));
    Ui::info("Press Ctrl+C to stop the server.");

    server.run().await?;

    Ok(())
}

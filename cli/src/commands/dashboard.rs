use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::{IpcClient, IpcCommand, IpcResponse, storage};

pub async fn exec(ctx: &CliContext) -> Result<()> {
    let state = ctx.server_state()?;
    let state_root = ctx.server_state_root()?;

    let ipc_client = IpcClient::new(state_root);
    let daemon_status = ipc_client.send(IpcCommand::GetStatus).await;

    Ui::header(&format!("Irosh v{}", env!("CARGO_PKG_VERSION")));

    // 1. Daemon & Connectivity
    match daemon_status {
        Ok(IpcResponse::Status(info)) => {
            Ui::success("Server Daemon: RUNNING");
            Ui::status("Active Sessions", &info.active_sessions.to_string(), None);
            let wormhole_status = if info.wormhole_active {
                "ACTIVE"
            } else {
                "INACTIVE"
            };
            Ui::status("Wormhole", wormhole_status, None);
        }
        _ => {
            Ui::status("Server Daemon", "STOPPED", None);
            Ui::info("run 'irosh system start' to enable P2P hosting");
        }
    }

    // 2. Identity
    if let Ok(identity) = storage::load_or_generate_identity(&state).await {
        let endpoint_id = identity.endpoint_id();
        let addr = irosh::iroh::EndpointAddr::from(identity.secret_key.public());
        let ticket = irosh::transport::ticket::Ticket::new(addr);

        Ui::header("Local Identity");
        Ui::status("Endpoint ID", &endpoint_id[..16], None);
        Ui::status(
            "Public Ticket",
            &crate::display::shorten_ticket(&ticket),
            None,
        );
    }

    // 3. Vault & Peers
    let trusted_keys = storage::list_authorized_keys(&state).unwrap_or_default();
    let saved_peers = storage::list_peers(&state).unwrap_or_default();

    Ui::header("Security & Peers");
    Ui::status("Trusted Devices", &trusted_keys.len().to_string(), None);
    Ui::status(
        "Address Book",
        &format!("{} saved peers", saved_peers.len()),
        None,
    );

    if trusted_keys.is_empty() && saved_peers.is_empty() {
        Ui::header("First Steps");
        Ui::info("Welcome to Irosh! To get started:");
        Ui::info("  1. Connect to another device:  irosh connect <code-or-ticket>");
        Ui::info("  2. Pair with a new device:     irosh wormhole");
        Ui::info("  3. Host this machine:          irosh system start");
        println!();
    } else {
        Ui::info("Quick connect: irosh <alias|ticket|code>");
    }

    Ok(())
}

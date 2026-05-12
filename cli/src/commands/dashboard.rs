use crate::context::CliContext;
use anyhow::Result;
use irosh::{IpcClient, IpcCommand, IpcResponse, storage};

pub async fn exec(ctx: &CliContext) -> Result<()> {
    let state = ctx.server_state()?;
    let state_root = ctx.server_state_root()?;

    let ipc_client = IpcClient::new(state_root);
    let daemon_status = ipc_client.send(IpcCommand::GetStatus).await;

    eprintln!("\n  Irosh v{} - Dashboard", env!("CARGO_PKG_VERSION"));
    eprintln!("  ----------------------------------------------------");

    // 1. Daemon & Connectivity
    match daemon_status {
        Ok(IpcResponse::Status(info)) => {
            eprintln!("  Server Daemon:    RUNNING");
            eprintln!("  Active Sessions:  {}", info.active_sessions);
            let wormhole_status = if info.wormhole_active {
                "ACTIVE"
            } else {
                "INACTIVE"
            };
            eprintln!("  Wormhole Status:  {}", wormhole_status);
        }
        _ => {
            eprintln!("  Server Daemon:    STOPPED");
            eprintln!("  Tip: Run 'irosh system start' to enable P2P hosting.");
        }
    }

    // 2. Identity
    if let Ok(identity) = storage::load_or_generate_identity(&state).await {
        let node_id = identity.node_id();
        let addr = irosh::iroh::EndpointAddr::from(identity.secret_key.public());
        let ticket = irosh::transport::ticket::Ticket::new(addr);

        eprintln!("\n  Local Identity");
        eprintln!("  Node ID:          {}", &node_id[..16]);
        eprintln!(
            "  Public Ticket:    {}",
            crate::display::shorten_ticket(&ticket)
        );
    }

    // 3. Vault & Peers
    let trusted_keys = storage::list_authorized_keys(&state).unwrap_or_default();
    let saved_peers = storage::list_peers(&state).unwrap_or_default();

    eprintln!("\n  Security & Peers");
    eprintln!("  Trusted Devices:  {}", trusted_keys.len());
    eprintln!("  Address Book:     {} saved peers", saved_peers.len());

    eprintln!("  ----------------------------------------------------");
    eprintln!("  Quick Connect: irosh <alias|ticket|code>\n");

    Ok(())
}

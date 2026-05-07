use crate::context::CliContext;
use anyhow::Result;
use irosh::{IpcClient, IpcCommand, IpcResponse, storage};

pub async fn exec(ctx: &CliContext) -> Result<()> {
    let state = ctx.server_state()?;
    let state_root = ctx.server_state_root()?;

    let ipc_client = IpcClient::new(state_root);
    let daemon_status = ipc_client.send(IpcCommand::GetStatus).await;

    println!("\n  Irosh v{} - Dashboard", env!("CARGO_PKG_VERSION"));
    println!("  ----------------------------------------------------");

    // 1. Daemon & Connectivity
    match daemon_status {
        Ok(IpcResponse::Status {
            wormhole_active,
            active_sessions,
            ..
        }) => {
            println!("  Server Daemon:    RUNNING");
            println!("  Active Sessions:  {}", active_sessions);
            let wormhole_status = if wormhole_active {
                "ACTIVE"
            } else {
                "INACTIVE"
            };
            println!("  Wormhole Status:  {}", wormhole_status);
        }
        _ => {
            println!("  Server Daemon:    STOPPED");
            println!("  Tip: Run 'irosh system start' to enable P2P hosting.");
        }
    }

    // 2. Identity
    if let Ok(identity) = storage::load_or_generate_identity(&state).await {
        let node_id = identity.node_id();
        let addr = irosh::iroh::EndpointAddr::from(identity.secret_key.public());
        let ticket = irosh::transport::ticket::Ticket::new(addr);

        println!("\n  Local Identity");
        println!("  Node ID:          {}", &node_id[..16]);
        println!(
            "  Public Ticket:    {}",
            crate::display::shorten_ticket(&ticket)
        );
    }

    // 3. Vault & Peers
    let trusted_keys = storage::list_authorized_keys(&state).unwrap_or_default();
    let saved_peers = storage::list_peers(&state).unwrap_or_default();

    println!("\n  Security & Peers");
    println!("  Trusted Devices:  {}", trusted_keys.len());
    println!("  Address Book:     {} saved peers", saved_peers.len());

    println!("  ----------------------------------------------------");
    println!("  Quick Connect: irosh <alias|ticket|code>\n");

    Ok(())
}

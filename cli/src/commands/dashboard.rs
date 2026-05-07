use crate::context::CliContext;
use anyhow::Result;
use irosh::{IpcClient, IpcCommand, IpcResponse, storage};

pub async fn exec(ctx: &CliContext) -> Result<()> {
    let state = ctx.server_state()?;
    let state_root = ctx.server_state_root()?;

    let ipc_client = IpcClient::new(state_root);
    let daemon_status = ipc_client.send(IpcCommand::GetStatus).await;

    println!(
        "\n  \x1b[1;36m🛸 irosh v{} — Dashboard\x1b[0m",
        env!("CARGO_PKG_VERSION")
    );
    println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m");

    // 1. Daemon & Connectivity
    match daemon_status {
        Ok(IpcResponse::Status {
            wormhole_active,
            active_sessions,
            ..
        }) => {
            println!("  \x1b[1;37m📡 Server Daemon:\x1b[0m    \x1b[1;32mRUNNING\x1b[0m");
            println!("     Active Sessions:   \x1b[1m{}\x1b[0m", active_sessions);
            let wormhole_status = if wormhole_active {
                "\x1b[1;33m✨ ACTIVE\x1b[0m"
            } else {
                "\x1b[2mINACTIVE\x1b[0m"
            };
            println!("     Wormhole Status:   {}", wormhole_status);
        }
        _ => {
            println!("  \x1b[1;37m📡 Server Daemon:\x1b[0m    \x1b[1;31mSTOPPED\x1b[0m");
            println!("     \x1b[2mTip: Run 'irosh system start' to enable P2P hosting.\x1b[0m");
        }
    }

    // 2. Identity
    if let Ok(identity) = storage::load_or_generate_identity(&state).await {
        let node_id = identity.node_id();
        let addr = irosh::iroh::EndpointAddr::from(identity.secret_key.public());
        let ticket = irosh::transport::ticket::Ticket::new(addr);

        println!("\n  \x1b[1;37m🆔 Local Identity\x1b[0m");
        println!("     Node ID:           \x1b[36m{}\x1b[0m", &node_id[..16]);
        println!(
            "     Public Ticket:     \x1b[33m{}\x1b[0m",
            crate::display::shorten_ticket(&ticket)
        );
    }

    // 3. Vault & Peers
    let trusted_keys = storage::list_authorized_keys(&state).unwrap_or_default();
    let saved_peers = storage::list_peers(&state).unwrap_or_default();

    println!("\n  \x1b[1;37m🛡️  Security & Peers\x1b[0m");
    println!(
        "     Trusted Devices:   \x1b[1m{}\x1b[0m",
        trusted_keys.len()
    );
    println!(
        "     Address Book:      \x1b[1m{}\x1b[0m saved peers",
        saved_peers.len()
    );

    println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m");
    println!("  \x1b[2mQuick Connect:\x1b[0m \x1b[36mirosh <alias|ticket|code>\x1b[0m\n");

    Ok(())
}

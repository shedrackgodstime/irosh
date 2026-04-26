use crate::Args;
use anyhow::{Context, Result};
use irosh::russh::keys::ssh_key::{HashAlg, PrivateKey, private::Ed25519Keypair};
use irosh::{StateConfig, storage, storage::trust};

use crate::commands::system::{ServiceStatus, query_service_status};
use crate::display::shorten_middle;

pub async fn exec(args: &Args) -> Result<()> {
    // Resolve client state directory.
    let state_root = args
        .state
        .clone()
        .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("client")))
        .context("could not determine state directory")?;

    let state = StateConfig::new(state_root);

    // ── Header ─────────────────────────────────────────────────────────────────
    let version = env!("CARGO_PKG_VERSION");
    println!();
    println!(
        "\x1b[1m{:<44}\x1b[0m \x1b[2mv{}\x1b[0m",
        "irosh — P2P SSH Tool", version
    );
    println!("\x1b[2m{}\x1b[0m", "─".repeat(54));

    // ── Identity ───────────────────────────────────────────────────────────────
    println!("\x1b[1mIdentity\x1b[0m");
    match storage::load_secret_key(&state) {
        Ok(secret) => {
            let node_id = secret.public().to_string();
            let short_id = shorten_middle(&node_id, 24);
            println!("  Node ID:    \x1b[36m{}\x1b[0m", short_id);

            // Derive SSH fingerprint
            let seed = secret.to_bytes();
            let keypair = Ed25519Keypair::from_seed(&seed);
            let ssh_key = PrivateKey::from(keypair);
            let fingerprint = ssh_key.public_key().fingerprint(HashAlg::Sha256);
            println!("  SSH Key:    \x1b[2m{}\x1b[0m (ed25519)", fingerprint);
        }
        Err(_) => {
            println!("  Node ID:    \x1b[33m(not initialized — run 'irosh host' to set up)\x1b[0m");
        }
    }
    println!("  State:      \x1b[2m{}\x1b[0m", state.root().display());
    println!();

    // ── Saved Peers ────────────────────────────────────────────────────────────
    let peers = storage::list_peers(&state).unwrap_or_default();
    println!(
        "\x1b[1m{:<40}\x1b[0m \x1b[2m{} total\x1b[0m",
        "Saved Peers",
        peers.len()
    );
    if peers.is_empty() {
        println!("  \x1b[2m(none — use 'irosh peer add <name> <ticket>' to save one)\x1b[0m");
    } else {
        for peer in &peers {
            let node_id = peer.ticket.to_addr().id.to_string();
            let short_id = shorten_middle(&node_id, 16);
            println!("  {:<20} \x1b[2m{}\x1b[0m", peer.name, short_id);
        }
    }
    println!();

    // ── Trust Store ────────────────────────────────────────────────────────────
    println!("\x1b[1mTrust Store\x1b[0m");
    match trust::inspect_trust(&state) {
        Ok(summary) => {
            println!("  Known Servers:        {}", summary.known_servers.len());
            println!(
                "  Authorized Clients:   {}",
                summary.authorized_clients.len()
            );
        }
        Err(_) => {
            println!("  \x1b[2m(trust store not initialized)\x1b[0m");
        }
    }
    println!();

    // ── Background Service ─────────────────────────────────────────────────────
    let service_label = os_service_label();
    println!(
        "\x1b[1mBackground Service\x1b[0m      \x1b[2m[{}]\x1b[0m",
        service_label
    );
    match query_service_status().await {
        ServiceStatus::Active(manager) => {
            println!("  \x1b[32m● Active\x1b[0m (running via {})", manager);
        }
        ServiceStatus::Inactive => {
            println!("  \x1b[31m○ Inactive\x1b[0m (installed but not running)");
            println!("    Run: \x1b[36mirosh system start\x1b[0m");
        }
        ServiceStatus::NotFound => {
            println!("  \x1b[2m— Not installed\x1b[0m");
            println!("    Run: \x1b[36mirosh system install\x1b[0m");
        }
        ServiceStatus::Unknown => {
            println!("  \x1b[33m? Unknown\x1b[0m (service manager not available)");
        }
    }

    // ── Footer ─────────────────────────────────────────────────────────────────
    println!();
    println!("\x1b[2m{}\x1b[0m", "─".repeat(54));
    println!(
        "\x1b[2mRun '\x1b[0m\x1b[36mirosh check\x1b[0m\x1b[2m' for network diagnostics.\x1b[0m"
    );
    println!();

    Ok(())
}

/// Returns a human-readable label for the current OS service manager.
fn os_service_label() -> &'static str {
    #[cfg(target_os = "linux")]
    return "Linux / systemd";
    #[cfg(target_os = "macos")]
    return "macOS / launchd";
    #[cfg(target_os = "windows")]
    return "Windows / Task Scheduler";
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return "Unknown OS";
}

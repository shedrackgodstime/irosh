use crate::Args;
use anyhow::{Context, Result};
use irosh::{SecurityConfig, Server, ServerOptions, StateConfig, storage};

/// Execute the identity command.
pub async fn exec(args: &Args) -> Result<()> {
    // 1. Resolve state directory (default to client state for identity).
    let state_root = args
        .state
        .clone()
        .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("client")))
        .context("could not determine state directory")?;

    let state = StateConfig::new(state_root);

    // 2. Load basic identity.
    let secret = storage::load_secret_key(&state).context("failed to load secret key")?;
    let public = secret.public();

    println!("🆔 Identity: {}", public);
    println!("📂 State:    {}", state.root().display());
    println!();

    // 3. Attempt to inspect server identity if possible.
    // We check if the server state directory exists or just use the current state.
    // For a "Pro" tool, we should show both if they differ or just the unified one.

    // Server usually lives in ~/.irosh/server
    let server_state_root = dirs::home_dir().map(|h| h.join(".irosh").join("server"));

    if let Some(root) = server_state_root {
        if root.exists() {
            let server_state = StateConfig::new(root);
            let options = ServerOptions::new(server_state).security(SecurityConfig {
                host_key_policy: irosh::config::HostKeyPolicy::Tofu,
            });

            if let Ok(ready) = Server::inspect(&options).await {
                println!("[Role: Server]");
                println!("🗝️ Host Key (SSH): {}", ready.host_key_openssh());
                println!("🎟️ Ticket:         {}", ready.ticket());
                println!();
            }
        }
    }

    Ok(())
}

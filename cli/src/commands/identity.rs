use crate::Args as GlobalArgs;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use irosh::{SecurityConfig, Server, ServerOptions, StateConfig, storage};

#[derive(Args, Debug, Clone)]
pub struct IdentityArgs {
    #[command(subcommand)]
    pub command: Option<IdentityCommands>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum IdentityCommands {
    /// Show current identity (default)
    Show,
    /// Rotate the identity (Warning: Breaks all existing connections)
    Rotate,
}

/// Execute the identity command.
pub async fn exec(identity_args: IdentityArgs, global_args: &GlobalArgs) -> Result<()> {
    // 1. Resolve state directory (default to client state for identity).
    let state_root = global_args
        .state
        .clone()
        .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("client")))
        .context("could not determine state directory")?;

    let state = StateConfig::new(state_root);

    let command = identity_args.command.unwrap_or(IdentityCommands::Show);

    match command {
        IdentityCommands::Show => {
            // Load basic identity.
            let secret = storage::load_secret_key(&state).context("failed to load secret key")?;
            let public = secret.public();

            println!("🆔 Identity: {}", public);
            println!("📂 State:    {}", state.root().display());
            println!();

            // Attempt to inspect server identity if possible.
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
        }
        IdentityCommands::Rotate => {
            let server_state_root = dirs::home_dir().map(|h| h.join(".irosh").join("server"));
            let target_state = if let Some(root) = server_state_root {
                StateConfig::new(root)
            } else {
                state
            };

            println!("🚨 WARNING: This will PERMANENTLY rotate your server identity.");
            println!(
                "All clients currently using your old ticket or alias will lose access immediately."
            );
            println!("You will need to share a new ticket with everyone.");
            println!();

            let confirm = dialoguer::Confirm::new()
                .with_prompt("Are you sure you want to rotate your identity?")
                .default(false)
                .interact()
                .context("failed to read confirmation from terminal")?;

            if confirm {
                if storage::delete_secret_key(&target_state)? {
                    println!("\n✅ Identity rotated successfully.");
                    println!(
                        "Restart the server with 'irosh host' to generate your new identity and ticket."
                    );
                } else {
                    println!(
                        "\nℹ️ No identity found to rotate. Starting the server will generate one."
                    );
                }
            } else {
                println!("\n❌ Identity rotation cancelled.");
            }
        }
    }

    Ok(())
}

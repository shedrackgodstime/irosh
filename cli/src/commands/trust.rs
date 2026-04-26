use crate::Args as GlobalArgs;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use irosh::{StateConfig, storage, storage::trust::TrustRecord};

#[derive(Args, Debug, Clone)]
pub struct TrustArgs {
    #[command(subcommand)]
    pub command: TrustCommands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum TrustCommands {
    /// Inspect the trust store.
    List,
    /// Authorize a new client public key.
    Allow {
        /// A unique identifier for the client (e.g. its Peer ID).
        node_id: String,
        /// The OpenSSH formatted public key string.
        key: String,
    },
    /// Revoke trust from a client or server.
    Revoke {
        /// The node ID to revoke.
        node_id: String,
        /// Revoke as a server (remove from known servers).
        #[arg(short, long)]
        server: bool,
        /// Revoke as a client (remove from authorized clients).
        #[arg(short, long)]
        client: bool,
    },
}

pub async fn exec(trust_args: TrustArgs, global_args: &GlobalArgs) -> Result<()> {
    // 1. Resolve state (use client state by default).
    let state_root = global_args
        .state
        .clone()
        .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("client")))
        .context("could not determine state directory")?;

    let state = StateConfig::new(state_root);

    match trust_args.command {
        TrustCommands::List => {
            let summary = storage::trust::inspect_trust(&state)?;
            println!("🔐 Trust Store: {}", state.root().join("trust").display());

            println!("\n[Known Servers (Your client trusts)]");
            if summary.known_servers.is_empty() {
                println!("  (None)");
            } else {
                for record in summary.known_servers {
                    print_record(record);
                }
            }

            println!("\n[Authorized Clients (Your server trusts)]");
            if summary.authorized_clients.is_empty() {
                println!("  (None)");
            } else {
                for record in summary.authorized_clients {
                    print_record(record);
                }
            }
        }
        TrustCommands::Allow { node_id, key } => {
            let public_key = irosh::russh::keys::ssh_key::PublicKey::from_openssh(&key)
                .context("invalid OpenSSH public key format")?;

            storage::trust::write_authorized_client(&state, &node_id, &public_key)?;
            println!("✅ Authorized client '{}'", node_id);
        }
        TrustCommands::Revoke {
            node_id,
            server,
            client,
        } => {
            if !server && !client {
                anyhow::bail!("Please specify --server, --client, or both to revoke trust.");
            }

            if server {
                if storage::trust::reset_known_server(&state, &node_id)? {
                    println!("✅ Revoked trust for server '{}'", node_id);
                } else {
                    println!("ℹ️ No trust record found for server '{}'", node_id);
                }
            }

            if client {
                if storage::trust::reset_authorized_client(&state, &node_id)? {
                    println!("✅ Revoked authorization for client '{}'", node_id);
                } else {
                    println!("ℹ️ No authorization record found for client '{}'", node_id);
                }
            }
        }
    }

    Ok(())
}

fn print_record(record: TrustRecord) {
    let filename = record
        .path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("?");
    let key_info = record
        .public_key_openssh
        .unwrap_or_else(|| "unknown".to_string());
    println!(" - {:<20} {}", filename, key_info);
}

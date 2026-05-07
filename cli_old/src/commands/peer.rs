use crate::Args as GlobalArgs;
use crate::display::{shorten_ticket, ticket_node_label};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use irosh::{StateConfig, storage};
use std::str::FromStr;

#[derive(Args, Debug, Clone)]
pub struct PeerArgs {
    #[command(subcommand)]
    pub command: PeerCommands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum PeerCommands {
    /// List all saved peers.
    List {
        /// Show full tickets instead of shortened versions.
        #[arg(short, long)]
        full: bool,
    },
    /// Save a new peer ticket.
    Add {
        /// Friendly name for the peer.
        name: String,
        /// The Iroh connection ticket.
        ticket: String,
    },
    /// Delete a saved peer.
    Remove {
        /// Name of the peer to remove.
        name: String,
    },
    /// Show detailed info about a specific peer.
    Info {
        /// Name of the peer.
        name: String,
    },
}

pub async fn exec(peer_args: PeerArgs, global_args: &GlobalArgs) -> Result<()> {
    let state_root = global_args
        .state
        .clone()
        .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("client")))
        .context("could not determine state directory")?;

    let state = StateConfig::new(state_root);

    match peer_args.command {
        PeerCommands::List { full } => {
            let peers = storage::list_peers(&state)?;
            if peers.is_empty() {
                println!("No peers saved.");
                println!("Use 'irosh peer add <name> <ticket>' to add one.");
            } else {
                println!("{:<20} {:<18} TARGET", "NAME", "NODE ID");
                println!("{}", "-".repeat(80));
                for peer in peers {
                    let node_id = ticket_node_label(&peer.ticket);
                    let target = if full {
                        peer.ticket.to_string()
                    } else {
                        shorten_ticket(&peer.ticket)
                    };
                    println!("{:<20} {:<18} {}", peer.name, node_id, target);
                }
            }
        }
        PeerCommands::Add { name, ticket } => {
            let ticket = irosh::Ticket::from_str(&ticket).context("invalid ticket format")?;
            storage::save_peer(
                &state,
                &storage::PeerProfile {
                    name: name.clone(),
                    ticket,
                },
            )?;
            println!("✅ Saved peer '{}'", name);
        }
        PeerCommands::Remove { name } => {
            if storage::delete_peer(&state, &name)? {
                println!("✅ Deleted peer '{}'", name);
            } else {
                println!("❌ Peer '{}' not found", name);
            }
        }
        PeerCommands::Info { name } => {
            let peers = storage::list_peers(&state)?;
            if let Some(peer) = peers.into_iter().find(|p| p.name == name) {
                println!("Peer:   {}", peer.name);
                println!("Node ID: {}", peer.ticket.to_addr().id);
                println!("Ticket:  {}", peer.ticket);
            } else {
                println!("❌ Peer '{}' not found", name);
            }
        }
    }

    Ok(())
}

// display helpers are provided by crate::display

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use irosh::{StateConfig, storage};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Parser, Debug)]
#[command(author, version, about = "Manage irosh peers and trust")]
struct Args {
    /// The directory used for persistent state.
    #[arg(short, long, env = "IROSH_STATE_DIR", value_name = "DIR")]
    state: Option<PathBuf>,

    /// Enable verbose logging to stderr.
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List all saved peers.
    List,
    /// Save a new peer ticket.
    Save {
        /// Friendly name for the peer.
        name: String,
        /// The Iroh connection ticket.
        ticket: String,
    },
    /// Delete a saved peer.
    Delete {
        /// Name of the peer to remove.
        name: String,
    },
    /// Inspect the trust store (known host keys).
    Trust,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // 1. Resolve state (use client state by default for the manager).
    let state_root = args
        .state
        .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("client")))
        .context("could not determine state directory")?;

    let state = StateConfig::new(state_root);

    match args.command {
        Commands::List => {
            let peers = storage::list_peers(&state)?;
            println!("Saved peers: {}", state.root().join("peers").display());
            if peers.is_empty() {
                println!("No peers saved.");
                println!("Use `irosh save <name> <ticket>` to add one.");
            } else if args.verbose {
                println!("{:<20} {:<18} TICKET", "NAME", "NODE ID");
                println!("{}", "-".repeat(120));
                for peer in peers {
                    println!(
                        "{:<20} {:<18} {}",
                        peer.name,
                        ticket_node_label(&peer.ticket),
                        peer.ticket,
                    );
                }
            } else {
                println!("{:<20} {:<18} TARGET", "NAME", "NODE ID");
                println!("{}", "-".repeat(96));
                for peer in peers {
                    println!(
                        "{:<20} {:<18} {}",
                        peer.name,
                        ticket_node_label(&peer.ticket),
                        shorten_ticket(&peer.ticket),
                    );
                }
            }
        }
        Commands::Save { name, ticket } => {
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
        Commands::Delete { name } => {
            if storage::delete_peer(&state, &name)? {
                println!("✅ Deleted peer '{}'", name);
            } else {
                println!("❌ Peer '{}' not found", name);
            }
        }
        Commands::Trust => match storage::trust::inspect_trust(&state) {
            Ok(summary) => {
                println!("🔐 Trust Store: {}", state.root().join("trust").display());
                println!("\n[Known Servers (Your client trusts)]");
                for server in summary.known_servers {
                    println!(" - {} (Exists: {})", server.path.display(), server.exists);
                }
                println!("\n[Authorized Clients (Your server trusts)]");
                for client in summary.authorized_clients {
                    println!(" - {} (Exists: {})", client.path.display(), client.exists);
                }
            }
            Err(e) => eprintln!("❌ Failed to read trust store: {}", e),
        },
    }

    Ok(())
}

fn ticket_node_label(ticket: &irosh::Ticket) -> String {
    let node_id = ticket.to_addr().id.to_string();
    shorten_middle(&node_id, 16)
}

fn shorten_ticket(ticket: &irosh::Ticket) -> String {
    shorten_middle(&ticket.to_string(), 40)
}

fn shorten_middle(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }

    let keep = (max_len.saturating_sub(3)) / 2;
    let head = &value[..keep];
    let tail_len = max_len.saturating_sub(keep + 3);
    let tail = &value[value.len() - tail_len..];
    format!("{head}...{tail}")
}

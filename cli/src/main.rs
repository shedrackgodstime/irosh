use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::prelude::*;

mod commands;
mod display;

/// Secure SSH-over-P2P remote access.
///
/// Irosh allows you to establish secure SSH sessions over the Iroh P2P network,
/// bypassing NAT and firewalls without open ports or public IPs.
#[derive(Parser, Debug)]
#[command(name = "irosh", version, about, long_about = None)]
pub struct Args {
    /// Connection target (ticket string or peer alias).
    ///
    /// If provided, initiates a connection immediately. This is a shortcut
    /// for 'irosh connect <target>'.
    pub target: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,

    /// The directory used for persistent state (identity, trust, keys).
    #[arg(
        short,
        long,
        env = "IROSH_STATE_DIR",
        value_name = "DIR",
        global = true
    )]
    pub state: Option<PathBuf>,

    /// Enable verbose logging to stderr.
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Start the irosh P2P SSH server listener.
    Host(commands::host::HostArgs),

    /// Connect to a remote irosh node.
    Connect(commands::connect::ConnectArgs),

    /// Manage saved peers and aliases.
    Peer(commands::peer::PeerArgs),

    /// Manage security trust and authorized keys.
    Trust(commands::trust::TrustArgs),

    /// Show local peer identity and fingerprints.
    Identity,

    /// Show current node status and active sessions.
    Status,

    /// Run connectivity and configuration diagnostics.
    Check,

    /// Manage background services and system integration.
    System(commands::system::SystemArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // 1. Initialize professional logging to stderr.
    let level = if args.verbose { "info" } else { "error" };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));
    let (filter_layer, filter_handle) = tracing_subscriber::reload::Layer::new(filter);

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(filter_layer)
        .init();

    // 2. Dispatch to the appropriate command.
    match (&args.target, &args.command) {
        (Some(target), None) => {
            // Shortcut: irosh <target>
            commands::connect::exec_shortcut(target.clone(), &args, &filter_handle).await?;
        }
        (None, Some(command)) => match command {
            Commands::Host(host_args) => {
                commands::host::exec(host_args.clone(), &args).await?;
            }
            Commands::Connect(connect_args) => {
                commands::connect::exec(connect_args.clone(), &args, &filter_handle).await?;
            }
            Commands::Peer(peer_args) => {
                commands::peer::exec(peer_args.clone(), &args).await?;
            }
            Commands::Trust(trust_args) => {
                commands::trust::exec(trust_args.clone(), &args).await?;
            }
            Commands::Identity => {
                commands::identity::exec(&args).await?;
            }
            Commands::Status => {
                commands::status::exec(&args).await?;
            }
            Commands::Check => {
                commands::check::exec(&args).await?;
            }
            Commands::System(system_args) => {
                commands::system::exec(system_args.clone(), &args).await?;
            }
        },
        (None, None) => {
            // No command or target provided, show status or help.
            commands::status::exec(&args).await?;
        }
        (Some(_), Some(_)) => {
            anyhow::bail!(
                "Cannot provide both a positional target and a subcommand. Try 'irosh --help'."
            );
        }
    }

    Ok(())
}

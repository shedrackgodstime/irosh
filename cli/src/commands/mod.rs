use crate::context::CliContext;
use anyhow::Result;
use clap::Subcommand;

pub mod check;
pub mod config;
pub mod connect;
pub mod dashboard;
pub mod host;
pub mod identity;
pub mod passwd;
pub mod peer;
pub mod system;
pub mod trust;
pub mod wormhole;

#[async_trait::async_trait]
pub trait CommandExec {
    async fn execute(&self, ctx: &CliContext) -> Result<()>;
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Connect to a peer
    Connect {
        /// Target peer (alias, ticket, or wormhole code)
        target: Option<String>,
        /// Forward a local port to a remote address (L:port:R:port)
        #[arg(long, short = 'L')]
        forward: Option<String>,
    },

    /// Run the server in the foreground
    Host {
        /// Secret for stealth mode
        #[arg(long)]
        secret: Option<String>,
    },

    /// Start or manage discovery wormholes
    Wormhole {
        /// Custom code or keyword (status, disable)
        code: Option<String>,
        /// Prompt for a one-time session password (Invite Pattern)
        #[arg(long, short = 'p')]
        passwd: bool,
        /// Make the wormhole persistent across reboots
        #[arg(long)]
        persistent: bool,
    },

    /// Manage the background service
    System {
        #[command(subcommand)]
        action: SystemAction,
    },

    /// Peer address book management
    Peer {
        #[command(subcommand)]
        action: PeerAction,
    },

    /// Trust management (authorized clients)
    Trust {
        #[command(subcommand)]
        action: TrustAction,
    },

    /// Node Password management
    Passwd {
        #[command(subcommand)]
        action: PasswdAction,
    },

    /// Identity management
    Identity {
        #[command(subcommand)]
        action: IdentityAction,
    },

    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Run diagnostics
    Check,
}

#[async_trait::async_trait]
impl CommandExec for Commands {
    async fn execute(&self, ctx: &CliContext) -> Result<()> {
        match self {
            Commands::Connect { target, forward } => {
                connect::exec(target.clone(), forward.clone(), ctx).await
            }
            Commands::Host { secret } => host::exec(secret.clone(), ctx).await,
            Commands::Wormhole {
                code,
                passwd,
                persistent,
            } => wormhole::exec(code.clone(), *passwd, *persistent, ctx).await,
            Commands::System { action } => system::exec(action.clone(), ctx).await,
            Commands::Peer { action } => peer::exec(action.clone(), ctx).await,
            Commands::Trust { action } => trust::exec(action.clone(), ctx).await,
            Commands::Passwd { action } => passwd::exec(action.clone(), ctx).await,
            Commands::Identity { action } => identity::exec(action.clone(), ctx).await,
            Commands::Config { action } => config::exec(action.clone(), ctx).await,
            Commands::Check => check::exec(ctx).await,
        }
    }
}

#[derive(Subcommand, Debug, Clone)]
pub enum SystemAction {
    Install,
    Uninstall,
    Start,
    Stop,
    Restart,
    Status,
    Logs {
        /// Follow log stream
        #[arg(short, long)]
        follow: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum PeerAction {
    List,
    Add { name: String, ticket: String },
    Remove { name: Option<String> },
    Info { name: String },
}

#[derive(Subcommand, Debug, Clone)]
pub enum TrustAction {
    List,
    Revoke { fingerprint: Option<String> },
    Reset,
}

#[derive(Subcommand, Debug, Clone)]
pub enum PasswdAction {
    Set,
    Remove,
    Status,
}

#[derive(Subcommand, Debug, Clone)]
pub enum IdentityAction {
    Show,
    Rotate,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigAction {
    List,
    Get {
        key: String,
    },
    Set {
        key: String,
        value: String,
    },
    Export {
        /// Output file path
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
    },
    Import {
        file: std::path::PathBuf,
    },
}

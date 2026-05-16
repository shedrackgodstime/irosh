use crate::context::CliContext;
use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use std::path::PathBuf;

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
    /// Connect to a remote peer (alias, ticket, or wormhole)
    #[command(
        long_about = "Connects to a remote irosh peer to start an interactive shell.\n\nExamples:\n  irosh connect my-server           # Using a saved alias\n  irosh connect --code apple-pie    # Using a wormhole code\n  irosh connect <ticket-string>     # Using a raw ticket"
    )]
    Connect {
        /// Target peer (alias, ticket, or wormhole code)
        target: Option<String>,
        /// Explicitly connect via wormhole code
        #[arg(long, short = 'c')]
        code: Option<String>,
        /// Explicitly connect via ticket
        #[arg(long, short = 't')]
        ticket: Option<String>,
        /// Forward a local port to a remote address (L:port:R:port)
        #[arg(long, short = 'L')]
        forward: Option<String>,
        /// Secret for stealth mode
        #[arg(long, short = 's')]
        secret: Option<String>,
        /// Run a single remote command and exit
        #[arg(long, short = 'e')]
        exec: Option<String>,
    },

    /// Run the server in the foreground
    #[command(
        long_about = "Starts the irosh server in the current terminal. This is useful for temporary sessions or debugging. Use 'system start' for background hosting."
    )]
    Host {
        /// Secret for stealth mode
        #[arg(long, short = 's')]
        secret: Option<String>,

        /// Force a specific authentication mode
        #[arg(long, value_enum)]
        auth_mode: Option<CliAuthMode>,

        /// Pre-authorize an SSH public key file (headless/scriptable setup)
        #[arg(long)]
        authorize: Option<PathBuf>,

        /// Use simplified output (machine-readable hints)
        #[arg(long)]
        simple: bool,
    },

    /// Start or manage discovery wormholes
    #[command(
        long_about = "Wormholes allow two devices to pair securely using a simple human-readable code word.\n\nExamples:\n  irosh wormhole                # Generate a random pairing code\n  irosh wormhole my-custom-code # Use a specific code"
    )]
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

    /// Manage the background daemon (install, start, stop)
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

    /// Run diagnostics and check system health
    Check,
    /// Alias for 'check'
    Status,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAuthMode {
    /// Only SSH public keys are allowed (Strict/TOFU)
    Key,
    /// Only passwords are allowed
    Password,
    /// Both keys and passwords are allowed
    Combined,
    /// Intelligent auto-detection (default)
    Unified,
}

impl From<CliAuthMode> for irosh::auth::AuthMode {
    fn from(mode: CliAuthMode) -> Self {
        match mode {
            CliAuthMode::Key => irosh::auth::AuthMode::Key,
            CliAuthMode::Password => irosh::auth::AuthMode::Password,
            CliAuthMode::Combined => irosh::auth::AuthMode::Combined,
            CliAuthMode::Unified => irosh::auth::AuthMode::Unified,
        }
    }
}

#[async_trait::async_trait]
impl CommandExec for Commands {
    async fn execute(&self, ctx: &CliContext) -> Result<()> {
        match self {
            Commands::Connect {
                target,
                code,
                ticket,
                forward,
                secret,
                exec,
            } => {
                connect::exec(
                    target.clone(),
                    code.clone(),
                    ticket.clone(),
                    forward.clone(),
                    secret.clone(),
                    exec.clone(),
                    ctx,
                )
                .await
            }
            Commands::Host {
                secret,
                auth_mode,
                authorize,
                simple,
            } => host::exec(secret.clone(), *auth_mode, authorize.clone(), *simple, ctx).await,
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
            Commands::Check | Commands::Status => check::exec(ctx).await,
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
    /// List all saved peers in the address book
    List,
    /// Add a new peer to the address book.
    /// If arguments are omitted, interactive prompts will be shown.
    Add {
        /// A friendly alias for this peer (optional - interactive prompt if omitted)
        name: Option<String>,
        /// The connection ticket (optional - interactive prompt if omitted)
        ticket: Option<String>,
    },
    /// Remove a peer from the address book.
    /// If name is omitted, an interactive selection prompt will be shown.
    Remove {
        /// The alias of the peer to remove
        name: Option<String>,
    },
    /// View detailed information about a saved peer.
    /// If name is omitted, an interactive selection prompt will be shown.
    Info {
        /// The alias of the peer (optional - interactive prompt if omitted)
        name: Option<String>,
    },
    /// Rename (or re-alias) a saved peer.
    /// If names are omitted, an interactive selection and input prompt will be shown.
    Rename {
        /// The current alias of the peer
        old_name: Option<String>,
        /// The new alias to assign
        new_name: Option<String>,
    },
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

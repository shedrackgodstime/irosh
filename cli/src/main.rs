//! Irosh CLI - The thin frontend for P2P SSH.

mod commands;
mod context;
mod display;
mod ui;

use anyhow::Result;
use clap::Parser;
use commands::{CommandExec, Commands};
use context::CliContext;

#[derive(Parser)]
#[command(name = "irosh")]
#[command(version, about = "P2P SSH sessions over Iroh", long_about = None)]
pub struct Args {
    /// Override default state directory
    #[arg(long, env = "IROSH_STATE")]
    pub state: Option<std::path::PathBuf>,

    /// Enable verbose logging (debug level for irosh, info for others)
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Specific log level override (e.g. 'debug', 'trace')
    #[arg(long, global = true)]
    pub log: Option<String>,

    /// Shortcut target for connection (alias, ticket, or wormhole code)
    #[arg(index = 1)]
    pub target: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize professional monochrome logging
    let filter = if args.verbose {
        "irosh=debug,info"
    } else if let Some(custom) = &args.log {
        custom.as_str()
    } else {
        "irosh=info,error"
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(args.verbose)
        .with_ansi(false) // Force monochrome
        .with_level(args.verbose)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false)
        .init();

    let ctx = CliContext::new(args)?;

    // Handle root command shortcut (irosh <target>)
    if let Some(target) = ctx.args.target.clone() {
        if ctx.args.command.is_none() {
            return commands::connect::exec_shortcut(&target, &ctx).await;
        }
    }

    if let Some(cmd) = &ctx.args.command {
        cmd.execute(&ctx).await
    } else {
        commands::dashboard::exec(&ctx).await
    }
}

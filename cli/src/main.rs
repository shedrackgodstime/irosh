//! Irosh CLI - The thin frontend for P2P SSH.

mod commands;
mod context;
mod display;
mod ui;

use clap::Parser;
use commands::{CommandExec, Commands};
use context::CliContext;
use ui::Ui;

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
async fn main() {
    let args = Args::parse();

    // Initialize professional monochrome logging
    let filter = if args.verbose {
        "irosh=debug,info"
    } else if let Some(custom) = &args.log {
        custom.as_str()
    } else {
        "irosh=warn,error"
    };

    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(args.verbose)
        .with_ansi(false)
        .with_level(args.verbose)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false);

    if args.verbose {
        builder.init();
    } else {
        builder.without_time().init();
    }

    let ctx = match CliContext::new(args) {
        Ok(c) => c,
        Err(e) => {
            use crate::ui::Ui;
            Ui::error(&format!("Initialization failed: {}", e));
            std::process::exit(1);
        }
    };

    let res = if let Some(target) = ctx.args.target.clone() {
        if ctx.args.command.is_none() {
            commands::connect::exec_shortcut(&target, &ctx).await
        } else {
            match &ctx.args.command {
                Some(cmd) => cmd.execute(&ctx).await,
                None => commands::dashboard::exec(&ctx).await,
            }
        }
    } else {
        match &ctx.args.command {
            Some(cmd) => cmd.execute(&ctx).await,
            None => commands::dashboard::exec(&ctx).await,
        }
    };

    if let Err(e) = res {
        if ctx.args.verbose {
            Ui::error(&format!("{:#}", e));
        } else {
            Ui::error(&format!("{}", e));
            eprintln!("  Tip: Run with --verbose for full diagnostic details.");
        }
        std::process::exit(1);
    }
}

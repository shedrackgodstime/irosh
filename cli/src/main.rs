//! Irosh CLI - The thin frontend for P2P SSH.
#![deny(unused_lifetimes)]
#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::double_must_use)]
#![warn(clippy::missing_errors_doc)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::suspicious)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(trivial_casts)]
#![warn(trivial_numeric_casts)]

mod commands;
mod context;
mod display;
mod error;
/// Terminal output helpers for JSON and plain-text formatting.
pub mod output;
mod terminal;
mod ui;

use clap::Parser;
use commands::{CommandExec, Commands};
use context::CliContext;
use ui::Ui;

/// Command-line arguments for the irosh CLI.
#[derive(Parser)]
#[command(name = "irosh")]
#[command(
    version,
    about = "Secure P2P SSH sessions with zero configuration",
    long_about = "Irosh provides secure, encrypted terminal sessions and file transfers over the Iroh P2P network.\n\nKey Features:\n  - Zero-Config: Connect through NATs and firewalls without port forwarding.\n  - Wormhole: Pair new devices using human-readable code words.\n  - Stealth Mode: Use shared secrets to make your server invisible to unauthorized scans.\n  - Integrated Transfers: Seamlessly 'put' and 'get' files over the P2P tunnel."
)]
#[derive(Clone, Debug, PartialEq, Eq)]
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

    /// Output results as JSON for automation
    #[arg(long, global = true)]
    pub json: bool,

    /// Automatically confirm all danger prompts
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

    /// Shortcut target for connection (alias, ticket, or wormhole code)
    #[arg(index = 1)]
    pub target: Option<String>,

    /// Optional subcommand (connect, host, wormhole, etc.)
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[tokio::main]
async fn main() {
    #[cfg(windows)]
    {
        // Attempt to run as a service. This will succeed if started by the SCM,
        // and fail immediately if started from a console.
        if irosh::sys::windows::service::run_service().is_ok() {
            return;
        }
    }

    let args = Args::parse();

    if args.json {
        crate::output::JSON_MODE.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    if args.yes {
        crate::output::YES_MODE.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    // Initialize professional monochrome logging
    let filter = if args.verbose {
        "irosh=debug,info"
    } else if let Some(custom) = &args.log {
        custom.as_str()
    } else {
        "irosh=warn,error"
    };

    #[cfg(windows)]
    struct CrlfWriter<W: std::io::Write>(W);
    #[cfg(windows)]
    impl<W: std::io::Write> std::io::Write for CrlfWriter<W> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut last = 0;
            for (i, &byte) in buf.iter().enumerate() {
                if byte == b'\n' {
                    self.0.write_all(&buf[last..i])?;
                    self.0.write_all(b"\r\n")?;
                    last = i + 1;
                }
            }
            self.0.write_all(&buf[last..])?;
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }

    let builder = tracing_subscriber::fmt().with_env_filter(filter);

    #[cfg(windows)]
    let builder = builder.with_writer(move || CrlfWriter(std::io::stderr()));
    #[cfg(not(windows))]
    let builder = builder.with_writer(std::io::stderr);

    let builder = builder
        .with_target(args.verbose)
        .with_ansi(true)
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
            if crate::output::JSON_MODE.load(std::sync::atomic::Ordering::SeqCst) {
                crate::output::print_error(&format!("Initialization failed: {e}"), "init_failed");
            } else {
                use crate::ui::Ui;
                Ui::error(
                    &format!("initialization failed: {e}"),
                    Some(crate::ui::messages::TIP_INIT_FAILED),
                );
            }
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
        if ctx.args.json {
            crate::output::print_error(&format!("{e:#}"), "command_failed");
        } else {
            let msg = if ctx.args.verbose {
                format!("{e:#}")
            } else {
                format!("{e}")
            };
            let tip = error::CliError::classify(&e).tip();
            Ui::error(&msg, Some(tip));
        }
        std::process::exit(1);
    }
}

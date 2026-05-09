//! Irosh CLI - The thin frontend for P2P SSH.

mod commands;
mod context;
mod display;
pub mod output;
mod ui;

use clap::Parser;
use commands::{CommandExec, Commands};
use context::CliContext;
use ui::Ui;

#[derive(Parser)]
#[command(name = "irosh")]
#[command(
    version,
    about = "Secure P2P SSH sessions with zero configuration",
    long_about = "Irosh provides secure, encrypted terminal sessions and file transfers over the Iroh P2P network.\n\nKey Features:\n  - Zero-Config: Connect through NATs and firewalls without port forwarding.\n  - Wormhole: Pair new devices using human-readable code words.\n  - Stealth Mode: Use shared secrets to make your server invisible to unauthorized scans.\n  - Integrated Transfers: Seamlessly 'put' and 'get' files over the P2P tunnel."
)]
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

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[tokio::main]
async fn main() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::*;
        unsafe {
            let stdout = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut mode = 0;
            if GetConsoleMode(stdout, &mut mode) != 0 {
                SetConsoleMode(stdout, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
            let stderr = GetStdHandle(STD_ERROR_HANDLE);
            if GetConsoleMode(stderr, &mut mode) != 0 {
                SetConsoleMode(stderr, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
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
                crate::output::print_error(&format!("Initialization failed: {}", e), "init_failed");
            } else {
                use crate::ui::Ui;
                Ui::error(&format!("Initialization failed: {}", e));
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
            crate::output::print_error(&format!("{:#}", e), "command_failed");
        } else {
            if ctx.args.verbose {
                Ui::error(&format!("{:#}", e));
            } else {
                Ui::error(&format!("{}", e));
                eprintln!("  Tip: Run with --verbose for full diagnostic details.");
            }
        }
        std::process::exit(1);
    }
}

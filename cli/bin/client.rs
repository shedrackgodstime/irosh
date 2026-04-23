#[path = "client/commands.rs"]
mod commands;
#[path = "client/local.rs"]
mod local;
#[path = "client/support/mod.rs"]
mod support;
#[path = "client/transfer/mod.rs"]
mod transfer;

use anyhow::{Context, Result};
use clap::Parser;
use irosh::session::{RawTerminal, current_terminal_size};
use irosh::{
    Client, ClientOptions, PtyOptions, SecurityConfig, Session, SessionEvent, StateConfig,
};
use std::io::IsTerminal;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing_subscriber::{EnvFilter, fmt, prelude::*, reload};

use crate::commands::{
    handle_connect_error, maybe_autosave_alias, print_identity, print_saved_peers,
};
use crate::local::{InputOutcome, TransferContext, process_stdin_chunk};
use crate::support::{normalize_path, suppress_interactive_logs};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Connect to an irosh P2P SSH server",
    long_about = "Dial any irosh node across the planet using its connection ticket or a saved peer name. \
                  This establishes a cryptographically secure SSH session over the public Iroh network, \
                  bypassing NAT and firewalls automatically.\n\n\
                  TOFU (Trust On First Use):\n  \
                  Security is paramount. The first time you connect to a server, its machine identity \
                  is pinned to your local trust store. Future connections are verified against this \
                  identity to block Man-In-The-Middle (MITM) attacks.\n\n\
                  EXAMPLES:\n  \
                  Connect via ticket: irosh-client <TICKET>\n  \
                  Connect to saved peer: irosh-client my-server\n  \
                  List saved peers: irosh list\n  \
                  Skip TOFU (Dangerous): irosh-client <TICKET> --insecure"
)]
struct Args {
    /// The connection ticket or a saved peer name.
    #[arg(help = "Connection target (ticket string or peer alias)")]
    target: Option<String>,

    /// State directory for keys and trust records. Defaults to ~/.irosh/client.
    #[arg(short, long, env = "IROSH_STATE_DIR", value_name = "DIR")]
    state: Option<PathBuf>,

    /// Bypass host key verification (Danger: vulnerable to MITM).
    #[arg(long, help = "Skip TOFU (Trust On First Use) verification")]
    insecure: bool,

    /// Stealth mode secret. Must match the server's passphrase.
    #[arg(long, env = "IROSH_SECRET", value_name = "PASSPHRASE")]
    secret: Option<String>,

    /// Show your own public key (for whitelisting on a server) and exit.
    #[arg(long)]
    identity: bool,

    /// List all saved peers and exit.
    #[arg(long)]
    list: bool,

    /// Alias for --list.
    #[arg(long)]
    peers: bool,

    /// Enable verbose network logging to stderr.
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // 1. Initialize logging (to stderr only, to keep stdout clean for the shell).
    let level = if args.verbose { "info" } else { "error" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let (filter_layer, filter_handle) = reload::Layer::new(filter);

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(filter_layer)
        .init();

    // 2. Resolve state directory.
    let state_root = args
        .state
        .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("client")))
        .context("could not determine state directory")?;

    let state = StateConfig::new(state_root);

    // 2.5 Handle Identity interrogation.
    if args.identity {
        print_identity(&state).await?;
        return Ok(());
    }

    // 2.6 Handle Peer listing.
    if args.list || args.peers {
        print_saved_peers(&state, args.verbose)?;
        return Ok(());
    }

    let mut options = ClientOptions::new(state.clone()).security(SecurityConfig {
        host_key_policy: if args.insecure {
            irosh::config::HostKeyPolicy::AcceptAll
        } else {
            irosh::config::HostKeyPolicy::Tofu
        },
    });
    if let Some(secret) = args.secret {
        options = options.secret(secret);
    }

    // 3. Resolve target.
    let target_str = args
        .target
        .as_ref()
        .context("A target (ticket or alias) is required unless using --identity. Try --help.")?;
    let target = Client::parse_target(options.state(), target_str)?;

    // 4. Connect.
    println!("📡 Dialing P2P node...");
    let session_res = Client::connect(&options, target).await;

    let mut session = match session_res {
        Ok(s) => s,
        Err(e) => handle_connect_error(e, &options).await?,
    };

    maybe_autosave_alias(&session, &options, target_str)?;

    // 5. Setup Terminal Driving.
    let stdin_is_tty = std::io::stdin().is_terminal();
    let stdout_is_tty = std::io::stdout().is_terminal();

    #[cfg(unix)]
    let _raw_terminal = if stdin_is_tty && stdout_is_tty {
        // Use STDIN_FILENO (0) directly to avoid libc dependency in CLI.
        Some(RawTerminal::new(0)?)
    } else {
        None
    };

    // 6. Request PTY and Shell.
    if stdin_is_tty && stdout_is_tty {
        let size = current_terminal_size();
        let term = std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string());
        session.request_pty(PtyOptions::new(term, size)).await?;
    }

    session.start_shell().await?;

    if stdin_is_tty && stdout_is_tty {
        suppress_interactive_logs(&filter_handle);
    }

    // 7. Drive the session.
    drive_session(session).await?;

    Ok(())
}

async fn drive_session(mut session: Session) -> Result<()> {
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut stderr = tokio::io::stderr();
    let mut buf = vec![0u8; 4096];

    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let mut pending_line: Vec<u8> = Vec::new();
    let mut local_command: Option<Vec<u8>> = None;
    let mut transfer_context = TransferContext {
        local_root: normalize_path(
            std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir()),
        ),
    };

    #[cfg(unix)]
    let mut sigwinch = if interactive {
        Some(tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::window_change(),
        )?)
    } else {
        None
    };

    #[cfg(not(unix))]
    let mut sigwinch: Option<bool> = None; // Dummy for Windows

    loop {
        tokio::select! {
            // Read from local stdin and push to remote channel.
            res = stdin.read(&mut buf) => {
                match res? {
                    0 => {
                        session.eof().await?;
                        break;
                    }
                    n => {
                        if interactive {
                            let outcome = process_stdin_chunk(
                                &mut session,
                                &mut stdout,
                                &buf[..n],
                                &mut pending_line,
                                &mut local_command,
                                &mut transfer_context,
                            ).await?;
                            if matches!(outcome, InputOutcome::Disconnect) {
                                break;
                            }
                        } else {
                            session.send(&buf[..n]).await?;
                        }
                    }
                }
            }
            // Watch for window resize.
            _ = async {
                #[cfg(unix)]
                if let Some(s) = sigwinch.as_mut() {
                    s.recv().await;
                } else {
                    std::future::pending::<()>().await;
                }

                #[cfg(not(unix))]
                std::future::pending::<()>().await;
            } => {
                let size = current_terminal_size();
                let _ = session.resize(size).await;
            }
            // Read from remote channel and push to local stdout/stderr.
            event = session.next_event() => {
                let Some(event) = event? else { break; };
                match event {
                    SessionEvent::Data(data) => {
                        stdout.write_all(&data).await?;
                        stdout.flush().await?;
                    }
                    SessionEvent::ExtendedData(data, _) => {
                        stderr.write_all(&data).await?;
                        stderr.flush().await?;
                    }
                    SessionEvent::ExitStatus(_) | SessionEvent::Closed => break,
                    _ => {}
                }
            }
        }
    }

    let _ = session.disconnect().await;
    Ok(())
}

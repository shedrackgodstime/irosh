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

    /// Local port forwarding (e.g. 8080:localhost:80)
    #[arg(
        short = 'L',
        long = "forward",
        value_name = "LOCAL:REMOTE_HOST:REMOTE_PORT"
    )]
    forward: Vec<String>,

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
        tracing::info!("Requesting PTY size={:?} term={}", size, term);
        session.request_pty(PtyOptions::new(term, size)).await?;
    }

    tracing::info!("Starting remote shell");
    session.start_shell().await?;

    // 6.5 Setup Port Forwarding.
    let mut tunnels = Vec::new();
    for f in &args.forward {
        tracing::info!("Setting up tunnel: {}", f);
        let parts: Vec<&str> = f.split(':').collect();
        if parts.len() != 3 {
            eprintln!(
                "Invalid forward specification: {}. Use LOCAL_PORT:REMOTE_HOST:REMOTE_PORT",
                f
            );
            continue;
        }
        let local_port: u16 = parts[0].parse().context("Invalid local port")?;
        let remote_host = parts[1].to_string();
        let remote_port: u32 = parts[2].parse().context("Invalid remote port")?;

        let local_addr = format!("127.0.0.1:{}", local_port);
        match session
            .local_forward(&local_addr, remote_host.clone(), remote_port)
            .await
        {
            Ok((handle, bound_addr)) => {
                println!(
                    "🔗 Forwarding {} -> {}:{}",
                    bound_addr, remote_host, remote_port
                );
                tunnels.push(handle);
            }
            Err(e) => {
                eprintln!("Failed to start forward for {}: {}", f, e);
            }
        }
    }

    if stdin_is_tty && stdout_is_tty && !args.verbose {
        suppress_interactive_logs(&filter_handle);
    }

    tracing::info!("Driving session loop");
    // 7. Drive the session.
    drive_session(session).await?;

    Ok(())
}

async fn drive_session(mut session: Session) -> Result<()> {
    tracing::debug!("drive_session started");
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
    let mut input_state = crate::local::LocalInputState::Normal;

    let history_path = dirs::home_dir().map(|p| p.join(".irosh").join("client_history"));
    let mut history = crate::support::CommandHistory::new(history_path);

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
                        tracing::debug!("stdin EOF");
                        session.eof().await?;
                        break;
                    }
                    n => {
                        tracing::debug!("stdin read {} bytes", n);
                        if interactive {
                            let mut state = crate::local::LocalSessionState {
                                pending_line: &mut pending_line,
                                local_command: &mut local_command,
                                transfer_context: &mut transfer_context,
                                input_state: &mut input_state,
                                history: &mut history,
                            };
                            let outcome = process_stdin_chunk(
                                &mut session,
                                &mut stdout,
                                &buf[..n],
                                &mut state,
                            )
                            .await?;
                            if matches!(outcome, InputOutcome::Disconnect) {
                                tracing::info!("Client requested disconnect via local command");
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
                tracing::debug!("window resize detected");
                let size = current_terminal_size();
                let _ = session.resize(size).await;
            }
            // Read from remote channel and push to local stdout/stderr.
            event = session.next_event() => {
                match event? {
                    Some(event) => {
                        match event {
                            SessionEvent::Data(data) => {
                                tracing::debug!("Remote Data: {} bytes", data.len());
                                stdout.write_all(&data).await?;
                                stdout.flush().await?;
                            }
                            SessionEvent::ExtendedData(data, ext) => {
                                tracing::debug!("Remote ExtendedData ({}): {} bytes", ext, data.len());
                                stderr.write_all(&data).await?;
                                stderr.flush().await?;
                            }
                            SessionEvent::ExitStatus(code) => {
                                tracing::info!("Remote process exited with status {}", code);
                                // Don't break immediately, wait for Closed
                            }
                            SessionEvent::Closed => {
                                tracing::info!("Remote session closed");
                                break;
                            }
                            SessionEvent::Ignore => {
                                tracing::debug!("Ignoring internal session event");
                            }
                            _ => {}
                        }
                    }
                    None => {
                        tracing::info!("Session event stream ended (None)");
                        break;
                    }
                }
            }
        }
    }

    tracing::info!("Disconnecting session");
    let _ = session.disconnect().await;
    Ok(())
}

use anyhow::{Context, Result};
use clap::Args;
use indicatif::{ProgressBar, ProgressStyle};
use irosh::session::current_terminal_size;
#[cfg(unix)]
use irosh::session::{AsyncStdin, RawTerminal};

use irosh::{
    Client, ClientOptions, PtyOptions, SecurityConfig, Session, SessionEvent, StateConfig,
};
use std::io::IsTerminal;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use tracing_subscriber::{EnvFilter, reload};

pub mod commands;
pub mod input;
pub mod support;
pub mod transfer;

use self::commands::{handle_connect_error, maybe_autosave_alias};
use self::input::InputEngine;
use self::support::{normalize_path, suppress_interactive_logs};
use self::transfer::TransferContext;
use crate::Args as GlobalArgs;

#[derive(Args, Debug, Clone)]
pub struct ConnectArgs {
    /// The connection ticket or a saved peer name.
    #[arg(help = "Connection target (ticket string or peer alias)")]
    pub target: String,

    /// Bypass host key verification (Danger: vulnerable to MITM).
    #[arg(long, help = "Skip TOFU (Trust On First Use) verification")]
    pub insecure: bool,

    /// Stealth mode secret. Must match the server's passphrase.
    #[arg(long, env = "IROSH_SECRET", value_name = "PASSPHRASE")]
    pub secret: Option<String>,

    /// Username for password authentication fallback.
    #[arg(long, env = "IROSH_USER", value_name = "USER")]
    pub auth_user: Option<String>,

    /// Password for password authentication fallback.
    #[arg(long, env = "IROSH_PASSWORD", value_name = "PASSWORD")]
    pub auth_password: Option<String>,

    /// Local port forwarding (e.g. 8080:localhost:80)
    #[arg(
        short = 'L',
        long = "forward",
        value_name = "LOCAL:REMOTE_HOST:REMOTE_PORT"
    )]
    pub forward: Vec<String>,

    /// Remote port forwarding (e.g. 8080:localhost:80)
    #[arg(
        short = 'R',
        long = "remote-forward",
        value_name = "REMOTE:LOCAL_HOST:LOCAL_PORT"
    )]
    pub remote_forward: Vec<String>,
}

#[derive(Debug)]
struct CliPasswordPrompter;

impl irosh::PasswordPrompter for CliPasswordPrompter {
    fn prompt_password(&self, _user: &str) -> Option<String> {
        eprintln!("Public key rejected. Server requires a password.");
        dialoguer::Password::new()
            .with_prompt("Password")
            .interact()
            .ok()
    }
}

/// Entry point for 'irosh <target>' shortcut.
pub async fn exec_shortcut<S>(
    target: String,
    global_args: &GlobalArgs,
    filter_handle: &reload::Handle<EnvFilter, S>,
) -> Result<()>
where
    S: tracing::Subscriber,
{
    let connect_args = ConnectArgs {
        target,
        insecure: false, // Default to secure
        secret: None,
        auth_user: None,
        auth_password: None,
        forward: Vec::new(),
        remote_forward: Vec::new(),
    };
    exec(connect_args, global_args, filter_handle).await
}

pub async fn exec<S>(
    connect_args: ConnectArgs,
    global_args: &GlobalArgs,
    filter_handle: &reload::Handle<EnvFilter, S>,
) -> Result<()>
where
    S: tracing::Subscriber,
{
    // 2. Resolve state directory.
    let state_root = global_args
        .state
        .clone()
        .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("client")))
        .context("could not determine state directory")?;

    let state = StateConfig::new(state_root);

    let mut options = ClientOptions::new(state.clone()).security(SecurityConfig {
        host_key_policy: if connect_args.insecure {
            irosh::config::HostKeyPolicy::AcceptAll
        } else {
            irosh::config::HostKeyPolicy::Tofu
        },
    });
    if let Some(secret) = connect_args.secret {
        options = options.secret(secret);
    }
    if let Some(password) = connect_args.auth_password {
        let user = connect_args
            .auth_user
            .unwrap_or_else(|| "admin".to_string());
        options = options.credentials(irosh::auth::Credentials::new(user, password));
    } else {
        options = options.password_prompter(CliPasswordPrompter);
    }

    // 3. Resolve target.
    let target = Client::parse_target(options.state(), &connect_args.target)?;

    // 4. Connect.
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈")
            .template("{spinner:.cyan} {msg}")?,
    );
    pb.set_message("Dialing P2P node...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    // Separate P2P connection from SSH handshake to avoid spinner/prompt conflict.
    let connection_res = Client::dial_p2p(&options, target).await;
    pb.finish_and_clear();

    let connection = match connection_res {
        Ok(c) => c,
        Err(e) => handle_connect_error(e, &options).await?,
    };

    let session_res = Client::establish_session(&options, connection).await;
    let mut session = match session_res {
        Ok(s) => s,
        Err(e) => handle_connect_error(e, &options).await?,
    };


    maybe_autosave_alias(&session, &options, &connect_args.target)?;

    // 5. Setup Terminal Driving.
    let stdin_is_tty = std::io::stdin().is_terminal();
    let stdout_is_tty = std::io::stdout().is_terminal();

    #[cfg(unix)]
    let _raw_terminal = if stdin_is_tty && stdout_is_tty {
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

    // 6.5 Setup Port Forwarding.
    let mut tunnels = Vec::new();
    for f in &connect_args.forward {
        let parts: Vec<&str> = f.split(':').collect();
        if parts.len() != 3 {
            eprintln!(
                "Invalid forward: {}. Use LOCAL_PORT:REMOTE_HOST:REMOTE_PORT",
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
                    "🔗 Local forward: {} -> {}:{}",
                    bound_addr, remote_host, remote_port
                );
                tunnels.push(handle);
            }
            Err(e) => eprintln!("Failed to start local forward for {}: {}", f, e),
        }
    }

    for f in &connect_args.remote_forward {
        let parts: Vec<&str> = f.split(':').collect();
        if parts.len() != 3 {
            eprintln!(
                "Invalid remote forward: {}. Use REMOTE_PORT:LOCAL_HOST:LOCAL_PORT",
                f
            );
            continue;
        }
        let remote_port: u32 = parts[0].parse().context("Invalid remote port")?;
        let local_host = parts[1].to_string();
        let local_port: u16 = parts[2].parse().context("Invalid local port")?;

        match session
            .remote_forward(
                "0.0.0.0".to_string(),
                remote_port,
                local_host.clone(),
                local_port,
            )
            .await
        {
            Ok(()) => println!(
                "🔗 Remote forward: [remote]:{} -> {}:{}",
                remote_port, local_host, local_port
            ),
            Err(e) => eprintln!("Failed to start remote forward for {}: {}", f, e),
        }
    }

    if stdin_is_tty && stdout_is_tty && !global_args.verbose {
        suppress_interactive_logs(filter_handle);
    }

    // 7. Drive the session.
    let res = drive_session(session).await;

    // 8. Cleanup.
    for t in tunnels {
        t.abort();
    }

    res?;
    Ok(())
}

async fn drive_session(mut session: Session) -> Result<()> {
    #[cfg(unix)]
    let mut stdin = AsyncStdin::new()?;
    #[cfg(not(unix))]
    let mut stdin = tokio::io::stdin();

    let mut stdout = tokio::io::stdout();
    let mut stderr = tokio::io::stderr();
    let mut buf = vec![0u8; 4096];

    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let initial_cwd =
        normalize_path(std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir()));
    let transfer_context = TransferContext {
        local_root: initial_cwd,
    };
    let mut input = InputEngine::new(transfer_context);

    #[cfg(unix)]
    let mut sigwinch = if interactive {
        Some(tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::window_change(),
        )?)
    } else {
        None
    };
    #[cfg(not(unix))]
    let _sigwinch: Option<bool> = None;

    loop {
        tokio::select! {
            res = stdin.read(&mut buf) => {
                match res? {
                    0 => {
                        session.eof().await?;
                        break;
                    }
                    n => {
                        if interactive {
                            input.process_stdin_chunk(
                                &mut session,
                                &mut stdout,
                                &mut stdin,
                                &buf[..n],
                            )
                            .await?;
                        } else {
                            session.send(&buf[..n]).await?;
                        }
                    }
                }
            }
            _ = async {
                #[cfg(unix)]
                if let Some(s) = sigwinch.as_mut() { s.recv().await; }
                else { std::future::pending::<()>().await; }
                #[cfg(not(unix))]
                std::future::pending::<()>().await;
            } => {
                let size = current_terminal_size();
                let _ = session.resize(size).await;
            }
            event = session.next_event() => {
                match event? {
                    Some(event) => {
                        match event {
                            SessionEvent::Data(data) => {
                                input.observe_remote_bytes(&data);
                                stdout.write_all(&data).await?;
                                stdout.flush().await?;
                                input.redraw_after_remote_output(&mut stdout).await?;
                            }
                            SessionEvent::ExtendedData(data, _) => {
                                input.observe_remote_bytes(&data);
                                stderr.write_all(&data).await?;
                                stderr.flush().await?;
                                input.redraw_after_remote_output(&mut stdout).await?;
                            }
                            SessionEvent::Closed => {
                                stdout.write_all("\r\n🔒 Session closed. Returning to local shell...\r\n".as_bytes()).await?;
                                stdout.flush().await?;
                                break;
                            }
                            _ => {}
                        }
                    }
                    None => break,
                }
            }
        }
    }

    let _ = session.disconnect().await;
    Ok(())
}

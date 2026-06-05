use crate::display;
use crate::terminal::TerminalGuard;
use crate::ui::Ui;
use anyhow::Result;
use irosh::sys::current_terminal_size;
use irosh::{Client, ClientOptions, PtyOptions, Session};
use std::io::{IsTerminal, Write};

mod completion;
mod editor;
mod history;
mod input;
mod prompt;
mod session;
mod transfer;
mod tunnels;

use session::DisconnectReason;

#[derive(Debug, Clone)]
struct CliPasswordPrompter {
    pb: Option<indicatif::ProgressBar>,
}

impl irosh::PasswordPrompter for CliPasswordPrompter {
    fn prompt_password(&self, _user: &str) -> Option<String> {
        if let Some(ref pb) = self.pb {
            pb.suspend(|| Ui::password_input("Public key rejected. Server requires a password"))
        } else {
            Ui::password_input("Public key rejected. Server requires a password")
        }
    }
}

use crate::context::CliContext;

/// Entry point for the shortcut 'irosh <target>'
#[must_use]
pub async fn exec_shortcut(target: &str, ctx: &CliContext) -> Result<()> {
    exec_internal(Some(target.to_string()), None, None, None, None, None, ctx).await
}

/// Entry point for 'irosh connect <target>'
#[must_use]
pub async fn exec(
    target: Option<String>,
    code: Option<String>,
    ticket: Option<String>,
    forward: Option<String>,
    secret: Option<String>,
    exec_cmd: Option<String>,
    ctx: &CliContext,
) -> Result<()> {
    exec_internal(target, code, ticket, forward, secret, exec_cmd, ctx).await
}

// ── Connect Phase State Machine ──────────────────────────────────────────────

/// A single state in the connect flow.
enum ConnectPhase {
    /// Resolve a target string into a ResolvedTarget (ticket, wormhole, or alias).
    Resolve { target_str: Option<String> },
    /// Dial the P2P connection and authenticate.
    Dial {
        target: irosh::ResolvedTarget,
        attempts: u32,
    },
    /// Setup the session (auto-save, PTY, shell, forwarding).
    Setup {
        session: Session,
        ticket: irosh::transport::ticket::Ticket,
        is_pairing: bool,
    },
    /// Active shell session.
    Shell {
        session: Session,
        engine: input::InputEngine,
        guard: Option<TerminalGuard>,
    },
    /// Disconnect and render summary.
    Close { reason: DisconnectReason },
}

/// Shared context threaded through all phases.
struct ConnectCtx {
    state: irosh::StateConfig,
    options: ClientOptions,
    forward: Option<String>,
    exec_cmd: Option<String>,
    peer_alias: Option<String>,
}

async fn exec_internal(
    target_str: Option<String>,
    code_str: Option<String>,
    ticket_str: Option<String>,
    forward_str: Option<String>,
    secret_str: Option<String>,
    exec_cmd: Option<String>,
    ctx: &CliContext,
) -> Result<()> {
    let state = ctx.state.clone();
    let config = irosh::storage::load_config(&state)?;

    let mut options = ClientOptions::new(state.clone());

    if let Some(secret) = secret_str.or(config.stealth_secret) {
        options = options.secret(secret);
    }

    if let Some(relay_url) = &config.relay_url {
        let mode = irosh::transport::iroh::parse_relay_mode(relay_url)?;
        options = options.relay_mode(mode);
    }

    // Inject explicit ticket/code into the target string for the resolve phase.
    let initial_target = ticket_str.or(code_str).or(target_str);

    let mut sm = ConnectCtx {
        state,
        options,
        forward: forward_str,
        exec_cmd,
        peer_alias: None,
    };

    let mut phase = ConnectPhase::Resolve {
        target_str: initial_target,
    };

    loop {
        phase = match phase {
            ConnectPhase::Resolve { target_str } => phase_resolve(target_str, &mut sm)?,
            ConnectPhase::Dial { target, attempts } => {
                phase_dial(target, attempts, &mut sm).await?
            }
            ConnectPhase::Setup {
                session,
                ticket,
                is_pairing,
            } => phase_setup(session, ticket, is_pairing, &mut sm).await?,
            ConnectPhase::Shell {
                session,
                engine,
                guard,
            } => phase_shell(session, engine, guard, &sm).await,
            ConnectPhase::Close { reason } => {
                phase_close(&reason, &sm);
                return Ok(());
            }
        };
    }
}

// ── Phase: Resolve ───────────────────────────────────────────────────────────

fn phase_resolve(target_str: Option<String>, sm: &mut ConnectCtx) -> Result<ConnectPhase> {
    use irosh::ResolvedTarget;

    let raw_target = if let Some(t) = target_str {
        t
    } else {
        let peers = irosh::storage::list_peers(&sm.state)?;
        if peers.is_empty() {
            Ui::warn("Address book is empty", "You haven't saved any peers yet.");
            Ui::info("To connect, you can:");
            Ui::info("  1. Use a wormhole code:   irosh <code-word>");
            Ui::info("  2. Use a full ticket:     irosh <ticket-string>");
            Ui::info("  3. Add a peer manually:   irosh peer add <name> <ticket>");
            Ui::blank();

            match Ui::input("Enter a wormhole code or ticket", None) {
                Some(val) if !val.trim().is_empty() => val.trim().to_string(),
                _ => anyhow::bail!("No target specified."),
            }
        } else {
            let mut items = vec!["[Use a wormhole code or ticket]".to_string()];
            items.extend(
                peers
                    .iter()
                    .map(|p| format!("[{}] {}", p.name, display::shorten_ticket(&p.ticket))),
            );

            match Ui::select("Select a peer to connect", &items) {
                Some(0) => match Ui::input("Enter a wormhole code or ticket", None) {
                    Some(val) if !val.trim().is_empty() => val.trim().to_string(),
                    _ => anyhow::bail!("No target specified."),
                },
                Some(idx) => peers[idx - 1].name.clone(),
                None => anyhow::bail!("Connection cancelled."),
            }
        }
    };

    let resolved = Client::parse_target(sm.options.state(), &raw_target)?;

    match &resolved {
        ResolvedTarget::Ticket(_) => {
            let is_alias = irosh::storage::list_peers(&sm.state)?
                .iter()
                .any(|p| p.name == raw_target);

            if is_alias {
                Ui::info(&format!("Connecting to saved peer: {raw_target}"));
            } else {
                Ui::info("Connecting via direct ticket...");
            }
        }
        ResolvedTarget::WormholeCode(code) => {
            Ui::info(&format!("Attempting wormhole connection: {code}"));
        }
        _ => {}
    }

    Ok(ConnectPhase::Dial {
        target: resolved,
        attempts: 0,
    })
}

// ── Phase: Dial ──────────────────────────────────────────────────────────────

async fn phase_dial(
    target: irosh::ResolvedTarget,
    _attempts: u32,
    sm: &mut ConnectCtx,
) -> Result<ConnectPhase> {
    use irosh::ResolvedTarget;

    let pb = Ui::spinner("Establishing connection...");
    let opts = sm.options.clone().password_prompter(CliPasswordPrompter {
        pb: Some(pb.clone()),
    });

    let (ticket, is_pairing) = tokio::select! {
        res = async {
            match target {
                ResolvedTarget::Ticket(t) => Ok((t, false)),
                ResolvedTarget::WormholeCode(ref code) => {
                    pb.set_message(format!("Searching for wormhole: {code}..."));
                    Client::connect_wormhole(&opts, code).await.map(|t| (t, true))
                }
                _ => unreachable!(),
            }
        } => res?,
        _ = tokio::signal::ctrl_c() => {
            pb.finish_with_message("Cancelled");
            anyhow::bail!("Connection cancelled by user.");
        }
    };

    let connection_info = tokio::select! {
        res = Client::dial_p2p(&opts, ticket.clone(), is_pairing) => {
            match res {
                Ok(c) => c,
                Err(e) => {
                    pb.finish_with_message("Failed");
                    if is_pairing {
                        auto_save_temp_peer(&sm.state, &ticket);
                    }
                    return Err(e.into());
                }
            }
        },
        _ = tokio::signal::ctrl_c() => {
            pb.finish_with_message("Cancelled");
            if is_pairing {
                auto_save_temp_peer(&sm.state, &ticket);
            }
            anyhow::bail!("Connection cancelled by user.");
        }
    };

    pb.set_message("Authenticating...");
    let session = tokio::select! {
        res = Client::establish_session(&opts, connection_info) => {
            match res {
                Ok(s) => s,
                Err(e) => {
                    pb.finish_with_message("Failed");
                    if is_pairing {
                        auto_save_temp_peer(&sm.state, &ticket);
                    }
                    return Err(e.into());
                }
            }
        },
        _ = tokio::signal::ctrl_c() => {
            pb.finish_with_message("Cancelled");
            if is_pairing {
                auto_save_temp_peer(&sm.state, &ticket);
            }
            anyhow::bail!("Connection cancelled by user.");
        }
    };

    pb.finish_with_message("Done");

    Ok(ConnectPhase::Setup {
        session,
        ticket,
        is_pairing,
    })
}

// ── Phase: Setup ─────────────────────────────────────────────────────────────

async fn phase_setup(
    mut session: Session,
    ticket: irosh::transport::ticket::Ticket,
    _is_pairing: bool,
    sm: &mut ConnectCtx,
) -> Result<ConnectPhase> {
    // NON-INTERACTIVE EXEC MODE
    if let Some(cmd) = &sm.exec_cmd {
        let output = session.capture_exec(cmd).await?;
        std::io::stdout().write_all(&output.stdout)?;
        std::io::stderr().write_all(&output.stderr)?;
        if output.exit_status != 0 {
            #[allow(clippy::cast_possible_wrap)]
            std::process::exit(output.exit_status as i32);
        }
        return Ok(ConnectPhase::Close {
            reason: DisconnectReason::UserInitiated,
        });
    }

    let metadata = session.remote_metadata();
    let display_name = if let Some(meta) = metadata {
        meta.default_alias()
    } else {
        format!("peer-{}", &ticket.to_addr().id.to_string()[..8])
    };

    Ui::success(&format!("Secure session established with {display_name}"));

    // Auto-save peer
    sm.peer_alias = auto_save_peer(&sm.state, &ticket, &display_name);

    // PTY and shell
    let stdin_is_tty = std::io::stdin().is_terminal();
    let stdout_is_tty = std::io::stdout().is_terminal();
    let guard = if stdin_is_tty && stdout_is_tty {
        let guard = TerminalGuard::new()?;
        let size = current_terminal_size();
        let term = std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string());
        session.request_pty(PtyOptions::new(term, size)).await?;
        Some(guard)
    } else {
        None
    };

    session.start_shell().await?;

    if let Some(ref fwd) = sm.forward {
        tunnels::setup_forwarding(&mut session, Some(fwd.clone())).await?;
    }

    let remote_is_windows = session
        .remote_metadata()
        .is_some_and(|meta| meta.os.eq_ignore_ascii_case("windows"));

    let engine = input::InputEngine::new(&sm.state, remote_is_windows);

    Ok(ConnectPhase::Shell {
        session,
        engine,
        guard,
    })
}

// ── Phase: Shell ─────────────────────────────────────────────────────────────

async fn phase_shell(
    session: Session,
    engine: input::InputEngine,
    _guard: Option<TerminalGuard>,
    _sm: &ConnectCtx,
) -> ConnectPhase {
    let reason = match session::drive_session(session, engine).await {
        Ok(reason) => reason,
        Err(_) => DisconnectReason::Error,
    };
    ConnectPhase::Close { reason }
}

// ── Phase: Close ─────────────────────────────────────────────────────────────

fn phase_close(reason: &DisconnectReason, sm: &ConnectCtx) {
    let peer = sm.peer_alias.as_deref().unwrap_or("remote");
    match reason {
        DisconnectReason::UserInitiated => {
            Ui::info(&format!("Disconnected from {peer}."));
        }
        DisconnectReason::RemoteClosed => {
            Ui::info(&format!("{peer} closed the connection."));
        }
        DisconnectReason::ShellCrashed => {
            Ui::error(&format!("{peer} shell exited unexpectedly."), None);
        }
        DisconnectReason::Error => {
            Ui::error(
                &format!("Session with {peer} terminated due to an error."),
                None,
            );
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn auto_save_peer(
    state: &irosh::StateConfig,
    ticket: &irosh::transport::ticket::Ticket,
    display_name: &str,
) -> Option<String> {
    let peers = irosh::storage::list_peers(state).unwrap_or_default();
    if peers
        .iter()
        .any(|p| p.ticket.to_addr().id == ticket.to_addr().id)
    {
        return None;
    }

    let name_exists = peers.iter().any(|p| p.name == display_name);
    let final_name = if name_exists {
        let fallback = format!("{}-{}", display_name, &ticket.to_addr().id.to_string()[..4]);
        Ui::input(
            &format!("A peer named '{display_name}' already exists. Enter a new alias"),
            Some(&fallback),
        )
    } else {
        Some(display_name.to_string())
    };

    if let Some(ref name) = final_name {
        let profile = irosh::storage::PeerProfile {
            name: name.clone(),
            ticket: ticket.clone(),
        };
        if irosh::storage::save_peer(state, &profile).is_ok() {
            if name_exists {
                Ui::success(&format!("Peer alias updated to '{name}'"));
            } else {
                Ui::success(&format!(
                    "Peer auto-saved as '{name}'. Use 'irosh {name}' next time."
                ));
            }
        }
    }
    final_name
}

fn auto_save_temp_peer(state: &irosh::StateConfig, ticket: &irosh::transport::ticket::Ticket) {
    let name = format!("peer-{}", &ticket.to_addr().id.to_string()[..8]);
    let peers = irosh::storage::list_peers(state).unwrap_or_default();

    // Check if the exact peer node ID is already in the address book
    if !peers
        .iter()
        .any(|p| p.ticket.to_addr().id == ticket.to_addr().id)
    {
        let profile = irosh::storage::PeerProfile {
            name: name.clone(),
            ticket: ticket.clone(),
        };
        if irosh::storage::save_peer(state, &profile).is_ok() {
            Ui::info("");
            Ui::warn(
                "Connection failed, but peer ticket was recovered!",
                &format!("Saved as '{name}' for future retries."),
            );
            Ui::info(&format!(
                "You can reconnect by running: irosh connect {name}"
            ));
        }
    }
}

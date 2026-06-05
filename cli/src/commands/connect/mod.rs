use crate::display;
use crate::terminal::TerminalGuard;
use crate::ui::Ui;
use anyhow::Result;
use irosh::sys::current_terminal_size;
use irosh::{Client, ClientOptions, PtyOptions};
use std::io::{IsTerminal, Write};

mod completion;
mod editor;
mod history;
mod input;
mod prompt;
mod session;
mod transfer;
mod tunnels;

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

    // Apply global config overrides
    if let Some(secret) = secret_str.or(config.stealth_secret) {
        options = options.secret(secret);
    }

    if let Some(relay_url) = &config.relay_url {
        let mode = irosh::transport::iroh::parse_relay_mode(relay_url)?;
        options = options.relay_mode(mode);
    }

    // Resolve connection target
    let target = if let Some(t) = ticket_str {
        Ui::info("Connecting via explicit ticket...");
        irosh::ResolvedTarget::Ticket(t.parse()?)
    } else if let Some(c) = code_str {
        Ui::info(&format!("Connecting via explicit wormhole: {c}"));
        irosh::ResolvedTarget::WormholeCode(c)
    } else {
        let raw_target = if let Some(t) = target_str {
            t
        } else {
            let peers = irosh::storage::list_peers(&state)?;
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

                let selection = Ui::select("Select a peer to connect", &items);
                match selection {
                    Some(0) => match Ui::input("Enter a wormhole code or ticket", None) {
                        Some(val) if !val.trim().is_empty() => val.trim().to_string(),
                        _ => anyhow::bail!("No target specified."),
                    },
                    Some(idx) => peers[idx - 1].name.clone(),
                    None => anyhow::bail!("Connection cancelled."),
                }
            }
        };

        let resolved = Client::parse_target(options.state(), &raw_target)?;

        match &resolved {
            irosh::ResolvedTarget::Ticket(_) => {
                let is_alias = irosh::storage::list_peers(&state)?
                    .iter()
                    .any(|p| p.name == raw_target);

                if is_alias {
                    Ui::info(&format!("Connecting to saved peer: {raw_target}"));
                } else {
                    Ui::info("Connecting via direct ticket...");
                }
            }
            irosh::ResolvedTarget::WormholeCode(code) => {
                Ui::info(&format!("Attempting wormhole connection: {code}"));
            }
            _ => {}
        }
        resolved
    };

    let pb = Ui::spinner("Establishing connection...");
    options = options.password_prompter(CliPasswordPrompter {
        pb: Some(pb.clone()),
    });

    let (ticket, is_pairing) = tokio::select! {
        res = async {
            match target {
                irosh::ResolvedTarget::Ticket(t) => Ok((t, false)),
                irosh::ResolvedTarget::WormholeCode(code) => {
                    pb.set_message(format!("Searching for wormhole: {code}..."));
                    match Client::connect_wormhole(&options, &code).await {
                        Ok(t) => Ok((t, true)),
                        Err(e) => Err(e),
                    }
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
        res = Client::dial_p2p(&options, ticket.clone(), is_pairing) => {
            match res {
                Ok(c) => c,
                Err(e) => {
                    pb.finish_with_message("Failed");
                    if is_pairing {
                        auto_save_temp_peer(&state, &ticket);
                    }
                    return Err(e.into());
                }
            }
        },
        _ = tokio::signal::ctrl_c() => {
            pb.finish_with_message("Cancelled");
            if is_pairing {
                auto_save_temp_peer(&state, &ticket);
            }
            anyhow::bail!("Connection cancelled by user.");
        }
    };

    pb.set_message("Authenticating...");
    let mut session = tokio::select! {
        res = Client::establish_session(&options, connection_info) => {
            match res {
                Ok(s) => s,
                Err(e) => {
                    pb.finish_with_message("Failed");
                    if is_pairing {
                        auto_save_temp_peer(&state, &ticket);
                    }
                    return Err(e.into());
                }
            }
        },
        _ = tokio::signal::ctrl_c() => {
            pb.finish_with_message("Cancelled");
            if is_pairing {
                auto_save_temp_peer(&state, &ticket);
            }
            anyhow::bail!("Connection cancelled by user.");
        }
    };

    pb.finish_with_message("Done");

    // NON-INTERACTIVE EXEC MODE
    if let Some(cmd) = exec_cmd {
        let output = session.capture_exec(&cmd).await?;
        std::io::stdout().write_all(&output.stdout)?;
        std::io::stderr().write_all(&output.stderr)?;
        if output.exit_status != 0 {
            #[allow(clippy::cast_possible_wrap)]
            std::process::exit(output.exit_status as i32);
        }
        return Ok(());
    }

    let metadata = session.remote_metadata();
    let display_name = if let Some(meta) = metadata {
        meta.default_alias()
    } else {
        format!("peer-{}", &ticket.to_addr().id.to_string()[..8])
    };

    Ui::success(&format!("Secure session established with {display_name}"));

    // Silent Auto-save logic: Automatically save the peer if it's new and doesn't conflict.
    let peers = irosh::storage::list_peers(&state)?;
    let is_already_saved = peers
        .iter()
        .any(|p| p.ticket.to_addr().id == ticket.to_addr().id);

    if !is_already_saved {
        let name_exists = peers.iter().any(|p| p.name == display_name);

        let final_name = if name_exists {
            // CONFLICT: Name is taken, ask for a new one.
            let fallback = format!("{}-{}", display_name, &ticket.to_addr().id.to_string()[..4]);
            Ui::input(
                &format!("A peer named '{display_name}' already exists. Enter a new alias"),
                Some(&fallback),
            )
        } else {
            // NO CONFLICT: Silent save.
            Some(display_name.clone())
        };

        if let Some(target_name) = final_name {
            let profile = irosh::storage::PeerProfile {
                name: target_name.clone(),
                ticket: ticket.clone(),
            };
            if irosh::storage::save_peer(&state, &profile).is_ok() {
                if name_exists {
                    Ui::success(&format!("Peer alias updated to '{target_name}'"));
                } else {
                    Ui::success(&format!(
                        "Peer auto-saved as '{target_name}'. Use 'irosh {target_name}' next time."
                    ));
                }
            }
        }
    }

    // Setup terminal
    let stdin_is_tty = std::io::stdin().is_terminal();
    let stdout_is_tty = std::io::stdout().is_terminal();
    let mut _guard = None;

    if stdin_is_tty && stdout_is_tty {
        _guard = Some(TerminalGuard::new()?);
        let size = current_terminal_size();
        let term = std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string());
        session.request_pty(PtyOptions::new(term, size)).await?;
    }

    session.start_shell().await?;

    // Handle port forwarding
    tunnels::setup_forwarding(&mut session, forward_str).await?;

    let remote_is_windows = session
        .remote_metadata()
        .is_some_and(|meta| meta.os.eq_ignore_ascii_case("windows"));

    let input_engine = input::InputEngine::new(&state, remote_is_windows);

    session::drive_session(session, input_engine).await
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

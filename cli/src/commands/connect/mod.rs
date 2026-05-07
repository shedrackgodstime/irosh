use crate::display;
use crate::ui::Ui;
use anyhow::Result;
use irosh::sys::{RawTerminal, current_terminal_size};
use irosh::{Client, ClientOptions, PtyOptions};
use std::io::IsTerminal;

mod session;
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
pub async fn exec_shortcut(target: &str, ctx: &CliContext) -> Result<()> {
    exec_internal(Some(target.to_string()), None, ctx).await
}

/// Entry point for 'irosh connect <target>'
pub async fn exec(target: Option<String>, forward: Option<String>, ctx: &CliContext) -> Result<()> {
    exec_internal(target, forward, ctx).await
}

async fn exec_internal(
    target_str: Option<String>,
    forward_str: Option<String>,
    ctx: &CliContext,
) -> Result<()> {
    let state = ctx.state.clone();
    let config = irosh::storage::load_config(&state)?;

    let mut options = ClientOptions::new(state.clone());

    // Apply global config overrides
    if let Some(secret) = &config.stealth_secret {
        options = options.secret(secret);
    }

    if let Some(relay_url) = &config.relay_url {
        let mode = irosh::transport::iroh::parse_relay_mode(relay_url)?;
        options = options.relay_mode(mode);
    }

    // Resolve target if not provided
    let raw_target = match target_str {
        Some(t) => t,
        None => {
            let peers = irosh::storage::list_peers(&state)?;
            if peers.is_empty() {
                Ui::warn("Address book is empty", "You haven't saved any peers yet.");
                Ui::info("To connect, you can:");
                Ui::info("  1. Use a wormhole code:   \x1b[36mirosh <code-word>\x1b[0m");
                Ui::info("  2. Use a full ticket:     \x1b[36mirosh <ticket-string>\x1b[0m");
                Ui::info(
                    "  3. Add a peer manually:   \x1b[36mirosh peer add <name> <ticket>\x1b[0m",
                );
                println!();
                anyhow::bail!("No target specified.");
            }

            let items: Vec<String> = peers
                .iter()
                .map(|p| format!("[{}] {}", p.name, display::shorten_ticket(&p.ticket)))
                .collect();

            let selection = Ui::select("Select a peer to connect", &items);
            match selection {
                Some(idx) => peers[idx].name.clone(),
                None => anyhow::bail!("Connection cancelled."),
            }
        }
    };

    let target = Client::parse_target(options.state(), &raw_target)?;

    match &target {
        irosh::ResolvedTarget::Ticket(_) => {
            let is_alias = irosh::storage::list_peers(&state)?
                .iter()
                .any(|p| p.name == raw_target);

            if is_alias {
                Ui::info(&format!(
                    "Connecting to saved peer: \x1b[1;36m{}\x1b[0m",
                    raw_target
                ));
            } else {
                Ui::info("Connecting via direct ticket...");
            }
        }
        irosh::ResolvedTarget::WormholeCode(code) => {
            Ui::info(&format!(
                "Attempting wormhole connection: \x1b[1;33m{}\x1b[0m",
                code
            ));
        }
    }

    let pb = Ui::spinner("Establishing connection...");
    options = options.password_prompter(CliPasswordPrompter {
        pb: Some(pb.clone()),
    });

    let (ticket, is_pairing) = match target {
        irosh::ResolvedTarget::Ticket(t) => (t, false),
        irosh::ResolvedTarget::WormholeCode(code) => {
            pb.set_message(format!("Searching for wormhole: {}...", code));
            match Client::connect_wormhole(&options, &code).await {
                Ok(t) => (t, true),
                Err(e) => {
                    pb.finish_and_clear();
                    return Err(e.into());
                }
            }
        }
    };

    pb.set_message("Dialing P2P node...");
    let connection = match Client::dial_p2p(&options, ticket.clone(), is_pairing).await {
        Ok(c) => c,
        Err(e) => {
            pb.finish_and_clear();
            return Err(e.into());
        }
    };

    pb.set_message("Performing SSH handshake...");
    let mut session = match Client::establish_session(&options, connection).await {
        Ok(s) => s,
        Err(e) => {
            pb.finish_and_clear();
            return Err(e.into());
        }
    };

    pb.finish_and_clear();

    let metadata = session.remote_metadata();
    let display_name = if let Some(meta) = metadata {
        meta.default_alias()
    } else {
        format!("peer-{}", &ticket.to_addr().id.to_string()[..8])
    };

    Ui::success(&format!(
        "Secure session established with \x1b[1;36m{}\x1b[0m",
        display_name
    ));

    // Auto-save logic: Offer to save the peer if it's not already in the address book
    let peers = irosh::storage::list_peers(&state)?;
    let is_already_saved = peers
        .iter()
        .any(|p| p.ticket.to_addr().id == ticket.to_addr().id);

    if !is_already_saved
        && Ui::soft_confirm(&format!(
            "Save this peer to your address book as '{}'?",
            display_name
        ))
    {
        let profile = irosh::storage::PeerProfile {
            name: display_name.clone(),
            ticket: ticket.clone(),
        };
        if let Err(e) = irosh::storage::save_peer(&state, &profile) {
            Ui::error(&format!("Failed to save peer: {}", e));
        } else {
            Ui::success(&format!(
                "Peer saved! You can now connect using: \x1b[36mirosh connect {}\x1b[0m",
                display_name
            ));
        }
    }

    // Setup terminal
    let stdin_is_tty = std::io::stdin().is_terminal();
    let stdout_is_tty = std::io::stdout().is_terminal();

    let _raw_terminal = if stdin_is_tty && stdout_is_tty {
        Some(RawTerminal::new(0)?)
    } else {
        None
    };

    if stdin_is_tty && stdout_is_tty {
        let size = current_terminal_size();
        let term = std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string());
        session.request_pty(PtyOptions::new(term, size)).await?;
    }

    session.start_shell().await?;

    // Handle port forwarding
    tunnels::setup_forwarding(&mut session, forward_str).await?;

    session::drive_session(session).await
}

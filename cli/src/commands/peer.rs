use crate::commands::PeerAction;
use crate::context::CliContext;
use crate::display;
use crate::ui::Ui;
use anyhow::Result;
use irosh::storage;

pub async fn exec(action: PeerAction, ctx: &CliContext) -> Result<()> {
    let state = &ctx.state;

    match action {
        PeerAction::List => {
            let peers = storage::list_peers(state)?;
            if peers.is_empty() {
                Ui::info(
                    "Your address book is empty. Add peers with 'irosh peer add' or use a wormhole code.",
                );
                return Ok(());
            }

            println!("\n  \x1b[1;36m👥 Saved Peers (Address Book)\x1b[0m");
            println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m");
            println!(
                "  \x1b[1;37m{:<18} {:<30}\x1b[0m",
                "ALIAS", "TICKET SUMMARY"
            );

            for p in peers {
                println!(
                    "  \x1b[1m{:<18}\x1b[0m \x1b[38;5;244m{}\x1b[0m",
                    p.name,
                    display::shorten_ticket(&p.ticket)
                );
            }
            println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m\n");
        }
        PeerAction::Add { name, ticket } => {
            let ticket_parsed = ticket.parse()?;
            storage::save_peer(
                state,
                &storage::PeerProfile {
                    name: name.clone(),
                    ticket: ticket_parsed,
                },
            )?;
            Ui::success(&format!(
                "Peer '{}' has been added to your address book.",
                name
            ));
        }
        PeerAction::Remove { name } => {
            let target_name = match name {
                Some(n) => n,
                None => {
                    let peers = storage::list_peers(state)?;
                    if peers.is_empty() {
                        Ui::info("No peers to remove.");
                        return Ok(());
                    }
                    let items: Vec<String> = peers.iter().map(|p| p.name.clone()).collect();
                    match Ui::select("Select a peer to remove", &items) {
                        Some(idx) => peers[idx].name.clone(),
                        None => {
                            Ui::info("Cancelled.");
                            return Ok(());
                        }
                    }
                }
            };
            if Ui::soft_confirm(&format!("Remove peer '{}' from address book?", target_name)) {
                storage::delete_peer(state, &target_name)?;
                Ui::success(&format!("Peer '{}' removed.", target_name));
            }
        }
        PeerAction::Info { name } => {
            if let Some(p) = storage::get_peer(state, &name)? {
                println!("\n  \x1b[1;36mℹ️  Peer Detail: {}\x1b[0m", name);
                println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m");
                println!("  \x1b[1;37mAlias:\x1b[0m     {}", name);
                println!(
                    "  \x1b[1;37mNode ID:\x1b[0m   \x1b[32m{}\x1b[0m",
                    p.ticket.to_addr().id
                );
                println!(
                    "  \x1b[1;37mTicket:\x1b[0m    \x1b[38;5;244m{}\x1b[0m",
                    p.ticket
                );

                let addr = p.ticket.to_addr();
                if let Some(relay) = addr.relay_urls().next() {
                    println!("  \x1b[1;37mRelay:\x1b[0m     {}", relay);
                }
                println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m\n");
            } else {
                Ui::error(&format!("Peer '{}' not found in address book.", name));
            }
        }
    }
    Ok(())
}

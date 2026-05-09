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

            if ctx.args.json {
                #[derive(serde::Serialize)]
                struct PeerInfoJson {
                    name: String,
                    ticket: String,
                }
                #[derive(serde::Serialize)]
                struct PeerListResponse {
                    total: usize,
                    peers: Vec<PeerInfoJson>,
                }
                let response = PeerListResponse {
                    total: peers.len(),
                    peers: peers
                        .into_iter()
                        .map(|p| PeerInfoJson {
                            name: p.name,
                            ticket: p.ticket.to_string(),
                        })
                        .collect(),
                };
                crate::output::print_success(response);
                return Ok(());
            }

            if peers.is_empty() {
                Ui::info(
                    "Your address book is empty. Add peers with 'irosh peer add' or use a wormhole code.",
                );
                return Ok(());
            }

            println!("\n  Saved Peers (Address Book)");
            println!("  ----------------------------------------------------");
            println!("  {:<18} {:<30}", "ALIAS", "TICKET SUMMARY");

            for p in peers {
                println!("  {:<18} {}", p.name, display::shorten_ticket(&p.ticket));
            }
            println!("  ----------------------------------------------------\n");
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
                let addr = p.ticket.to_addr();
                let relay = addr.relay_urls().next().map(|r| r.to_string());

                if ctx.args.json {
                    #[derive(serde::Serialize)]
                    struct PeerDetailResponse {
                        name: String,
                        node_id: String,
                        ticket: String,
                        relay: Option<String>,
                    }
                    crate::output::print_success(PeerDetailResponse {
                        name: name.clone(),
                        node_id: addr.id.to_string(),
                        ticket: p.ticket.to_string(),
                        relay: relay.clone(),
                    });
                    return Ok(());
                }

                println!("\n  Peer Detail: {}", name);
                println!("  ----------------------------------------------------");
                println!("  Alias:     {}", name);
                println!("  Node ID:   {}", addr.id);
                println!("  Ticket:    {}", p.ticket);

                if let Some(r) = relay {
                    println!("  Relay:     {}", r);
                }
                println!("  ----------------------------------------------------\n");
            } else {
                if ctx.args.json {
                    crate::output::print_error(&format!("Peer '{}' not found", name), "not_found");
                    return Ok(());
                }
                Ui::error(&format!("Peer '{}' not found in address book.", name));
            }
        }
    }
    Ok(())
}

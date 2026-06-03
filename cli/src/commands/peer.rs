use crate::commands::PeerAction;
use crate::context::CliContext;
use crate::display;
use crate::ui::Ui;
use anyhow::Result;
use irosh::storage;

#[must_use]
pub fn exec(action: PeerAction, ctx: &CliContext) -> Result<()> {
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
            let target_ticket = if let Some(t) = ticket { t } else {
                if ctx.args.json {
                    crate::output::print_error(
                        "Missing required argument: ticket",
                        "missing_args",
                    );
                    return Ok(());
                }
                match Ui::input("Enter the peer connection ticket", None) {
                    Some(t) if !t.trim().is_empty() => t.trim().to_string(),
                    _ => {
                        Ui::info("Cancelled.");
                        return Ok(());
                    }
                }
            };

            // Validate ticket early
            let ticket_parsed = target_ticket
                .parse::<irosh::transport::ticket::Ticket>()
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Invalid ticket format: {e}. Make sure you copied the full ticket string."
                    )
                })?;

            let target_name = match name {
                Some(n) => n,
                None => {
                    if ctx.args.json {
                        // In JSON mode, if no name is provided, use the default without prompting
                        format!("peer-{}", &ticket_parsed.to_addr().id.to_string()[..8])
                    } else {
                        let default_name =
                            format!("peer-{}", &ticket_parsed.to_addr().id.to_string()[..8]);
                        match Ui::input("Enter a friendly alias for this peer", Some(&default_name))
                        {
                            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
                            _ => {
                                Ui::info("Cancelled.");
                                return Ok(());
                            }
                        }
                    }
                }
            };

            // Check for duplicate name
            if storage::load_peer(state, &target_name)?.is_some() {
                if ctx.args.json {
                    crate::output::print_error(
                        &format!("A peer named '{target_name}' already exists."),
                        "duplicate_name",
                    );
                    return Ok(());
                }
                Ui::error(
                    &format!("a peer named '{target_name}' already exists"),
                    Some(
                        "use a different alias, or remove the existing one with 'irosh peer remove'",
                    ),
                );
                return Ok(());
            }

            storage::save_peer(
                state,
                &storage::PeerProfile {
                    name: target_name.clone(),
                    ticket: ticket_parsed.clone(),
                },
            )?;

            if ctx.args.json {
                #[derive(serde::Serialize)]
                struct PeerAddResponse {
                    name: String,
                    endpoint_id: String,
                    ticket: String,
                }
                crate::output::print_success(PeerAddResponse {
                    name: target_name,
                    endpoint_id: ticket_parsed.to_addr().id.to_string(),
                    ticket: ticket_parsed.to_string(),
                });
                return Ok(());
            }

            Ui::success(&format!(
                "Peer '{target_name}' has been added to your address book."
            ));
        }
        PeerAction::Remove { name } => {
            let target_name = if let Some(n) = name { n } else {
                let peers = storage::list_peers(state)?;
                if peers.is_empty() {
                    Ui::info("No peers to remove.");
                    return Ok(());
                }
                let items: Vec<String> = peers.iter().map(|p| p.name.clone()).collect();
                if let Some(idx) = Ui::select("Select a peer to remove", &items) { peers[idx].name.clone() } else {
                    Ui::info("Cancelled.");
                    return Ok(());
                }
            };
            if ctx.args.json
                || Ui::soft_confirm(&format!("Remove peer '{target_name}' from address book?"))
            {
                storage::delete_peer(state, &target_name)?;

                if ctx.args.json {
                    #[derive(serde::Serialize)]
                    struct PeerRemoveResponse {
                        name: String,
                    }
                    crate::output::print_success(PeerRemoveResponse { name: target_name });
                    return Ok(());
                }

                Ui::success(&format!("Peer '{target_name}' removed."));
            }
        }
        PeerAction::Info { name } => {
            let target_name = if let Some(n) = name { n } else {
                let peers = storage::list_peers(state)?;
                if peers.is_empty() {
                    Ui::info("Your address book is empty.");
                    return Ok(());
                }
                let items: Vec<String> = peers.iter().map(|p| p.name.clone()).collect();
                if let Some(idx) = Ui::select("Select a peer to inspect", &items) { peers[idx].name.clone() } else {
                    Ui::info("Cancelled.");
                    return Ok(());
                }
            };

            if let Some(p) = storage::load_peer(state, &target_name)? {
                let addr = p.ticket.to_addr();
                let relay = addr.relay_urls().next().map(std::string::ToString::to_string);

                if ctx.args.json {
                    #[derive(serde::Serialize)]
                    struct PeerDetailResponse {
                        name: String,
                        endpoint_id: String,
                        ticket: String,
                        relay: Option<String>,
                    }
                    crate::output::print_success(PeerDetailResponse {
                        name: target_name.clone(),
                        endpoint_id: addr.id.to_string(),
                        ticket: p.ticket.to_string(),
                        relay: relay.clone(),
                    });
                    return Ok(());
                }

                println!("\n  Peer Detail: {target_name}");
                println!("  ----------------------------------------------------");
                println!("  Alias:       {target_name}");
                println!("  Endpoint ID: {}", addr.id);
                println!("  Ticket:    {}", p.ticket);

                if let Some(r) = relay {
                    println!("  Relay:     {r}");
                }
                println!("  ----------------------------------------------------\n");
            } else {
                if ctx.args.json {
                    crate::output::print_error(
                        &format!("Peer '{target_name}' not found"),
                        "not_found",
                    );
                    return Ok(());
                }
                Ui::error(
                    &format!("peer '{target_name}' not found in address book"),
                    Some("run 'irosh peer list' to see known peers"),
                );
            }
        }

        PeerAction::Rename { old_name, new_name } => {
            let target_old = if let Some(n) = old_name { n } else {
                let peers = storage::list_peers(state)?;
                if peers.is_empty() {
                    Ui::info("No peers to rename.");
                    return Ok(());
                }
                let items: Vec<String> = peers.iter().map(|p| p.name.clone()).collect();
                if let Some(idx) = Ui::select("Select a peer to rename", &items) { peers[idx].name.clone() } else {
                    Ui::info("Cancelled.");
                    return Ok(());
                }
            };

            let target_new = match new_name {
                Some(n) => n,
                None => match Ui::input(&format!("Enter new name for '{target_old}'"), None) {
                    Some(n) if !n.trim().is_empty() => n.trim().to_string(),
                    _ => {
                        Ui::info("Cancelled.");
                        return Ok(());
                    }
                },
            };

            // Validate target_new doesn't conflict with an existing peer (unless it's the same)
            if target_old != target_new && storage::load_peer(state, &target_new)?.is_some() {
                Ui::error(
                    &format!("a peer named '{target_new}' already exists"),
                    Some("remove the existing peer first with 'irosh peer remove'"),
                );
                return Ok(());
            }

            if storage::rename_peer(state, &target_old, &target_new)? {
                Ui::success(&format!(
                    "Peer renamed: '{target_old}' -> '{target_new}'"
                ));
                Ui::info(&format!("Connect with: irosh connect {target_new}"));
            } else {
                Ui::error(
                    &format!("peer '{target_old}' not found in address book"),
                    None,
                );
            }
        }
    }
    Ok(())
}

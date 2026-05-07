use crate::commands::TrustAction;
use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::russh::keys::ssh_key::HashAlg;
use irosh::storage;

pub async fn exec(action: TrustAction, ctx: &CliContext) -> Result<()> {
    let state = ctx.server_state()?;

    match action {
        TrustAction::List => {
            let keys = storage::load_all_authorized_clients(&state)?;
            if keys.is_empty() {
                Ui::info("Vault is empty. No devices are trusted yet.");
                return Ok(());
            }

            println!("\n  Authorized Devices (Vault)");
            println!("  ----------------------------------------------------");
            println!("  {:<20} {:<30}", "IDENTITY", "FINGERPRINT (SHA256)");

            for (id, k) in keys {
                let fingerprint = k.fingerprint(HashAlg::Sha256).to_string();
                let short_id = if id == fingerprint {
                    "Unknown Device".to_string()
                } else {
                    id.clone()
                };

                println!("  {:<20} {}", short_id, fingerprint);
            }
            println!("  ----------------------------------------------------\n");
        }
        TrustAction::Revoke { fingerprint: _ } => {
            let keys = storage::load_all_authorized_clients(&state)?;
            if keys.is_empty() {
                Ui::info("No devices to revoke.");
                return Ok(());
            }

            let items: Vec<String> = keys
                .iter()
                .map(|(id, k)| {
                    let fingerprint = k.fingerprint(HashAlg::Sha256).to_string();
                    if id == &fingerprint {
                        format!("Unknown [{}]", fingerprint)
                    } else {
                        format!("{} [{}]", id, fingerprint)
                    }
                })
                .collect();

            match Ui::select("Select a device to revoke", &items) {
                Some(idx) => {
                    let (id, _) = &keys[idx];
                    if Ui::danger_confirm(
                        &format!("Are you sure you want to revoke trust for '{}'?", id),
                        "yes",
                    ) {
                        storage::revoke_key(&state, id)?;
                        Ui::success(&format!(
                            "Identity '{}' has been removed from the vault.",
                            id
                        ));
                    }
                }
                None => Ui::info("Cancelled."),
            }
        }
        TrustAction::Reset => {
            Ui::warn(
                "SECURITY WARNING",
                "A reset will wipe ALL trusted devices and clear your Node Password.",
            );
            if Ui::danger_confirm("Type 'yes' to proceed with full vault reset", "yes") {
                storage::reset_vault(&state)?;
                Ui::success("Vault fully reset. Node is now in bootstrap mode.");
            }
        }
    }
    Ok(())
}

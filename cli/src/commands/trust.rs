use crate::commands::TrustAction;
use crate::context::CliContext;
use crate::output::Output;
use crate::ui::Ui;
use anyhow::Result;
use irosh::russh::keys::ssh_key::HashAlg;
use irosh::storage;

#[must_use]
// Reason: CLI dispatch pattern; value is moved into match.
#[allow(clippy::needless_pass_by_value)]
pub fn exec(action: TrustAction, ctx: &CliContext) -> Result<()> {
    let state = ctx.server_state()?;

    match action {
        TrustAction::List => {
            let keys = storage::load_all_authorized_clients(&state)?;

            if ctx.args.json {
                #[derive(serde::Serialize)]
                struct TrustedDeviceJson {
                    identity: String,
                    fingerprint: String,
                    unknown: bool,
                }
                #[derive(serde::Serialize)]
                struct TrustListResponse {
                    total: usize,
                    devices: Vec<TrustedDeviceJson>,
                }
                let response = TrustListResponse {
                    total: keys.len(),
                    devices: keys
                        .into_iter()
                        .map(|(id, k)| {
                            let fingerprint = k.fingerprint(HashAlg::Sha256).to_string();
                            TrustedDeviceJson {
                                unknown: id == fingerprint,
                                identity: id,
                                fingerprint,
                            }
                        })
                        .collect(),
                };
                crate::output::print_success(response);
                return Ok(());
            }

            if keys.is_empty() {
                Ui::info("Vault is empty. No devices are trusted yet.");
                return Ok(());
            }

            Output::section("Authorized Devices (Vault)");
            Output::line(&format!(
                "  {:<20} {:<30}",
                "IDENTITY", "FINGERPRINT (SHA256)"
            ));

            for (id, k) in keys {
                let fingerprint = k.fingerprint(HashAlg::Sha256).to_string();
                let short_id = if id == fingerprint {
                    "Unknown Device".to_string()
                } else {
                    id.clone()
                };

                Output::line(&format!("  {short_id:<20} {fingerprint}"));
            }
            Output::hr();
            Output::nl();
        }
        TrustAction::Revoke { fingerprint } => {
            let keys = storage::load_all_authorized_clients(&state)?;
            if keys.is_empty() {
                Ui::info("No devices to revoke.");
                return Ok(());
            }

            let target: Option<String> = match fingerprint.as_deref() {
                Some(query) => {
                    let matches: Vec<&String> = keys
                        .iter()
                        .filter(|(id, k)| {
                            let fp = k.fingerprint(HashAlg::Sha256).to_string();
                            id == query || fp == query || fp.starts_with(query)
                        })
                        .map(|(id, _)| id)
                        .collect();

                    match matches.as_slice() {
                        [] => {
                            Ui::error(
                                &format!("No trusted device matches '{query}'."),
                                Some("Run 'irosh trust list' to see authorized fingerprints."),
                            );
                            return Ok(());
                        }
                        [only] => Some((*only).clone()),
                        _ => {
                            Ui::error(
                                &format!("'{query}' matches multiple trusted devices."),
                                Some("Provide a longer fingerprint prefix."),
                            );
                            return Ok(());
                        }
                    }
                }
                None => {
                    let items: Vec<String> = keys
                        .iter()
                        .map(|(id, k)| {
                            let fp = k.fingerprint(HashAlg::Sha256).to_string();
                            if id == &fp {
                                format!("Unknown [{fp}]")
                            } else {
                                format!("{id} [{fp}]")
                            }
                        })
                        .collect();

                    Ui::select("Select a device to revoke", &items).map(|idx| keys[idx].0.clone())
                }
            };

            let Some(id) = target else {
                if ctx.args.json {
                    crate::output::print_error(
                        "No identity specified for revocation",
                        "missing_args",
                    );
                    return Ok(());
                }
                Ui::info("Cancelled.");
                return Ok(());
            };

            if ctx.args.json
                || Ui::danger_confirm(
                    &format!("Are you sure you want to revoke trust for '{id}'?"),
                    "yes",
                )
            {
                storage::revoke_key(&state, &id)?;

                if ctx.args.json {
                    #[derive(serde::Serialize)]
                    struct TrustRevokeResponse {
                        identity: String,
                    }
                    crate::output::print_success(TrustRevokeResponse { identity: id });
                    return Ok(());
                }

                Ui::success(&format!("Identity '{id}' has been removed from the vault."));
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

use crate::commands::IdentityAction;
use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::russh::keys::ssh_key::HashAlg;
use irosh::storage;

pub async fn exec(action: IdentityAction, ctx: &CliContext) -> Result<()> {
    let state = ctx.server_state()?;

    match action {
        IdentityAction::Show => {
            let options = ctx.server_options()?;
            let ready = irosh::Server::inspect(&options).await?;

            let identity = storage::load_or_generate_identity(options.state()).await?;
            let fingerprint = identity.ssh_key.public_key().fingerprint(HashAlg::Sha256);

            if ctx.args.json {
                #[derive(serde::Serialize)]
                struct IdentityShowResponse {
                    node_id: String,
                    fingerprint: String,
                    ticket: String,
                    kind: &'static str,
                }

                crate::output::print_success(IdentityShowResponse {
                    node_id: ready.endpoint_id().to_string(),
                    fingerprint: fingerprint.to_string(),
                    ticket: ready.ticket().to_string(),
                    kind: "local",
                });
                return Ok(());
            }

            Ui::machine_identity(
                ready.endpoint_id(),
                &fingerprint.to_string(),
                &ready.ticket().to_string(),
                "Local",
            );
        }
        IdentityAction::Rotate => {
            Ui::warn(
                "IDENTITY ROTATION",
                "This will permanently delete your current cryptographic keys.",
            );
            Ui::info("      - Your Node ID and Ticket will change immediately.");
            Ui::info("      - All trusted relationships with other servers will be broken.");
            Ui::info("      - You will need to re-pair with all existing devices.");

            if Ui::danger_confirm("Type ROTATE to confirm this destructive action", "ROTATE") {
                let identity = storage::rotate_identity(&state).await?;
                let node_id = identity.node_id();

                Ui::success("New identity generated and saved.");
                Ui::info(&format!("New Node ID: {}", node_id));
                Ui::info("Don't forget to update your saved tickets on other machines.");
            }
        }
    }
    Ok(())
}

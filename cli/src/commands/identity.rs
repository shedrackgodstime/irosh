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

            println!("\n  \x1b[1;36m🆔 Machine Identity\x1b[0m");
            println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m");
            println!(
                "  \x1b[1;37mNode ID:\x1b[0m     \x1b[36m{}\x1b[0m",
                ready.endpoint_id()
            );
            println!(
                "  \x1b[1;37mFingerprint:\x1b[0m \x1b[32m{}\x1b[0m",
                fingerprint
            );
            println!(
                "  \x1b[1;37mTicket:\x1b[0m      \x1b[33m{}\x1b[0m",
                ready.ticket()
            );
            println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m\n");
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
                Ui::info(&format!("New Node ID: \x1b[1;36m{}\x1b[0m", node_id));
                Ui::info("Don't forget to update your saved tickets on other machines.");
            }
        }
    }
    Ok(())
}

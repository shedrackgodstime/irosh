use crate::commands::PasswdAction;
use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::storage;

pub async fn exec(action: PasswdAction, ctx: &CliContext) -> Result<()> {
    let state = ctx.server_state()?;

    match action {
        PasswdAction::Set => {
            if let Some(pw) = Ui::password_set() {
                let hash = irosh::auth::hash_password(&pw)?;
                storage::write_shadow_file(&state, &hash)?;
                Ui::success("Node Password has been updated and hashed securely.");
                Ui::info(
                    "New unknown devices will now be required to enter this password to pair.",
                );
            }
        }
        PasswdAction::Remove => {
            Ui::warn(
                "SECURITY NOTICE",
                "Removing the password will re-enable TOFU (Trust on First Use) if your vault is empty.",
            );
            if Ui::danger_confirm("Are you sure you want to remove the Node Password?", "yes") {
                storage::delete_shadow_file(&state)?;
                Ui::success("Node Password has been removed.");
            }
        }
        PasswdAction::Status => {
            println!("\n  Node Password Status");
            println!("  ----------------------------------------------------");
            if storage::load_shadow_file(&state)?.is_some() {
                println!("  Status:    ACTIVE");
                println!("  Security:  Argon2id Hashed");
            } else {
                println!("  Status:    NOT SET");
                println!("  Warning:   Node is currently in TOFU or Invite-only mode.");
            }
            println!("  ----------------------------------------------------\n");
        }
    }
    Ok(())
}

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
            let is_set = storage::load_shadow_file(&state)?.is_some();

            if ctx.args.json {
                #[derive(serde::Serialize)]
                struct PasswdStatusResponse {
                    is_set: bool,
                    security: Option<&'static str>,
                }

                crate::output::print_success(PasswdStatusResponse {
                    is_set,
                    security: if is_set { Some("argon2id") } else { None },
                });
                return Ok(());
            }

            Ui::header("Node Password Status");
            if is_set {
                Ui::status("Status", "ACTIVE", Some("Argon2id Hashed"));
            } else {
                Ui::status("Status", "NOT SET", None);
                Ui::warn(
                    "Security Notice",
                    "Node is currently in TOFU or Invite-only mode.",
                );
            }
            println!();
        }
    }
    Ok(())
}

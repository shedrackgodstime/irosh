use crate::commands::PasswdAction;
use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;
use irosh::storage;

#[must_use]
// Reason: CLI dispatch pattern; value is moved into match.
#[allow(clippy::needless_pass_by_value)]
pub fn exec(action: PasswdAction, ctx: &CliContext) -> Result<()> {
    let state = ctx.server_state()?;

    match action {
        PasswdAction::Set => {
            if let Some(pw) = Ui::password_set() {
                let hash = irosh::auth::hash_password(&pw)?;
                storage::write_shadow_file(&state, &hash)?;

                if ctx.args.json {
                    #[derive(serde::Serialize)]
                    struct PasswdSetResponse {
                        success: bool,
                        status: &'static str,
                    }
                    crate::output::print_success(PasswdSetResponse {
                        success: true,
                        status: "password_updated",
                    });
                    return Ok(());
                }

                Ui::success("Node Password has been updated and hashed securely.");
                Ui::info(
                    "New unknown devices will now be required to enter this password to pair.",
                );
            } else if ctx.args.json {
                crate::output::print_error("No password provided.", "missing_input");
                return Ok(());
            }
        }
        PasswdAction::Remove => {
            if !ctx.args.json {
                Ui::warn(
                    "SECURITY NOTICE",
                    "Removing the password will re-enable TOFU (Trust on First Use) if your vault is empty.",
                );
            }
            if ctx.args.json
                || Ui::danger_confirm("Are you sure you want to remove the Node Password?", "yes")
            {
                storage::delete_shadow_file(&state)?;

                if ctx.args.json {
                    #[derive(serde::Serialize)]
                    struct PasswdRemoveResponse {
                        success: bool,
                    }
                    crate::output::print_success(PasswdRemoveResponse { success: true });
                    return Ok(());
                }

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
            Ui::blank();
        }
    }
    Ok(())
}

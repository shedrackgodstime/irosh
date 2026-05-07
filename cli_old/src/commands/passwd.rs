use crate::Args as GlobalArgs;
use anyhow::{Context, Result};
use clap::Args;
use dialoguer::Password as PasswordPrompt;
use irosh::{
    StateConfig,
    auth::hash_password,
    storage::{delete_shadow_file, write_shadow_file},
};

#[derive(Args, Debug, Clone)]
pub struct PasswdArgs {
    /// Remove the existing password authentication.
    #[arg(long)]
    pub remove: bool,
}

pub async fn exec(passwd_args: PasswdArgs, global_args: &GlobalArgs) -> Result<()> {
    // 1. Resolve state directory.
    let state_root = global_args
        .state
        .clone()
        .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("server")))
        .context("could not determine state directory; please provide --state")?;

    let state = StateConfig::new(state_root);

    if passwd_args.remove {
        if delete_shadow_file(&state)? {
            println!("✅ Password authentication removed successfully.");
        } else {
            println!("ℹ️ No password authentication was configured.");
        }
        return Ok(());
    }

    // 2. Prompt for new password.
    let password = PasswordPrompt::new()
        .with_prompt("New password")
        .with_confirmation("Retype new password", "Passwords do not match")
        .interact()
        .context("failed to read password from terminal")?;

    if password.is_empty() {
        anyhow::bail!("Password cannot be empty.");
    }

    // 3. Hash and save.
    let hash = hash_password(&password).context("failed to hash password")?;
    write_shadow_file(&state, &hash).context("failed to save shadow file")?;

    println!("✅ Password authentication configured successfully.");
    println!(
        "ℹ️ The password hash is stored securely in: {}",
        state.root().join("shadow").display()
    );

    Ok(())
}

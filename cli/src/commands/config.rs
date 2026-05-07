use crate::commands::ConfigAction;
use crate::context::CliContext;
use crate::ui::Ui;
use anyhow::Result;

use irosh::storage;

pub async fn exec(action: ConfigAction, ctx: &CliContext) -> Result<()> {
    let state = &ctx.state;
    let mut config = storage::load_config(state)?;

    match action {
        ConfigAction::List => {
            println!("\n  \x1b[1;35m⚙️  Global Configuration\x1b[0m");
            println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m");
            println!("  \x1b[1;37m{:<18} {:<30}\x1b[0m", "SETTING", "VALUE");

            let settings = [
                (
                    "stealth-secret",
                    config.stealth_secret.as_deref().unwrap_or("<none>"),
                ),
                (
                    "relay-url",
                    config.relay_url.as_deref().unwrap_or("<iroh-default>"),
                ),
                ("log-level", &config.log_level),
                ("wormhole-timeout", &format!("{}s", config.wormhole_timeout)),
                (
                    "default-user",
                    config.default_user.as_deref().unwrap_or("<system-user>"),
                ),
            ];

            for (k, v) in settings {
                println!("  {:<18} \x1b[36m{}\x1b[0m", k, v);
            }
            println!("  \x1b[2m────────────────────────────────────────────────────\x1b[0m\n");
        }
        ConfigAction::Get { key } => {
            let val = match key.as_str() {
                "stealth-secret" => config
                    .stealth_secret
                    .as_deref()
                    .unwrap_or("<none>")
                    .to_string(),
                "relay-url" => config
                    .relay_url
                    .as_deref()
                    .unwrap_or("<iroh-default>")
                    .to_string(),
                "log-level" => config.log_level,
                "wormhole-timeout" => format!("{}s", config.wormhole_timeout),
                "default-user" => config
                    .default_user
                    .as_deref()
                    .unwrap_or("<system-user>")
                    .to_string(),
                _ => {
                    Ui::error(&format!("Unknown configuration key: {}", key));
                    anyhow::bail!("Invalid key.");
                }
            };
            Ui::info(&format!("\x1b[1m{}\x1b[0m = \x1b[36m{}\x1b[0m", key, val));
        }
        ConfigAction::Set { key, value } => {
            match key.as_str() {
                "stealth-secret" => {
                    config.stealth_secret = if value.is_empty() || value == "none" {
                        None
                    } else {
                        Some(value)
                    }
                }
                "relay-url" => {
                    config.relay_url = if value.is_empty() || value == "default" {
                        None
                    } else {
                        Some(value)
                    }
                }
                "log-level" => config.log_level = value,
                "wormhole-timeout" => {
                    config.wormhole_timeout = value
                        .parse()
                        .map_err(|_| anyhow::anyhow!("Timeout must be a number (seconds)"))?
                }
                "default-user" => {
                    config.default_user = if value.is_empty() { None } else { Some(value) }
                }
                _ => {
                    Ui::error(&format!("Unknown configuration key: {}", key));
                    anyhow::bail!("Invalid key.");
                }
            }
            storage::save_config(state, &config)?;
            Ui::success(&format!("Configuration updated: '{}' has been saved.", key));
        }
        ConfigAction::Export { .. } => {
            Ui::info("Export not yet implemented.");
        }
        ConfigAction::Import { .. } => {
            Ui::info("Import not yet implemented.");
        }
    }
    Ok(())
}

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
            if ctx.args.json {
                #[derive(serde::Serialize)]
                struct ConfigListJson {
                    stealth_secret: Option<String>,
                    relay_url: Option<String>,
                    log_level: String,
                    wormhole_timeout: u64,
                    default_user: Option<String>,
                }
                crate::output::print_success(ConfigListJson {
                    stealth_secret: config.stealth_secret.clone(),
                    relay_url: config.relay_url.clone(),
                    log_level: config.log_level.clone(),
                    wormhole_timeout: config.wormhole_timeout,
                    default_user: config.default_user.clone(),
                });
                return Ok(());
            }

            println!("\n  Global Configuration");
            println!("  ----------------------------------------------------");
            println!("  {:<18} {:<30}", "SETTING", "VALUE");

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
                println!("  {:<18} {}", k, v);
            }
            println!("  ----------------------------------------------------\n");
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
                "log-level" => config.log_level.clone(),
                "wormhole-timeout" => format!("{}s", config.wormhole_timeout),
                "default-user" => config
                    .default_user
                    .as_deref()
                    .unwrap_or("<system-user>")
                    .to_string(),
                _ => {
                    if ctx.args.json {
                        crate::output::print_error(&format!("Unknown key: {}", key), "invalid_key");
                        return Ok(());
                    }
                    Ui::error(
                        &format!("unknown configuration key: {}", key),
                        Some("run 'irosh config list' to see all valid keys"),
                    );
                    anyhow::bail!("Invalid key.");
                }
            };

            if ctx.args.json {
                #[derive(serde::Serialize)]
                struct ConfigGetJson {
                    key: String,
                    value: String,
                }
                crate::output::print_success(ConfigGetJson {
                    key: key.clone(),
                    value: val,
                });
                return Ok(());
            }

            Ui::info(&format!("{} = {}", key, val));
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
                    Ui::error(
                        &format!("unknown configuration key: {}", key),
                        Some("run 'irosh config list' to see all valid keys"),
                    );
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

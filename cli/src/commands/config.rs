use std::str::FromStr;

use crate::commands::ConfigAction;
use crate::context::CliContext;
use crate::output::Output;
use crate::ui::Ui;
use crate::ui::messages;
use anyhow::Result;

use irosh::config::LogLevel;
use irosh::storage;

#[must_use]
pub fn exec(action: ConfigAction, ctx: &CliContext) -> Result<()> {
    let state = &ctx.state;
    let mut config = storage::load_config(state)?;

    match action {
        ConfigAction::List => {
            if ctx.args.json {
                #[derive(serde::Serialize)]
                struct ConfigListJson {
                    stealth_secret: Option<String>,
                    relay_url: Option<String>,
                    log_level: irosh::LogLevel,
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

            Output::section("Global Configuration");
            Output::line(&format!("  {:<18} {:<30}", "SETTING", "VALUE"));

            let settings = [
                (
                    "stealth-secret",
                    config.stealth_secret.as_deref().unwrap_or("<none>"),
                ),
                (
                    "relay-url",
                    config.relay_url.as_deref().unwrap_or("<iroh-default>"),
                ),
                ("log-level", config.log_level.as_str()),
                ("wormhole-timeout", &format!("{}s", config.wormhole_timeout)),
                (
                    "default-user",
                    config.default_user.as_deref().unwrap_or("<system-user>"),
                ),
            ];

            for (k, v) in settings {
                Output::line(&format!("  {k:<18} {v}"));
            }
            Output::hr();
            Output::nl();
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
                "log-level" => config.log_level.as_str().to_string(),
                "wormhole-timeout" => format!("{}s", config.wormhole_timeout),
                "default-user" => config
                    .default_user
                    .as_deref()
                    .unwrap_or("<system-user>")
                    .to_string(),
                _ => {
                    if ctx.args.json {
                        crate::output::print_error(&format!("Unknown key: {key}"), "invalid_key");
                        return Ok(());
                    }
                    Ui::error(
                        &format!("unknown configuration key: {key}"),
                        Some(messages::TIP_CONFIG_LIST),
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

            Ui::info(&format!("{key} = {val}"));
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
                "log-level" => {
                    config.log_level = LogLevel::from_str(&value).map_err(|_| {
                        anyhow::anyhow!("Invalid log level (expected: debug, info, warn, error)")
                    })?
                }
                "wormhole-timeout" => {
                    config.wormhole_timeout = value
                        .parse()
                        .map_err(|_| anyhow::anyhow!("Timeout must be a number (seconds)"))?;
                }
                "default-user" => {
                    config.default_user = if value.is_empty() { None } else { Some(value) }
                }
                _ => {
                    Ui::error(
                        &format!("unknown configuration key: {key}"),
                        Some(messages::TIP_CONFIG_LIST),
                    );
                    anyhow::bail!("Invalid key.");
                }
            }
            storage::save_config(state, &config)?;
            Ui::success(&format!("Configuration updated: '{key}' has been saved."));
        }
        ConfigAction::Export { output } => {
            if let Some(path) = output {
                storage::export_config(state, &path)?;
                if ctx.args.json {
                    #[derive(serde::Serialize)]
                    struct ExportJson {
                        path: String,
                    }
                    crate::output::print_success(ExportJson {
                        path: path.display().to_string(),
                    });
                } else {
                    Ui::success(&format!("Configuration exported to: {}", path.display()));
                }
            } else {
                let config = storage::load_config(state)?;
                let json = serde_json::to_string_pretty(&config)?;
                Output::line(&json);
            }
        }
        ConfigAction::Import { file } => {
            storage::import_config(state, &file)?;
            if ctx.args.json {
                #[derive(serde::Serialize)]
                struct ImportJson {
                    file: String,
                }
                crate::output::print_success(ImportJson {
                    file: file.display().to_string(),
                });
            } else {
                Ui::success(&format!("Configuration imported from: {}", file.display()));
            }
        }
    }
    Ok(())
}

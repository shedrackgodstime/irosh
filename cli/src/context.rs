use crate::Args;
use anyhow::{Context, Result};
use irosh::StateConfig;
use std::path::PathBuf;

/// Shared context for all CLI commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliContext {
    pub args: Args,
    pub state: StateConfig,
}

/// Returns true when `dirs::home_dir()` returned the SYSTEM profile on Windows.
fn is_system_profile(home: &std::path::Path) -> bool {
    home.to_string_lossy()
        .to_lowercase()
        .contains("system32\\config\\systemprofile")
}

/// Finds the best state directory, handling Windows SYSTEM context.
fn resolve_state_dir(subdir: &str) -> Option<PathBuf> {
    if let Some(state) = std::env::var_os("IROSH_STATE") {
        return Some(PathBuf::from(state));
    }
    let home = dirs::home_dir()?;
    if !is_system_profile(&home) {
        return Some(home.join(".irosh").join(subdir));
    }
    // Running as SYSTEM on Windows — try common user profile paths.
    #[cfg(windows)]
    {
        let base = PathBuf::from("C:\\Users");
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                let path = entry
                    .path()
                    .join("AppData")
                    .join("Local")
                    .join("irosh")
                    .join(subdir);
                if path.join("ipc.port").exists()
                    || path.join("config").exists()
                    || path.join("keys").exists()
                {
                    return Some(path);
                }
            }
        }
    }
    Some(PathBuf::from("C:\\Users\\Default\\AppData\\Local\\irosh").join(subdir))
}

impl CliContext {
    #[must_use]
    pub fn new(args: Args) -> Result<Self> {
        let state_root = args
            .state
            .clone()
            .or_else(|| resolve_state_dir("client"))
            .context("could not determine state directory")?;

        let state = StateConfig::new(state_root.clone());

        Ok(Self { args, state })
    }

    /// Returns the server-specific state directory (default fallback).
    #[must_use]
    pub fn server_state_root(&self) -> Result<PathBuf> {
        self.args
            .state
            .clone()
            .or_else(|| resolve_state_dir("server"))
            .context("could not determine server state directory")
    }

    #[must_use]
    pub fn server_state(&self) -> Result<StateConfig> {
        Ok(StateConfig::new(self.server_state_root()?))
    }

    #[must_use]
    pub fn server_options(&self) -> Result<irosh::ServerOptions> {
        let state = self.server_state()?;
        let config = irosh::storage::load_config(&state)?;
        let relay_str = config
            .relay_url
            .clone()
            .unwrap_or_else(|| "default".to_string());

        let mut options = irosh::ServerOptions::new(state)
            .relay_mode(
                irosh::transport::iroh::parse_relay_mode(&relay_str)?,
                Some(relay_str),
            )
            .security(irosh::SecurityConfig {
                host_key_policy: irosh::config::HostKeyPolicy::Tofu,
            });

        if let Some(secret) = config.stealth_secret {
            options = options.secret(secret);
        }

        Ok(options)
    }
}

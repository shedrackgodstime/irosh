use crate::Args;
use anyhow::{Context, Result};
use irosh::{IpcClient, StateConfig};
use std::path::PathBuf;

/// Shared context for all CLI commands.
pub struct CliContext {
    pub args: Args,
    pub state: StateConfig,
    #[allow(dead_code)]
    pub ipc: IpcClient,
}

impl CliContext {
    pub fn new(args: Args) -> Result<Self> {
        let state_root = args
            .state
            .clone()
            .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("client")))
            .context("could not determine state directory")?;

        let state = StateConfig::new(state_root.clone());
        let ipc = IpcClient::new(state_root);

        Ok(Self { args, state, ipc })
    }

    /// Returns the server-specific state directory (default fallback).
    pub fn server_state_root(&self) -> Result<PathBuf> {
        self.args
            .state
            .clone()
            .or_else(|| dirs::home_dir().map(|h| h.join(".irosh").join("server")))
            .context("could not determine server state directory")
    }

    pub fn server_state(&self) -> Result<StateConfig> {
        Ok(StateConfig::new(self.server_state_root()?))
    }

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

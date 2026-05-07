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
}

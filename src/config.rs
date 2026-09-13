//! Configuration data structures for the library.
//!
//! These types define the runtime configuration, state storage paths,
//! and security parameters used across the irosh subsystem.

use std::fmt;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Newtype for Iroh endpoint/node identifiers.
///
/// Ensures `endpoint_id` cannot be confused with `peer_id` or wormhole codes.
#[cfg_attr(feature = "storage", derive(serde::Serialize, serde::Deserialize))]
#[serde(transparent)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EndpointId(pub String);

impl EndpointId {
    /// Creates a new `EndpointId` from a string.
    #[must_use]
    pub fn new(id: String) -> Self {
        Self(id)
    }

    /// Returns the inner string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for EndpointId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for EndpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for EndpointId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for EndpointId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// Newtype for remote peer identifiers.
///
/// Ensures `peer_id` cannot be confused with `endpoint_id` or wormhole codes.
#[cfg_attr(feature = "storage", derive(serde::Serialize, serde::Deserialize))]
#[serde(transparent)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerId(pub String);

impl PeerId {
    /// Creates a new `PeerId` from a string.
    #[must_use]
    pub fn new(id: String) -> Self {
        Self(id)
    }

    /// Returns the inner string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for PeerId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for PeerId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for PeerId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// Newtype for wormhole pairing codes (rendezvous topics).
///
/// Codes may be arbitrary phrases; validated non-empty on construction.
#[cfg_attr(feature = "storage", derive(serde::Serialize, serde::Deserialize))]
#[serde(transparent)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WormholeCode(String);

impl WormholeCode {
    /// Creates a new `WormholeCode` from a string.
    ///
    /// # Errors
    ///
    /// Returns an error if the code is empty or whitespace-only.
    pub fn new(code: String) -> Result<Self, String> {
        if code.trim().is_empty() {
            return Err("wormhole code cannot be empty".to_string());
        }
        Ok(Self(code))
    }

    /// Returns the inner string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for WormholeCode {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for WormholeCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for WormholeCode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.to_string())
    }
}

/// Logging verbosity level.
///
/// Parsed case-insensitively; defaults to `Info`.
#[cfg_attr(feature = "storage", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Finest-grained diagnostic output.
    Trace,
    /// Detailed debug output.
    Debug,
    /// General operational information.
    #[default]
    Info,
    /// Warnings that do not interrupt operation.
    Warn,
    /// Errors that may interrupt operation.
    Error,
}

impl LogLevel {
    /// Returns the string representation of this log level.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Trace => f.write_str("trace"),
            Self::Debug => f.write_str("debug"),
            Self::Info => f.write_str("info"),
            Self::Warn => f.write_str("warn"),
            Self::Error => f.write_str("error"),
        }
    }
}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "trace" => Ok(Self::Trace),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warn" | "warning" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            other => Err(format!(
                "invalid log level: '{other}' (expected: trace, debug, info, warn, error)"
            )),
        }
    }
}

/// Defines where `irosh` stores persistent state.
///
/// This directory is used by the storage layer for identity material, trust
/// records, and saved peer aliases.
///
/// Under the `C-CALLER-CONTROL` principle, this type does **NOT** implement
/// `Default`. The consuming application must explicitly choose a state
/// directory path.
///
/// # Example
///
/// ```no_run
/// use irosh::StateConfig;
///
/// let state = StateConfig::new("/tmp/irosh-client".into());
/// assert!(state.root().ends_with("irosh-client"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateConfig {
    /// The root directory for state files.
    root: PathBuf,
}

impl StateConfig {
    /// Creates a new `StateConfig` anchored at the provided path.
    ///
    /// The directory does not need to exist prior to initialization;
    /// the relevant storage modules will create subdirectories as needed.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Returns the root directory used for state storage.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the directory where iroh blobs are stored.
    #[must_use]
    pub fn blobs_path(&self) -> PathBuf {
        self.root.join("blobs")
    }
}

/// Defines the policy for handling remote host keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HostKeyPolicy {
    /// Strictly verify host keys against the trust store. Connections to
    /// unknown or mismatched hosts will fail.
    Strict,
    /// Trust on first use. Unknown host keys will be automatically trusted and
    /// saved. Mismatched keys will still be rejected.
    Tofu,
    /// Automatically accept any host key without verification or saving.
    /// This is insecure and should only be used for testing.
    AcceptAll,
}

/// Defines security policies and connection semantics.
///
/// This type currently controls host-key validation behavior for SSH sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecurityConfig {
    /// The policy used to validate remote host keys.
    pub host_key_policy: HostKeyPolicy,
}

impl Default for SecurityConfig {
    /// Provides a safe default security posture.
    ///
    /// By default, TOFU (Trust On First Use) is enabled as it provides a good
    /// balance between security and usability for P2P.
    fn default() -> Self {
        Self {
            host_key_policy: HostKeyPolicy::Tofu,
        }
    }
}

impl SecurityConfig {
    /// Creates a new `SecurityConfig` with the given host key policy.
    #[must_use]
    pub fn new(host_key_policy: HostKeyPolicy) -> Self {
        Self { host_key_policy }
    }
}

/// Persistent application configuration stored on disk.
///
/// This includes global settings like stealth secrets, custom relays,
/// and default usernames.
#[cfg_attr(feature = "storage", derive(serde::Serialize, serde::Deserialize))]
#[serde(default)]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AppConfig {
    /// Shared ALPN secret for extra stealth.
    pub stealth_secret: Option<String>,
    /// Custom relay server URL.
    pub relay_url: Option<String>,
    /// Logging verbosity.
    pub log_level: LogLevel,
    /// Default wormhole expiry duration in seconds.
    pub wormhole_timeout: u64,
    /// Default username for connections.
    pub default_user: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            stealth_secret: None,
            relay_url: None,
            log_level: LogLevel::default(),
            wormhole_timeout: 3600, // 1 hour
            default_user: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_config_new_sets_root() {
        let state = StateConfig::new("/tmp/test-irosh".into());
        assert!(state.root().ends_with("test-irosh"));
    }

    #[test]
    fn state_config_blobs_path_appends_blobs() {
        let state = StateConfig::new("/tmp/test-irosh".into());
        assert!(state.blobs_path().ends_with("blobs"));
        assert_eq!(state.blobs_path(), state.root().join("blobs"));
    }

    #[test]
    fn state_config_equality() {
        let a = StateConfig::new("/tmp/a".into());
        let b = StateConfig::new("/tmp/a".into());
        assert_eq!(a, b);
    }

    #[test]
    fn state_config_clone_roundtrip() {
        let a = StateConfig::new("/tmp/clone-test".into());
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn host_key_policy_variants_distinct() {
        assert_ne!(HostKeyPolicy::Strict, HostKeyPolicy::Tofu);
        assert_ne!(HostKeyPolicy::Tofu, HostKeyPolicy::AcceptAll);
        assert_ne!(HostKeyPolicy::AcceptAll, HostKeyPolicy::Strict);
    }

    #[test]
    fn host_key_policy_copy_semantics() {
        let policy = HostKeyPolicy::Strict;
        let copied = policy;
        assert_eq!(policy, copied);
    }

    #[test]
    fn security_config_default_is_tofu() {
        let config = SecurityConfig::default();
        assert_eq!(config.host_key_policy, HostKeyPolicy::Tofu);
    }

    #[test]
    fn app_config_default_values() {
        let config = AppConfig::default();
        assert!(config.stealth_secret.is_none());
        assert!(config.relay_url.is_none());
        assert_eq!(config.log_level, LogLevel::Info);
        assert_eq!(config.wormhole_timeout, 3600);
        assert!(config.default_user.is_none());
    }

    #[test]
    fn app_config_equality() {
        let a = AppConfig::default();
        let b = AppConfig::default();
        assert_eq!(a, b);
    }
}

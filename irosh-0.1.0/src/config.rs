//! Configuration data structures for the library.
//!
//! These types define the runtime configuration, state storage paths,
//! and security parameters used across the irosh subsystem.

use std::path::{Path, PathBuf};

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
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Returns the root directory used for state storage.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Defines the policy for handling remote host keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

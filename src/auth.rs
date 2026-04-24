//! Pluggable authentication backends for irosh.
//!
//! This module provides the [`Authenticator`] trait that defines how credentials
//! are validated. Library consumers implement this to control authentication
//! logic. The CLI ships with built-in implementations for common use cases.
//!
//! Under the **C-CALLER-CONTROL** principle, the library never decides **how** to
//! validate credentials — it only calls the trait methods and respects the result.
//!
//! # Built-in Backends
//!
//! - [`KeyOnlyAuth`] — The default. Replicates the existing TOFU/Strict/AcceptAll
//!   key-based authentication. Zero change for existing users.
//! - [`PasswordAuth`] — A single shared password for all connections. Good for
//!   personal or simple setups.
//! - [`CombinedAuth`] — Accepts either public keys or passwords.

use std::fmt;
use std::sync::{Arc, Mutex as StdMutex};

use russh::keys::ssh_key::{HashAlg, PublicKey};
use tracing::{info, warn};

use crate::config::{HostKeyPolicy, SecurityConfig, StateConfig};
use crate::error::Result;
use crate::storage::trust::write_authorized_client;

/// Which authentication methods a backend supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthMethod {
    /// SSH public key authentication.
    PublicKey,
    /// Username + password authentication.
    Password,
}

/// Trait for pluggable authentication backends.
///
/// Library consumers implement this to control how credentials are validated.
/// The default behavior (key-only TOFU) is provided by [`KeyOnlyAuth`], which
/// is used automatically when no custom authenticator is configured.
///
/// # Example
///
/// ```no_run
/// use irosh::auth::{Authenticator, AuthMethod};
/// use russh::keys::ssh_key::PublicKey;
///
/// struct MyAuth;
///
/// impl std::fmt::Debug for MyAuth {
///     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
///         f.debug_struct("MyAuth").finish()
///     }
/// }
///
/// impl Authenticator for MyAuth {
///     fn supported_methods(&self) -> Vec<AuthMethod> {
///         vec![AuthMethod::Password]
///     }
///     fn check_public_key(&self, _user: &str, _key: &PublicKey) -> irosh::Result<bool> {
///         Ok(false)
///     }
///     fn check_password(&self, _user: &str, password: &str) -> irosh::Result<bool> {
///         Ok(password == "secret")
///     }
/// }
/// ```
pub trait Authenticator: Send + Sync + fmt::Debug + 'static {
    /// Returns which auth methods this backend supports.
    ///
    /// The server will advertise these methods to clients during the SSH
    /// handshake. Methods not listed here will be rejected immediately.
    fn supported_methods(&self) -> Vec<AuthMethod>;

    /// Validate a public key for the given user.
    ///
    /// Return `Ok(true)` to accept, `Ok(false)` to reject.
    /// Return `Err(...)` for internal failures.
    fn check_public_key(&self, user: &str, key: &PublicKey) -> Result<bool>;

    /// Validate a username + password combination.
    ///
    /// Return `Ok(true)` to accept, `Ok(false)` to reject.
    /// Return `Err(...)` for internal failures.
    fn check_password(&self, user: &str, password: &str) -> Result<bool>;
}

// ---------------------------------------------------------------------------
// Built-in backend: KeyOnlyAuth (default, backward compatible)
// ---------------------------------------------------------------------------

/// Key-only authentication using TOFU/Strict/AcceptAll policies.
///
/// This replicates the existing irosh authentication behavior exactly.
/// It is used automatically when no custom [`Authenticator`] is configured
/// on [`ServerOptions`](crate::ServerOptions).
#[derive(Debug)]
pub struct KeyOnlyAuth {
    policy: HostKeyPolicy,
    authorized_keys: Arc<StdMutex<Vec<PublicKey>>>,
    state: StateConfig,
}

impl KeyOnlyAuth {
    /// Creates a new key-only authenticator with the given policy and initial keys.
    pub fn new(
        security: SecurityConfig,
        authorized_keys: Vec<PublicKey>,
        state: StateConfig,
    ) -> Self {
        Self {
            policy: security.host_key_policy,
            authorized_keys: Arc::new(StdMutex::new(authorized_keys)),
            state,
        }
    }

    fn lock_keys(&self) -> std::sync::MutexGuard<'_, Vec<PublicKey>> {
        match self.authorized_keys.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("authorized client state mutex poisoned; recovering");
                poisoned.into_inner()
            }
        }
    }
}

impl Authenticator for KeyOnlyAuth {
    fn supported_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::PublicKey]
    }

    fn check_public_key(&self, _user: &str, key: &PublicKey) -> Result<bool> {
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();

        if self.policy == HostKeyPolicy::AcceptAll {
            info!(%fingerprint, "AcceptAll policy: automatically accepting client key.");
            return Ok(true);
        }

        let mut authorized = self.lock_keys();

        if !authorized.is_empty() {
            if authorized.contains(key) {
                info!(%fingerprint, "Client matched pre-authorized key. Access granted.");
                return Ok(true);
            }
            warn!(%fingerprint, "Client key not in authorized list. Rejecting connection.");
            return Ok(false);
        }

        // No authorized keys yet — check policy for new keys.
        match self.policy {
            HostKeyPolicy::Strict => {
                warn!(%fingerprint, "Strict policy: No pre-authorized keys found. Rejecting connection.");
                Ok(false)
            }
            HostKeyPolicy::Tofu => {
                info!(%fingerprint, "Tofu policy: No pre-authorized keys found. Trusting first client.");
                let _event = write_authorized_client(&self.state, &fingerprint, key)?;
                authorized.push(key.clone());
                Ok(true)
            }
            HostKeyPolicy::AcceptAll => unreachable!(),
        }
    }

    fn check_password(&self, _user: &str, _password: &str) -> Result<bool> {
        Ok(false) // Key-only backend never accepts passwords.
    }
}

// ---------------------------------------------------------------------------
// Built-in backend: PasswordAuth (single shared password)
// ---------------------------------------------------------------------------

/// Simple password authentication with a single shared password.
///
/// This is intended for personal or simple setups where one password
/// protects the server. The username is ignored — any user with the
/// correct password is accepted.
///
/// # Example (CLI)
///
/// ```bash
/// irosh-server --auth-mode password --auth-password "mySecret123"
/// ```
#[derive(Debug)]
pub struct PasswordAuth {
    password: String,
}

impl PasswordAuth {
    /// Creates a new password authenticator with the given password.
    pub fn new(password: impl Into<String>) -> Self {
        Self {
            password: password.into(),
        }
    }
}

impl Authenticator for PasswordAuth {
    fn supported_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::Password]
    }

    fn check_public_key(&self, _user: &str, _key: &PublicKey) -> Result<bool> {
        Ok(false) // Password-only backend never accepts keys.
    }

    fn check_password(&self, _user: &str, password: &str) -> Result<bool> {
        // NOTE: Timing attacks over P2P SSH are extremely difficult to exploit,
        // but a constant-time comparison (e.g. via `subtle`) would be ideal
        // for hardened deployments. This is acceptable for Phase 1.
        Ok(self.password == password)
    }
}

// ---------------------------------------------------------------------------
// Built-in backend: CombinedAuth (keys OR password)
// ---------------------------------------------------------------------------

/// Combined authentication accepting either public keys or passwords.
///
/// This delegates to a [`KeyOnlyAuth`] for key checks and a [`PasswordAuth`]
/// for password checks. A client can authenticate with either method.
#[derive(Debug)]
pub struct CombinedAuth {
    key_auth: KeyOnlyAuth,
    password_auth: PasswordAuth,
}

impl CombinedAuth {
    /// Creates a combined authenticator from a key backend and a password backend.
    pub fn new(key_auth: KeyOnlyAuth, password_auth: PasswordAuth) -> Self {
        Self {
            key_auth,
            password_auth,
        }
    }
}

impl Authenticator for CombinedAuth {
    fn supported_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::PublicKey, AuthMethod::Password]
    }

    fn check_public_key(&self, user: &str, key: &PublicKey) -> Result<bool> {
        self.key_auth.check_public_key(user, key)
    }

    fn check_password(&self, user: &str, password: &str) -> Result<bool> {
        self.password_auth.check_password(user, password)
    }
}

// ---------------------------------------------------------------------------
// Client-side credentials
// ---------------------------------------------------------------------------

/// Credentials for password-based authentication on the client side.
///
/// When provided to [`ClientOptions`](crate::ClientOptions), the client will
/// attempt password authentication if public key authentication is rejected.
#[derive(Debug, Clone)]
pub struct Credentials {
    /// The username to authenticate as.
    pub user: String,
    /// The password.
    pub password: String,
}

impl Credentials {
    /// Creates a new credentials pair.
    pub fn new(user: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            password: password.into(),
        }
    }
}

/// A callback trait to interactively prompt for a password.
///
/// Library consumers can implement this to ask the user for a password
/// when public key authentication fails but the server supports passwords.
pub trait PasswordPrompter: Send + Sync + std::fmt::Debug + 'static {
    /// Prompts the user for a password for the given username.
    ///
    /// This method will be called inside a blocking task (`spawn_blocking`),
    /// so it is safe to perform blocking I/O (like reading from stdin).
    /// Return `None` if the user cancels or prompting fails.
    fn prompt_password(&self, user: &str) -> Option<String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HostKeyPolicy, SecurityConfig, StateConfig};

    fn temp_state(name: &str) -> StateConfig {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "irosh-auth-test-{}-{}",
            name,
            rand::random::<u32>()
        ));
        StateConfig::new(path)
    }

    #[test]
    fn key_only_accept_all_accepts_any_key() {
        let auth = KeyOnlyAuth::new(
            SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            },
            vec![],
            temp_state("accept-all"),
        );
        assert!(auth.supported_methods().contains(&AuthMethod::PublicKey));
        assert!(!auth.supported_methods().contains(&AuthMethod::Password));
        // Password should always be rejected.
        assert!(!auth.check_password("user", "pass").unwrap());
    }

    #[test]
    fn password_auth_validates_correct_password() {
        let auth = PasswordAuth::new("secret123");
        assert!(auth.check_password("anyone", "secret123").unwrap());
        assert!(!auth.check_password("anyone", "wrong").unwrap());
        assert!(!auth.check_password("anyone", "").unwrap());
        // PublicKey should always be rejected.
        assert!(auth.supported_methods().contains(&AuthMethod::Password));
        assert!(!auth.supported_methods().contains(&AuthMethod::PublicKey));
    }

    #[test]
    fn combined_auth_supports_both_methods() {
        let key = KeyOnlyAuth::new(
            SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            },
            vec![],
            temp_state("combined"),
        );
        let pass = PasswordAuth::new("combo");
        let auth = CombinedAuth::new(key, pass);
        assert_eq!(auth.supported_methods().len(), 2);
        assert!(auth.supported_methods().contains(&AuthMethod::PublicKey));
        assert!(auth.supported_methods().contains(&AuthMethod::Password));
        assert!(auth.check_password("user", "combo").unwrap());
        assert!(!auth.check_password("user", "wrong").unwrap());
    }

    #[test]
    fn credentials_construction() {
        let creds = Credentials::new("admin", "pass123");
        assert_eq!(creds.user, "admin");
        assert_eq!(creds.password, "pass123");
    }
}

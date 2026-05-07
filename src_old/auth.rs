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
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use russh::keys::ssh_key::{HashAlg, PublicKey};
use tracing::{info, warn};

use crate::error::AuthError;

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
    password_hash: String,
}

impl PasswordAuth {
    /// Creates a new password authenticator with a pre-hashed password.
    pub fn new(password_hash: impl Into<String>) -> Self {
        Self {
            password_hash: password_hash.into(),
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
        let parsed_hash = PasswordHash::new(&self.password_hash)
            .map_err(|reason| AuthError::VerificationFailed { reason })?;

        match Argon2::default().verify_password(password.as_bytes(), &parsed_hash) {
            Ok(()) => Ok(true),
            Err(argon2::password_hash::Error::Password) => Ok(false),
            Err(reason) => Err(AuthError::VerificationFailed { reason }.into()),
        }
    }
}

/// Hashes a password using Argon2 with a random salt.
///
/// This uses Argon2id (the default in `argon2` crate) which is the current
/// industry standard for password hashing, providing resistance against
/// GPU cracking and side-channel attacks.
///
/// # Errors
///
/// Returns a [`StorageError::PasswordHash`] if salt generation or hashing fails.
pub fn hash_password(password: &str) -> Result<String> {
    let mut salt_bytes = [0u8; 16];

    // Securely fill the salt buffer using the OS RNG.
    // In rand 0.9, rand::fill is the idiomatic way to fill a buffer with OS-provided entropy.
    rand::fill(&mut salt_bytes);

    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|reason| crate::error::StorageError::PasswordHash { reason })?;

    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|reason| crate::error::StorageError::PasswordHash { reason })?
        .to_string();

    Ok(password_hash)
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

/// A callback trait to interactively confirm a pairing request.
pub trait ConfirmationCallback: Send + Sync + std::fmt::Debug + 'static {
    /// Confirms whether to accept a pairing request from a peer.
    ///
    /// This will be called on the server side when a client attempts to pair.
    fn confirm_pairing(&self, fingerprint: &str, key: &PublicKey) -> bool;
}

/// Authenticator used specifically for one-time wormhole pairing.
///
/// Tracks failed authentication attempts via a shared atomic counter,
/// enabling the server loop to enforce rate-limiting policies.
#[derive(Debug)]
pub struct PairingAuthenticator {
    state: StateConfig,
    expected_password: Option<String>,
    cached_key: Arc<StdMutex<Option<PublicKey>>>,
    confirmation_callback: Option<Arc<dyn ConfirmationCallback>>,
    failed_attempts: Arc<AtomicU32>,
    success_flag: Arc<std::sync::atomic::AtomicBool>,
}

impl PairingAuthenticator {
    /// Creates a new pairing authenticator.
    ///
    /// The `failed_attempts` counter is shared with the caller so the server
    /// loop can inspect it for rate-limiting decisions without reaching into
    /// authenticator internals.
    pub fn new(
        state: StateConfig,
        expected_password: Option<String>,
        confirmation_callback: Option<Arc<dyn ConfirmationCallback>>,
        failed_attempts: Arc<AtomicU32>,
        success_flag: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            state,
            expected_password,
            cached_key: Arc::new(StdMutex::new(None)),
            confirmation_callback,
            failed_attempts,
            success_flag,
        }
    }

    /// Returns the current number of failed authentication attempts.
    pub fn failed_attempts(&self) -> u32 {
        self.failed_attempts.load(Ordering::Relaxed)
    }
}

impl Authenticator for PairingAuthenticator {
    fn supported_methods(&self) -> Vec<AuthMethod> {
        if self.expected_password.is_some() {
            vec![AuthMethod::PublicKey, AuthMethod::Password]
        } else {
            vec![AuthMethod::PublicKey]
        }
    }

    fn check_public_key(&self, _user: &str, key: &PublicKey) -> Result<bool> {
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();

        if self.expected_password.is_none() {
            // Ephemeral wormhole (no password).
            // If we have a confirmation callback, ask the user.
            if let Some(callback) = &self.confirmation_callback {
                if !callback.confirm_pairing(&fingerprint, key) {
                    warn!(%fingerprint, "Wormhole pairing rejected by user.");
                    self.failed_attempts.fetch_add(1, Ordering::Relaxed);
                    return Ok(false);
                }
            }

            info!(%fingerprint, "Wormhole pairing: automatically accepting client key.");
            let _event = write_authorized_client(&self.state, &fingerprint, key)?;
            self.success_flag.store(true, Ordering::Relaxed);
            Ok(true)
        } else {
            // Persistent wormhole (requires password).
            // We still ask for confirmation first if a callback is present.
            if let Some(callback) = &self.confirmation_callback {
                if !callback.confirm_pairing(&fingerprint, key) {
                    warn!(%fingerprint, "Wormhole pairing rejected by user.");
                    self.failed_attempts.fetch_add(1, Ordering::Relaxed);
                    return Ok(false);
                }
            }

            // We cache the key so we can authorize it if password succeeds.
            if let Ok(mut cache) = self.cached_key.lock() {
                *cache = Some(key.clone());
            }
            Ok(false) // Reject public key, force fallback to password auth.
        }
    }

    fn check_password(&self, _user: &str, password: &str) -> Result<bool> {
        if let Some(expected) = &self.expected_password {
            if password == expected {
                // Correct password! Authorize the cached key.
                if let Ok(cache) = self.cached_key.lock() {
                    if let Some(key) = &*cache {
                        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
                        info!(%fingerprint, "Wormhole password accepted: authorizing client key.");
                        let _event = write_authorized_client(&self.state, &fingerprint, key)?;
                        self.success_flag.store(true, Ordering::Relaxed);
                        return Ok(true);
                    }
                }
            }
            // Wrong password — count as a failed attempt.
            self.failed_attempts.fetch_add(1, Ordering::Relaxed);
        }
        Ok(false)
    }
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
        let password = "secret123";
        let hash = hash_password(password).expect("failed to hash test password");
        let auth = PasswordAuth::new(hash);

        assert!(auth.check_password("anyone", password).unwrap());
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
        let password = "combo";
        let hash = hash_password(password).expect("failed to hash test password");
        let pass = PasswordAuth::new(hash);
        let auth = CombinedAuth::new(key, pass);

        assert_eq!(auth.supported_methods().len(), 2);
        assert!(auth.supported_methods().contains(&AuthMethod::PublicKey));
        assert!(auth.supported_methods().contains(&AuthMethod::Password));
        assert!(auth.check_password("user", password).unwrap());
        assert!(!auth.check_password("user", "wrong").unwrap());
    }

    #[test]
    fn credentials_construction() {
        let creds = Credentials::new("admin", "pass123");
        assert_eq!(creds.user, "admin");
        assert_eq!(creds.password, "pass123");
    }
}

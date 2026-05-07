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

/// Confirms whether to accept a pairing request from a peer.
pub trait ConfirmationCallback: Send + Sync + std::fmt::Debug + 'static {
    /// Confirms whether to accept a pairing request from a peer.
    fn confirm_pairing(&self, fingerprint: &str, key: &PublicKey) -> bool;
}

/// Tracking and notification handles for a pairing session.
#[derive(Debug, Clone)]
pub struct PairingMonitor {
    /// Flag set to true on successful pairing.
    pub success_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Counter for failed password attempts.
    pub failed_attempts: Arc<AtomicU32>,
    /// Notification channel for success.
    pub success_tx: Option<tokio::sync::mpsc::Sender<()>>,
}

/// The master authenticator for irosh, implementing the unified security policy.
///
/// This authenticator governs all connection attempts (direct or via wormhole)
/// and enforces a strict precedence:
/// 1. Established trust (Vault) always wins.
/// 2. Permanent Node Password challenges unknown keys.
/// 3. Active Wormhole Temp Password (Invite Pattern) provides a one-time override.
/// 4. Empty Vault + No Passwords allows TOFU.
#[derive(Debug)]
pub struct UnifiedAuthenticator {
    state: StateConfig,
    policy: HostKeyPolicy,
    authorized_keys: Arc<StdMutex<Vec<PublicKey>>>,
    temp_password_hash: Option<String>,
    success_flag: Arc<std::sync::atomic::AtomicBool>,
    failed_attempts: Arc<AtomicU32>,
    /// Tracks the key currently attempting password auth.
    cached_key: Arc<StdMutex<Option<PublicKey>>>,
    /// Optional notification channel for successful pairing.
    success_tx: Option<tokio::sync::mpsc::Sender<()>>,
}

impl UnifiedAuthenticator {
    /// Creates a new unified authenticator.
    pub fn new(
        state: StateConfig,
        policy: HostKeyPolicy,
        authorized_keys: Vec<PublicKey>,
        _temp_password_hash: Option<String>,
    ) -> Self {
        Self {
            state,
            policy,
            authorized_keys: Arc::new(StdMutex::new(authorized_keys)),
            temp_password_hash: _temp_password_hash,
            success_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            failed_attempts: Arc::new(AtomicU32::new(0)),
            cached_key: Arc::new(StdMutex::new(None)),
            success_tx: None,
        }
    }

    /// Creates a new unified authenticator that shares its success and failure tracking
    /// with an external monitor (used by the Server for wormhole auto-burn).
    pub fn with_tracking(
        state: StateConfig,
        policy: HostKeyPolicy,
        authorized_keys: Vec<PublicKey>,
        _temp_password_hash: Option<String>,
        monitor: PairingMonitor,
    ) -> Self {
        Self {
            state,
            policy,
            authorized_keys: Arc::new(StdMutex::new(authorized_keys)),
            temp_password_hash: _temp_password_hash,
            success_flag: monitor.success_flag,
            failed_attempts: monitor.failed_attempts,
            cached_key: Arc::new(StdMutex::new(None)),
            success_tx: monitor.success_tx,
        }
    }

    /// Returns the success flag, which is set to true when a NEW device is successfully added to the vault.
    pub fn was_successful(&self) -> bool {
        self.success_flag.load(Ordering::Relaxed)
    }

    /// Returns the number of failed password attempts.
    pub fn failed_attempts(&self) -> u32 {
        self.failed_attempts.load(Ordering::Relaxed)
    }

    fn lock_keys(&self) -> std::sync::MutexGuard<'_, Vec<PublicKey>> {
        match self.authorized_keys.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn refresh_keys(&self) -> Result<()> {
        let vault = crate::storage::load_all_authorized_clients(&self.state)?;
        let keys: Vec<_> = vault.into_iter().map(|(_, k)| k).collect();
        let mut authorized = self.lock_keys();
        *authorized = keys;
        Ok(())
    }

    fn check_password_match(&self, password: &str) -> Result<bool> {
        let argon2 = Argon2::default();

        // 1. Check Node Password (Permanent)
        // Refresh from disk to catch 'irosh passwd set' without restart
        let node_hash = crate::storage::load_shadow_file(&self.state).unwrap_or_default();
        if let Some(hash) = node_hash {
            let parsed_hash = PasswordHash::new(&hash)
                .map_err(|reason| AuthError::VerificationFailed { reason })?;
            if argon2
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok()
            {
                return Ok(true);
            }
        }

        // 2. Check Temp Password (Invite Pattern)
        if let Some(hash) = &self.temp_password_hash {
            let parsed_hash = PasswordHash::new(hash)
                .map_err(|reason| AuthError::VerificationFailed { reason })?;
            if argon2
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok()
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn notify_success(&self) {
        if let Some(tx) = &self.success_tx {
            let tx = tx.clone();
            tokio::spawn(async move {
                let _ = tx.send(()).await;
            });
        }
    }
}

impl Authenticator for UnifiedAuthenticator {
    fn supported_methods(&self) -> Vec<AuthMethod> {
        let mut methods = vec![AuthMethod::PublicKey];
        let node_pw_exists = crate::storage::load_shadow_file(&self.state)
            .unwrap_or_default()
            .is_some();
        if node_pw_exists || self.temp_password_hash.is_some() {
            methods.push(AuthMethod::Password);
        }
        methods
    }

    fn check_public_key(&self, _user: &str, key: &PublicKey) -> Result<bool> {
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();

        {
            let authorized = self.lock_keys();
            // 1. Established trust (Vault) always wins.
            if authorized.contains(key) {
                info!(%fingerprint, "Client matched pre-authorized key. Access granted.");
                return Ok(true);
            }
        }

        // If not found, refresh vault from disk to see if it was updated by another process.
        let _ = self.refresh_keys();
        let mut authorized = self.lock_keys();

        if authorized.contains(key) {
            info!(%fingerprint, "Client matched key after vault refresh. Access granted.");
            return Ok(true);
        }

        // 2. If node is under Strict policy and not empty, reject strangers early.
        if self.policy == HostKeyPolicy::Strict && !authorized.is_empty() {
            warn!(%fingerprint, "Strict policy: unknown key rejected.");
            return Ok(false);
        }

        // 3. If any password exists, we MUST reject the public key and force a password challenge.
        let node_pw_exists = crate::storage::load_shadow_file(&self.state)
            .unwrap_or_default()
            .is_some();
        if node_pw_exists || self.temp_password_hash.is_some() {
            if let Ok(mut cache) = self.cached_key.lock() {
                *cache = Some(key.clone());
            }
            return Ok(false);
        }

        // 4. No passwords set. Check for TOFU (Bootstrap phase).
        if authorized.is_empty() {
            info!(%fingerprint, "Vault is empty and no password set. Accepting first connection (TOFU).");
            let _event =
                crate::storage::trust::write_authorized_client(&self.state, &fingerprint, key)?;
            authorized.push(key.clone());
            self.success_flag.store(true, Ordering::Relaxed);
            self.notify_success();
            return Ok(true);
        }

        // 5. Default: Reject (Vault not empty, no password set).
        warn!(%fingerprint, "Vault is claimed and no password is set. Unknown key rejected.");
        Ok(false)
    }

    fn check_password(&self, _user: &str, password: &str) -> Result<bool> {
        if self.check_password_match(password)? {
            // Password accepted! Now we must authorize the key that was cached during the publickey step.
            if let Ok(cache) = self.cached_key.lock() {
                if let Some(key) = &*cache {
                    let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
                    let mut authorized = self.lock_keys();
                    if !authorized.contains(key) {
                        info!(%fingerprint, "Password accepted: Adding new client to vault.");
                        let _event = write_authorized_client(&self.state, &fingerprint, key)?;
                        authorized.push(key.clone());
                        self.success_flag.store(true, Ordering::Relaxed);
                        self.notify_success();
                    }
                    return Ok(true);
                }
            }
        }

        self.failed_attempts.fetch_add(1, Ordering::Relaxed);
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
    fn key_only_accept_all_accepts_any_key() -> crate::Result<()> {
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
        assert!(!auth.check_password("user", "pass")?);
        Ok(())
    }

    #[test]
    fn password_auth_validates_correct_password() -> crate::Result<()> {
        let password = "secret123";
        let hash = hash_password(password).expect("failed to hash test password");
        let auth = PasswordAuth::new(hash);

        assert!(auth.check_password("anyone", password)?);
        assert!(!auth.check_password("anyone", "wrong")?);
        assert!(!auth.check_password("anyone", "")?);

        // PublicKey should always be rejected.
        assert!(auth.supported_methods().contains(&AuthMethod::Password));
        assert!(!auth.supported_methods().contains(&AuthMethod::PublicKey));
        Ok(())
    }

    #[test]
    fn combined_auth_supports_both_methods() -> crate::Result<()> {
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
        assert!(auth.check_password("user", password)?);
        assert!(!auth.check_password("user", "wrong")?);
        Ok(())
    }

    #[test]
    fn unified_auth_tofu_works_with_no_passwords() -> crate::Result<()> {
        let state = temp_state("unified-tofu");
        let auth = UnifiedAuthenticator::new(state.clone(), HostKeyPolicy::Tofu, vec![], None);

        use russh::keys::ssh_key::PrivateKey;
        use russh::keys::ssh_key::private::Ed25519Keypair;

        let keypair = Ed25519Keypair::from_seed(&[0u8; 32]);
        let key = PrivateKey::from(keypair).public_key().clone();

        // 1. First connection should succeed (TOFU)
        assert!(auth.check_public_key("user", &key)?);

        // 2. Vault should now contain the key
        let vault = crate::storage::load_all_authorized_clients(&state)?;
        assert_eq!(vault.len(), 1);
        Ok(())
    }

    #[test]
    fn credentials_construction() {
        let creds = Credentials::new("admin", "pass123");
        assert_eq!(creds.user, "admin");
        assert_eq!(creds.password, "pass123");
    }
}

//! The unified security-policy authenticator and its pairing tracking handles.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use argon2::{Argon2, PasswordVerifier};
use async_trait::async_trait;
use russh::keys::ssh_key::{HashAlg, PublicKey};
use tracing::{info, warn};

use crate::config::{HostKeyPolicy, StateConfig};
use crate::error::Result;
use crate::storage::trust::write_authorized_client;

use super::{AuthMethod, Authenticator};

/// Decay window for the authentication rate limiter.
///
/// After this timeout the failure counter resets even if no successful
/// authentication occurred, preventing a permanent remote lockout.
const LOCKOUT_WINDOW: Duration = Duration::from_secs(60);

/// Tracking and notification handles for a pairing session.
#[derive(Debug, Clone)]
pub struct PairingMonitor {
    /// Flag set to true on successful pairing.
    pub success_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Counter for failed password attempts.
    pub failed_attempts: Arc<AtomicU32>,
    /// Notification channel for success.
    pub success_tx: Option<tokio::sync::mpsc::Sender<()>>,
    /// Notification channel for failure (rate limit reached).
    pub failure_tx: Option<tokio::sync::mpsc::Sender<()>>,
}

/// The master authenticator for irosh, implementing the unified security policy.
///
/// This authenticator governs all connection attempts (direct or via wormhole)
/// and enforces a strict precedence:
/// 1. Established trust (Vault) always wins.
/// 2. Permanent Node Password challenges unknown keys.
/// 3. Active Wormhole Temp Password (Invite Pattern) provides a one-time override.
/// 4. Empty Vault + No Passwords allows TOFU.
///
/// Failed attempts are rate limited with a decaying window ([`LOCKOUT_WINDOW`]):
/// the counter is reset on any successful authentication and expires on its own
/// after the window, so a single malicious client cannot permanently brick auth
/// for the whole node.
#[derive(Debug, Clone)]
pub struct UnifiedAuthenticator {
    state: StateConfig,
    policy: HostKeyPolicy,
    authorized_keys: Arc<StdMutex<Vec<PublicKey>>>,
    temp_password_hash: Option<String>,
    success_flag: Arc<std::sync::atomic::AtomicBool>,
    failed_attempts: Arc<AtomicU32>,
    /// Timestamp of the most recent failed authentication attempt.
    last_failure: Arc<StdMutex<Option<Instant>>>,
    /// Tracks the key currently attempting password auth.
    /// This is a single shared slot bridged across the SSH pubkey→password
    /// handshake within a connection; it is cleared after use (success or
    /// failure) to prevent a stale key from being authorized later.
    cached_key: Arc<StdMutex<Option<PublicKey>>>,
    /// Optional notification channel for successful pairing.
    success_tx: Option<tokio::sync::mpsc::Sender<()>>,
    /// Optional notification channel for failed pairing (rate limit reached).
    failure_tx: Option<tokio::sync::mpsc::Sender<()>>,
}

impl UnifiedAuthenticator {
    /// Creates a new unified authenticator.
    #[must_use]
    pub fn new(
        state: StateConfig,
        policy: HostKeyPolicy,
        authorized_keys: Vec<PublicKey>,
        temp_password_hash: Option<String>,
    ) -> Self {
        Self {
            state,
            policy,
            authorized_keys: Arc::new(StdMutex::new(authorized_keys)),
            temp_password_hash,
            success_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            failed_attempts: Arc::new(AtomicU32::new(0)),
            last_failure: Arc::new(StdMutex::new(None)),
            cached_key: Arc::new(StdMutex::new(None)),
            success_tx: None,
            failure_tx: None,
        }
    }

    /// Creates a new unified authenticator that shares its success and failure tracking
    /// with an external monitor (used by the Server for wormhole auto-burn).
    #[must_use]
    pub fn with_tracking(
        state: StateConfig,
        policy: HostKeyPolicy,
        authorized_keys: Vec<PublicKey>,
        temp_password_hash: Option<String>,
        monitor: PairingMonitor,
    ) -> Self {
        Self {
            state,
            policy,
            authorized_keys: Arc::new(StdMutex::new(authorized_keys)),
            temp_password_hash,
            success_flag: monitor.success_flag,
            failed_attempts: monitor.failed_attempts,
            last_failure: Arc::new(StdMutex::new(None)),
            cached_key: Arc::new(StdMutex::new(None)),
            success_tx: monitor.success_tx,
            failure_tx: monitor.failure_tx,
        }
    }

    /// Returns a view sharing all durable/tracking state but with a fresh
    /// per-handshake public-key cache.
    ///
    /// The server calls this once per accepted connection so that a concurrent
    /// handshake cannot overwrite the key cached by this one and get it
    /// authorized by the other's password step.
    #[must_use]
    pub fn for_new_session(&self) -> Self {
        Self {
            state: self.state.clone(),
            policy: self.policy,
            authorized_keys: self.authorized_keys.clone(),
            temp_password_hash: self.temp_password_hash.clone(),
            success_flag: self.success_flag.clone(),
            failed_attempts: self.failed_attempts.clone(),
            last_failure: self.last_failure.clone(),
            cached_key: Arc::new(StdMutex::new(None)),
            success_tx: self.success_tx.clone(),
            failure_tx: self.failure_tx.clone(),
        }
    }

    /// Returns the success flag, which is set to true when a NEW device is successfully added to the vault.
    #[must_use]
    pub fn was_successful(&self) -> bool {
        self.success_flag.load(Ordering::Relaxed)
    }

    /// Returns the number of failed password attempts.
    #[must_use]
    pub fn failed_attempts(&self) -> u32 {
        self.failed_attempts.load(Ordering::Relaxed)
    }

    /// Whether attempts are currently rate limited.
    ///
    /// Once [`LOCKOUT_WINDOW`] has passed since the last failure the counter
    /// decays and authentication is allowed again, so a remote peer cannot
    /// permanently lock out the node.
    fn is_locked_out(&self) -> bool {
        if self.failed_attempts.load(Ordering::Relaxed) < 3 {
            return false;
        }
        let last = match self.last_failure.lock() {
            Ok(guard) => guard.as_ref().copied(),
            Err(poisoned) => poisoned.into_inner().as_ref().copied(),
        };
        let Some(last) = last else {
            return true;
        };
        if last.elapsed() > LOCKOUT_WINDOW {
            self.failed_attempts.store(0, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// Resets the failure counter (used on successful authentication).
    fn reset_failures(&self) {
        self.failed_attempts.store(0, Ordering::Relaxed);
    }

    /// Records a failed authentication attempt and notifies the wormhole if the
    /// rate limit (3 failures within [`LOCKOUT_WINDOW`]) has been reached.
    fn record_failure(&self) {
        if let Ok(mut last) = self.last_failure.lock() {
            *last = Some(Instant::now());
        }
        let fails = self.failed_attempts.fetch_add(1, Ordering::Relaxed) + 1;
        if fails >= 3 {
            warn!("Authentication rate limit reached (3 failures). Burning wormhole.");
            if let Some(tx) = &self.failure_tx {
                let _ = tx.try_send(());
            }
        }
    }

    fn lock_keys(&self) -> std::sync::MutexGuard<'_, Vec<PublicKey>> {
        match self.authorized_keys.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("authorized keys mutex poisoned; recovering");
                poisoned.into_inner()
            }
        }
    }

    fn refresh_keys(&self) -> Result<()> {
        let vault = crate::storage::load_all_authorized_clients(&self.state)?;
        let keys: Vec<_> = vault.into_iter().map(|(_, k)| k).collect();
        let mut authorized = self.lock_keys();
        *authorized = keys;
        Ok(())
    }

    /// Returns `Some(is_wormhole)` if the password matches.
    /// `is_wormhole` is true if the temp (pairing) password was used, false if the node password was used.
    fn check_password_match(&self, password: &str) -> Option<bool> {
        let argon2 = Argon2::default();

        // 1. Check Node Password (Permanent)
        // Refresh from disk to catch 'irosh passwd set' without restart
        // NOTE: if the file doesn't exist, that's OK (no password configured).
        // Only fail-closed if the file exists but cannot be read.
        match crate::storage::load_shadow_file(&self.state) {
            Ok(Some(hash)) => {
                if argon2
                    .verify_password(password.as_bytes(), hash.as_str())
                    .is_ok()
                {
                    return Some(false);
                }
                warn!("Invalid shadow file hash format.");
            }
            Ok(None) => {} // no password configured, continue
            Err(_) => {
                warn!("Failed to read shadow file; failing closed");
                return None;
            }
        }

        // 2. Check Temp Password (Invite Pattern)
        if let Some(hash) = &self.temp_password_hash {
            if argon2
                .verify_password(password.as_bytes(), hash.as_str())
                .is_ok()
            {
                return Some(true);
            }
            warn!("Invalid temp password hash format.");
        }

        None
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

#[async_trait]
impl Authenticator for UnifiedAuthenticator {
    fn session_scoped(self: Arc<Self>) -> Arc<dyn Authenticator> {
        Arc::new(self.for_new_session())
    }

    async fn supported_methods(&self) -> Vec<AuthMethod> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut methods = vec![AuthMethod::PublicKey];
            let node_pw_exists = if let Ok(hash) = crate::storage::load_shadow_file(&this.state) {
                hash.is_some()
            } else {
                warn!("Failed to read shadow file, assuming password exists (fail-closed)");
                true
            };
            if node_pw_exists || this.temp_password_hash.is_some() {
                methods.push(AuthMethod::Password);
            }
            methods
        })
        .await
        .unwrap_or_else(|join_err| {
            warn!("supported_methods task failed: {join_err}");
            vec![AuthMethod::PublicKey]
        })
    }

    async fn check_public_key(&self, _user: &str, key: &PublicKey) -> Result<bool> {
        let this = self.clone();
        let key = key.clone();
        tokio::task::spawn_blocking(move || {
            if this.is_locked_out() {
                warn!("Authentication rejected: Rate limit exceeded.");
                return Ok(false);
            }
            let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();

            // 0. AcceptAll: accept every key without tracking.
            if this.policy == HostKeyPolicy::AcceptAll {
                return Ok(true);
            }

            {
                let authorized = this.lock_keys();
                // 1. Established trust (Vault) always wins.
                if authorized.contains(&key) {
                    info!(%fingerprint, "Client matched pre-authorized key. Access granted.");
                    this.reset_failures();
                    return Ok(true);
                }
            }

            // If not found, refresh vault from disk to see if it was updated by another process.
            let _ = this.refresh_keys();
            let authorized = this.lock_keys();

            if authorized.contains(&key) {
                info!(%fingerprint, "Client matched key after vault refresh. Access granted.");
                this.reset_failures();
                return Ok(true);
            }

            // 2. Strict policy rejects every key that is not already in the
            //    vault, including when the vault is empty. Unlike TOFU it never
            //    auto-adopts the first stranger. Bootstrap headlessly with
            //    `irosh host --authorize` or switch to the Tofu policy.
            if this.policy == HostKeyPolicy::Strict {
                warn!(%fingerprint, "Strict policy: unknown key rejected.");
                this.record_failure();
                return Ok(false);
            }

            // 3. If any password exists, we MUST reject the public key and force a password challenge.
            let node_pw_exists =
                if let Ok(hash) = crate::storage::load_shadow_file(&this.state) {
                    hash.is_some()
                } else {
                    warn!("Failed to read shadow file, assuming password exists (fail-closed)");
                    true
                };
            if node_pw_exists || this.temp_password_hash.is_some() {
                if let Ok(mut cache) = this.cached_key.lock() {
                    *cache = Some(key.clone());
                }
                return Ok(false);
            }

            // 4. No passwords set. Check for TOFU (Bootstrap phase).
            if authorized.is_empty() {
                info!(%fingerprint, "Vault is empty and no password set. Accepting first connection (TOFU).");
                let _event =
                    crate::storage::trust::write_authorized_client(&this.state, &fingerprint, &key)?;
                authorized.push(key.clone());
                this.success_flag.store(true, Ordering::Relaxed);
                this.reset_failures();
                this.notify_success();
                return Ok(true);
            }

            // 5. Default: Reject (Vault not empty, no password set).
            warn!(%fingerprint, "Vault is claimed and no password is set. Unknown key rejected.");
            this.record_failure();
            Ok(false)
        })
        .await
        .map_err(|e| crate::error::IroshError::Io(std::io::Error::other(e)))?
    }

    async fn check_password(&self, _user: &str, password: &str) -> Result<bool> {
        let this = self.clone();
        let password = password.to_string();
        tokio::task::spawn_blocking(move || {
            if this.is_locked_out() {
                warn!("Authentication rejected: Rate limit exceeded.");
                return Ok(false);
            }

            if let Some(is_wormhole) = this.check_password_match(&password) {
                // Password accepted!
                if is_wormhole {
                    // Wormhole code used: we must authorize the key that was cached during the publickey step.
                    if let Ok(mut cache) = this.cached_key.lock() {
                        if let Some(key) = cache.take() {
                            let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
                            let mut authorized = this.lock_keys();
                            if !authorized.contains(&key) {
                                info!(%fingerprint, "Wormhole code accepted: Adding new client to vault.");
                                let _event = write_authorized_client(&this.state, &fingerprint, &key)?;
                                authorized.push(key.clone());
                                this.success_flag.store(true, Ordering::Relaxed);
                                this.notify_success();
                            }
                        }
                    }
                } else {
                    info!("Node password accepted: Access granted for this session only.");
                    // Node-password auth does not authorize any key; drop the
                    // cached key so it cannot be consumed later.
                    if let Ok(mut cache) = this.cached_key.lock() {
                        *cache = None;
                    }
                }
                this.reset_failures();
                return Ok(true);
            }
            // Wrong or unknown password: drop the cached key so it cannot be
            // authorized by a later, unrelated password attempt.
            if let Ok(mut cache) = this.cached_key.lock() {
                *cache = None;
            }

            this.record_failure();
            Ok(false)
        })
        .await
        .map_err(|e| crate::error::IroshError::Io(std::io::Error::other(e)))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{block_on, hash_password, temp_state};

    /// Helper: generate a deterministic ed25519 public key from a seed byte.
    fn make_key(seed_byte: u8) -> russh::keys::ssh_key::PublicKey {
        use russh::keys::ssh_key::PrivateKey;
        use russh::keys::ssh_key::private::Ed25519Keypair;
        let mut seed = [0u8; 32];
        seed[0] = seed_byte;
        PrivateKey::from(Ed25519Keypair::from_seed(&seed))
            .public_key()
            .clone()
    }

    #[test]
    fn unified_auth_tofu_works_with_no_passwords() -> crate::Result<()> {
        use russh::keys::ssh_key::{PrivateKey, private::Ed25519Keypair};

        let state = temp_state("unified-tofu");
        let auth = UnifiedAuthenticator::new(state.clone(), HostKeyPolicy::Tofu, vec![], None);

        let keypair = Ed25519Keypair::from_seed(&[0u8; 32]);
        let key = PrivateKey::from(keypair).public_key().clone();

        // 1. First connection should succeed (TOFU)
        assert!(block_on(auth.check_public_key("user", &key))?);

        // 2. Vault should now contain the key
        let vault = crate::storage::load_all_authorized_clients(&state)?;
        assert_eq!(vault.len(), 1);
        Ok(())
    }

    /// A wormhole temp-password lets an unknown key authenticate and get
    /// permanently added to the vault on first success.
    #[test]
    fn unified_auth_temp_password_admits_new_key_and_stores_it() -> crate::Result<()> {
        let state = temp_state("unified-wormhole-admit");
        let password = "wormhole-secret";
        let hash = hash_password(password)?;

        let auth =
            UnifiedAuthenticator::new(state.clone(), HostKeyPolicy::Tofu, vec![], Some(hash));

        let unknown_key = make_key(0xAB);

        // Step 1: public-key check must FAIL (forces password challenge).
        assert!(
            !block_on(auth.check_public_key("user", &unknown_key))?,
            "unknown key should be rejected to force password challenge"
        );

        // Step 2: correct password must succeed and store the key in the vault.
        assert!(
            block_on(auth.check_password("user", password))?,
            "correct wormhole password must succeed"
        );
        assert!(
            auth.was_successful(),
            "success flag must be set after pairing"
        );

        // Step 3: vault must now contain the key.
        let vault = crate::storage::load_all_authorized_clients(&state)?;
        assert_eq!(vault.len(), 1, "paired key must be persisted to vault");

        // Step 4: the key is now trusted - subsequent connection must succeed
        // without a password, even on a freshly loaded authenticator.
        let auth2 = UnifiedAuthenticator::new(
            state.clone(),
            HostKeyPolicy::Strict,
            vault.into_iter().map(|(_, k)| k).collect(),
            None,
        );
        assert!(
            block_on(auth2.check_public_key("user", &unknown_key))?,
            "previously-paired key must be trusted on subsequent connection"
        );

        Ok(())
    }

    /// Two concurrent handshakes sharing one temp-password authenticator must
    /// not let one connection's cached public key be authorized by the other's
    /// password step. `session_scoped` gives each connection an isolated cache.
    #[test]
    fn unified_auth_session_scoped_isolates_cached_keys() -> crate::Result<()> {
        let state = temp_state("unified-session-isolation");
        let password = "wormhole-secret";
        let hash = hash_password(password)?;

        let shared: std::sync::Arc<dyn Authenticator> = std::sync::Arc::new(
            UnifiedAuthenticator::new(state.clone(), HostKeyPolicy::Tofu, vec![], Some(hash)),
        );

        // Each connection gets its own scoped authenticator.
        let session_a = shared.clone().session_scoped();
        let session_b = shared.clone().session_scoped();

        let key_a = make_key(0xA1);
        let key_b = make_key(0xB2);

        // Both unknown keys are rejected, caching a different key per session.
        assert!(!block_on(session_a.check_public_key("user", &key_a))?);
        assert!(!block_on(session_b.check_public_key("user", &key_b))?);

        // Session A's password must authorize A's key only.
        assert!(block_on(session_a.check_password("user", password))?);

        let vault = crate::storage::load_all_authorized_clients(&state)?;
        let stored: Vec<_> = vault.into_iter().map(|(_, k)| k).collect();
        assert_eq!(stored.len(), 1, "exactly one key must be paired");
        assert_eq!(
            stored[0].fingerprint(russh::keys::HashAlg::Sha256),
            key_a.fingerprint(russh::keys::HashAlg::Sha256),
            "session A must authorize its own key, not session B's cached key"
        );

        Ok(())
    }

    /// Wrong wormhole passwords must be rejected and each attempt must
    /// increment the failed-attempts counter (used by the server for rate-limiting).
    #[test]
    fn unified_auth_wrong_password_increments_failure_counter() -> crate::Result<()> {
        let state = temp_state("unified-wormhole-fail-count");
        let hash = hash_password("correct-password")?;

        let auth =
            UnifiedAuthenticator::new(state.clone(), HostKeyPolicy::Tofu, vec![], Some(hash));

        let key = make_key(0x01);

        // Force the key into the cache (simulates the SSH handshake sequence).
        let _ = block_on(auth.check_public_key("user", &key))?;

        // Three wrong attempts.
        assert!(!block_on(auth.check_password("user", "wrong-1"))?);
        assert!(!block_on(auth.check_password("user", "wrong-2"))?);
        assert!(!block_on(auth.check_password("user", "wrong-3"))?);

        assert_eq!(
            auth.failed_attempts(),
            3,
            "three failures must be recorded precisely"
        );
        assert!(
            !auth.was_successful(),
            "success flag must remain false after only failures"
        );

        Ok(())
    }

    /// Under Strict policy with a non-empty vault, unknown keys must be
    /// rejected immediately - no password challenge offered.
    #[test]
    fn unified_auth_strict_policy_rejects_stranger_immediately() -> crate::Result<()> {
        let state = temp_state("unified-strict-reject");
        let trusted_key = make_key(0x01);
        let stranger_key = make_key(0x02);

        // Persist the trusted key to disk so `refresh_keys` sees it.
        let fingerprint = trusted_key
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string();
        crate::storage::trust::write_authorized_client(&state, &fingerprint, &trusted_key)?;

        let auth = UnifiedAuthenticator::new(state, HostKeyPolicy::Strict, vec![trusted_key], None);

        assert!(
            !block_on(auth.check_public_key("user", &stranger_key))?,
            "Strict policy with a non-empty vault must reject unknown keys"
        );
        Ok(())
    }

    /// `supported_methods` must dynamically include `Password` when and
    /// only when a temp password hash is active - i.e. the method set must
    /// match the live security state.
    #[test]
    fn unified_auth_advertises_password_method_when_temp_hash_present() -> crate::Result<()> {
        let state = temp_state("unified-methods");
        let hash = hash_password("temp-pass")?;

        let auth_no_pw =
            UnifiedAuthenticator::new(state.clone(), HostKeyPolicy::Tofu, vec![], None);
        assert!(
            !block_on(auth_no_pw.supported_methods()).contains(&AuthMethod::Password),
            "without a temp hash, Password must not be advertised"
        );

        let auth_with_pw =
            UnifiedAuthenticator::new(state, HostKeyPolicy::Tofu, vec![], Some(hash));
        assert!(
            block_on(auth_with_pw.supported_methods()).contains(&AuthMethod::Password),
            "with a temp hash, Password must be advertised"
        );

        Ok(())
    }

    /// When the vault is non-empty but no password is configured, an unknown
    /// key must be rejected - there is no mechanism to admit it.
    #[test]
    fn unified_auth_claimed_vault_no_password_rejects_unknown_key() -> crate::Result<()> {
        let state = temp_state("unified-claimed-vault");
        let existing_key = make_key(0x01);
        let newcomer_key = make_key(0x02);

        // Pre-populate the vault by running a TOFU first-connection.
        let bootstrap = UnifiedAuthenticator::new(state.clone(), HostKeyPolicy::Tofu, vec![], None);
        assert!(block_on(bootstrap.check_public_key("user", &existing_key))?);

        // Load vault keys into a fresh authenticator - no password.
        let vault = crate::storage::load_all_authorized_clients(&state)?;
        let auth = UnifiedAuthenticator::new(
            state,
            HostKeyPolicy::Tofu,
            vault.into_iter().map(|(_, k)| k).collect(),
            None,
        );

        assert!(
            !block_on(auth.check_public_key("user", &newcomer_key))?,
            "claimed vault with no password must reject unknown keys"
        );
        assert!(
            block_on(auth.check_public_key("user", &existing_key))?,
            "claimed vault must still accept the existing trusted key"
        );

        Ok(())
    }
}

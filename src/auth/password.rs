//! Password-based authentication backend and password hashing.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use argon2::password_hash::Error as PasswordError;
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use async_trait::async_trait;
use russh::keys::ssh_key::PublicKey;

use crate::error::{AuthError, Result};

use super::{AuthMethod, Authenticator};

/// Window after which failed password attempts decay and the lockout lifts.
const LOCKOUT_WINDOW: Duration = Duration::from_secs(60);

/// Simple password authentication with a single shared password.
///
/// This is intended for personal or simple setups where one password
/// protects the server. The username is ignored - any user with the
/// correct password is accepted.
///
/// Failed attempts are rate limited with a decaying window
/// ([`LOCKOUT_WINDOW`]): three failures lock further checks until the window
/// passes, and any success resets the counter.
///
/// # Example (CLI)
///
/// ```bash
/// irosh-server --auth-mode password --auth-password "mySecret123"
/// ```
#[derive(Debug, Clone)]
pub struct PasswordAuth {
    password_hash: String,
    failed_attempts: Arc<AtomicU32>,
    last_failure: Arc<StdMutex<Option<Instant>>>,
}

impl PartialEq for PasswordAuth {
    fn eq(&self, other: &Self) -> bool {
        self.password_hash == other.password_hash
    }
}

impl Eq for PasswordAuth {}

impl PasswordAuth {
    /// Creates a new password authenticator with a pre-hashed password.
    pub fn new(password_hash: impl Into<String>) -> Self {
        Self {
            password_hash: password_hash.into(),
            failed_attempts: Arc::new(AtomicU32::new(0)),
            last_failure: Arc::new(StdMutex::new(None)),
        }
    }

    /// Returns the number of failed password attempts.
    #[must_use]
    pub fn failed_attempts(&self) -> u32 {
        self.failed_attempts.load(Ordering::Relaxed)
    }

    /// Whether attempts are currently rate limited.
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

    /// Records a failed authentication attempt.
    fn record_failure(&self) {
        if let Ok(mut last) = self.last_failure.lock() {
            *last = Some(Instant::now());
        }
        let fails = self.failed_attempts.fetch_add(1, Ordering::Relaxed) + 1;
        if fails >= 3 {
            tracing::warn!("Password authentication rate limit reached (3 failures).");
        }
    }
}

#[async_trait]
impl Authenticator for PasswordAuth {
    async fn supported_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::Password]
    }

    async fn check_public_key(&self, _user: &str, _key: &PublicKey) -> Result<bool> {
        Ok(false) // Password-only backend never accepts keys.
    }

    async fn check_password(&self, _user: &str, password: &str) -> Result<bool> {
        if self.is_locked_out() {
            return Ok(false);
        }

        let this = self.clone();
        let password = password.to_string();
        let verified = tokio::task::spawn_blocking(move || -> crate::Result<bool> {
            match Argon2::default()
                .verify_password(password.as_bytes(), this.password_hash.as_str())
            {
                Ok(()) => Ok(true),
                Err(PasswordError::PasswordInvalid) => Ok(false),
                Err(reason) => Err(AuthError::VerificationFailed { reason }.into()),
            }
        })
        .await
        .map_err(|e| crate::error::IroshError::Io(std::io::Error::other(e)))??;

        if verified {
            self.reset_failures();
        } else {
            self.record_failure();
        }
        Ok(verified)
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
/// Returns a [`crate::error::StorageError::PasswordHash`] if salt generation or hashing fails.
#[must_use]
pub fn hash_password(password: &str) -> Result<String> {
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes())
        .map_err(|reason| crate::error::StorageError::PasswordHash { reason })?
        .to_string();

    Ok(password_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::block_on;

    #[test]
    fn password_auth_validates_correct_password() -> crate::Result<()> {
        let password = "secret123";
        let hash = hash_password(password).expect("failed to hash test password");
        let auth = PasswordAuth::new(hash);

        assert!(block_on(auth.check_password("anyone", password))?);
        assert!(!block_on(auth.check_password("anyone", "wrong"))?);
        assert!(!block_on(auth.check_password("anyone", ""))?);

        // PublicKey should always be rejected.
        assert!(block_on(auth.supported_methods()).contains(&AuthMethod::Password));
        assert!(!block_on(auth.supported_methods()).contains(&AuthMethod::PublicKey));
        Ok(())
    }

    /// Regression test for the password rate-limit fix: three failures lock
    /// further checks inside the 60 s window, so even the correct password
    /// must be rejected until the window passes.
    #[test]
    fn password_auth_locks_out_after_three_failures() -> crate::Result<()> {
        let password = "secret123";
        let hash = hash_password(password).expect("failed to hash test password");
        let auth = PasswordAuth::new(hash);

        assert!(!block_on(auth.check_password("anyone", "wrong-1"))?);
        assert!(!block_on(auth.check_password("anyone", "wrong-2"))?);
        assert!(!block_on(auth.check_password("anyone", "wrong-3"))?);
        assert_eq!(
            auth.failed_attempts(),
            3,
            "three failures must be recorded precisely"
        );

        assert!(
            !block_on(auth.check_password("anyone", password))?,
            "correct password must be rejected while the lockout window is active"
        );
        Ok(())
    }
}

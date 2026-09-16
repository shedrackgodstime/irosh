//! Password-based authentication backend and password hashing.

use argon2::password_hash::Error as PasswordError;
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use async_trait::async_trait;
use russh::keys::ssh_key::PublicKey;

use crate::error::{AuthError, Result};

use super::{AuthMethod, Authenticator};

/// Simple password authentication with a single shared password.
///
/// This is intended for personal or simple setups where one password
/// protects the server. The username is ignored - any user with the
/// correct password is accepted.
///
/// # Example (CLI)
///
/// ```bash
/// irosh-server --auth-mode password --auth-password "mySecret123"
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[async_trait]
impl Authenticator for PasswordAuth {
    async fn supported_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::Password]
    }

    async fn check_public_key(&self, _user: &str, _key: &PublicKey) -> Result<bool> {
        Ok(false) // Password-only backend never accepts keys.
    }

    async fn check_password(&self, _user: &str, password: &str) -> Result<bool> {
        let this = self.clone();
        let password = password.to_string();
        tokio::task::spawn_blocking(move || {
            match Argon2::default()
                .verify_password(password.as_bytes(), this.password_hash.as_str())
            {
                Ok(()) => Ok(true),
                Err(PasswordError::PasswordInvalid) => Ok(false),
                Err(reason) => Err(AuthError::VerificationFailed { reason }.into()),
            }
        })
        .await
        .map_err(|e| crate::error::IroshError::Io(std::io::Error::other(e)))?
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
}

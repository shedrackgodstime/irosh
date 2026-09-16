//! Key-based authentication backends.
//!
//! - [`KeyOnlyAuth`] - The default backend. Replicates the existing
//!   TOFU/Strict/AcceptAll key-based authentication. Zero change for existing users.
//! - [`CombinedAuth`] - Accepts either public keys or passwords, delegating to a
//!   [`KeyOnlyAuth`] and a [`PasswordAuth`](crate::auth::PasswordAuth).

use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use russh::keys::ssh_key::{HashAlg, PublicKey};
use tracing::{info, warn};

use crate::config::{HostKeyPolicy, SecurityConfig, StateConfig};
use crate::error::Result;
use crate::storage::trust::write_authorized_client;

use super::{AuthMethod, Authenticator};

/// Key-only authentication using TOFU/Strict/AcceptAll policies.
///
/// This replicates the existing irosh authentication behavior exactly.
/// It is used automatically when no custom [`Authenticator`] is configured
/// on [`ServerOptions`](crate::ServerOptions).
#[derive(Debug, Clone)]
pub struct KeyOnlyAuth {
    policy: HostKeyPolicy,
    authorized_keys: Arc<StdMutex<Vec<PublicKey>>>,
    state: StateConfig,
}

impl KeyOnlyAuth {
    /// Creates a new key-only authenticator with the given policy and initial keys.
    #[must_use]
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

#[async_trait]
impl Authenticator for KeyOnlyAuth {
    async fn supported_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::PublicKey]
    }

    async fn check_public_key(&self, _user: &str, key: &PublicKey) -> Result<bool> {
        let this = self.clone();
        let key = key.clone();
        tokio::task::spawn_blocking(move || {
            let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();

            if this.policy == HostKeyPolicy::AcceptAll {
                info!(%fingerprint, "AcceptAll policy: automatically accepting client key.");
                return Ok(true);
            }

            let mut authorized = this.lock_keys();

            if !authorized.is_empty() {
                if authorized.contains(&key) {
                    info!(%fingerprint, "Client matched pre-authorized key. Access granted.");
                    return Ok(true);
                }
                warn!(%fingerprint, "Client key not in authorized list. Rejecting connection.");
                return Ok(false);
            }

            // No authorized keys yet - check policy for new keys.
            match this.policy {
                HostKeyPolicy::Strict => {
                    warn!(%fingerprint, "Strict policy: No pre-authorized keys found. Rejecting connection.");
                    Ok(false)
                }
                HostKeyPolicy::Tofu => {
                    info!(%fingerprint, "Tofu policy: No pre-authorized keys found. Trusting first client.");
                    let _event = write_authorized_client(&this.state, &fingerprint, &key)?;
                    authorized.push(key.clone());
                    Ok(true)
                }
                HostKeyPolicy::AcceptAll => {
                    info!(%fingerprint, "AcceptAll policy: automatically accepting client key.");
                    Ok(true)
                }
            }
        })
        .await
        .map_err(|e| crate::error::IroshError::Io(std::io::Error::other(e)))?
    }

    async fn check_password(&self, _user: &str, _password: &str) -> Result<bool> {
        Ok(false) // Key-only backend never accepts passwords.
    }
}

/// Combined authentication accepting either public keys or passwords.
///
/// This delegates to a [`KeyOnlyAuth`] for key checks and a [`PasswordAuth`](crate::auth::PasswordAuth)
/// for password checks. A client can authenticate with either method.
#[derive(Debug, Clone)]
pub struct CombinedAuth {
    key_auth: KeyOnlyAuth,
    password_auth: crate::auth::PasswordAuth,
}

impl CombinedAuth {
    /// Creates a combined authenticator from a key backend and a password backend.
    #[must_use]
    pub fn new(key_auth: KeyOnlyAuth, password_auth: crate::auth::PasswordAuth) -> Self {
        Self {
            key_auth,
            password_auth,
        }
    }
}

#[async_trait]
impl Authenticator for CombinedAuth {
    async fn supported_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::PublicKey, AuthMethod::Password]
    }

    async fn check_public_key(&self, user: &str, key: &PublicKey) -> Result<bool> {
        self.key_auth.check_public_key(user, key).await
    }

    async fn check_password(&self, user: &str, password: &str) -> Result<bool> {
        self.password_auth.check_password(user, password).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{block_on, hash_password, temp_state};
    use crate::config::SecurityConfig;

    #[test]
    fn key_only_accept_all_accepts_any_key() -> crate::Result<()> {
        let auth = KeyOnlyAuth::new(
            SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            },
            vec![],
            temp_state("accept-all"),
        );
        assert!(block_on(auth.supported_methods()).contains(&AuthMethod::PublicKey));
        assert!(!block_on(auth.supported_methods()).contains(&AuthMethod::Password));
        // Password should always be rejected.
        assert!(!block_on(auth.check_password("user", "pass"))?);
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
        let pass = crate::auth::PasswordAuth::new(hash);
        let auth = CombinedAuth::new(key, pass);

        assert_eq!(block_on(auth.supported_methods()).len(), 2);
        assert!(block_on(auth.supported_methods()).contains(&AuthMethod::PublicKey));
        assert!(block_on(auth.supported_methods()).contains(&AuthMethod::Password));
        assert!(block_on(auth.check_password("user", password))?);
        assert!(!block_on(auth.check_password("user", "wrong"))?);
        Ok(())
    }
}

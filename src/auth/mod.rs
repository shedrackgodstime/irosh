//! Pluggable authentication backends for irosh.
//!
//! This module provides the [`Authenticator`] trait that defines how credentials
//! are validated. Library consumers implement this to control authentication
//! logic. The CLI ships with built-in implementations for common use cases.
//!
//! Under the **C-CALLER-CONTROL** principle, the library never decides **how** to
//! validate credentials - it only calls the trait methods and respects the result.
//!
//! # Built-in Backends
//!
//! - [`KeyOnlyAuth`] - The default. Replicates the existing TOFU/Strict/AcceptAll
//!   key-based authentication. Zero change for existing users.
//! - [`PasswordAuth`] - A single shared password for all connections. Good for
//!   personal or simple setups.
//! - [`CombinedAuth`] - Accepts either public keys or passwords.
//! - [`UnifiedAuthenticator`] - The master security policy for Irosh V2. Manages the
//!   precedence between established trust, node passwords, and temporary wormhole codes.

use std::fmt;

use async_trait::async_trait;
use russh::keys::ssh_key::PublicKey;

use crate::error::Result;

/// Which authentication methods a backend supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum AuthMethod {
    /// SSH public key authentication.
    PublicKey,
    /// Username + password authentication.
    Password,
}

/// The overall authentication policy mode for the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum AuthMode {
    /// Only SSH public keys are allowed (Strict/TOFU).
    Key,
    /// Only passwords are allowed.
    Password,
    /// Both keys and passwords are allowed.
    Combined,
    /// The intelligent, auto-detecting policy (default).
    Unified,
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
/// use async_trait::async_trait;
///
/// struct MyAuth;
///
/// impl std::fmt::Debug for MyAuth {
///     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
///         f.debug_struct("MyAuth").finish()
///     }
/// }
///
/// #[async_trait]
/// impl Authenticator for MyAuth {
///     async fn supported_methods(&self) -> Vec<AuthMethod> {
///         vec![AuthMethod::Password]
///     }
///     async fn check_public_key(&self, _user: &str, _key: &PublicKey) -> irosh::Result<bool> {
///         Ok(false)
///     }
///     async fn check_password(&self, _user: &str, password: &str) -> irosh::Result<bool> {
///         Ok(password == "secret")
///     }
/// }
/// ```
#[async_trait]
pub trait Authenticator: Send + Sync + fmt::Debug + 'static {
    /// Returns which auth methods this backend supports.
    ///
    /// The server will advertise these methods to clients during the SSH
    /// handshake. Methods not listed here will be rejected immediately.
    async fn supported_methods(&self) -> Vec<AuthMethod>;

    /// Validate a public key for the given user.
    ///
    /// Return `Ok(true)` to accept, `Ok(false)` to reject.
    /// Return `Err(...)` for internal failures.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying storage or cryptographic operation fails.
    async fn check_public_key(&self, user: &str, key: &PublicKey) -> Result<bool>;

    /// Validate a username + password combination.
    ///
    /// Return `Ok(true)` to accept, `Ok(false)` to reject.
    /// Return `Err(...)` for internal failures.
    ///
    /// # Errors
    ///
    /// Returns an error if the authentication fails or the credentials are invalid.
    async fn check_password(&self, user: &str, password: &str) -> Result<bool>;

    /// Returns a session-scoped view of this authenticator.
    ///
    /// The server calls this once per accepted connection so that any state
    /// bridging the SSH public-key and password steps is never shared between
    /// concurrent handshakes. Implementations that hold such per-handshake
    /// state (e.g. [`UnifiedAuthenticator`]'s public-key cache) must override
    /// this to return a fresh view. Stateless or intentionally-shared
    /// implementations can rely on the default, which forwards `self`.
    #[must_use]
    fn session_scoped(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn Authenticator> {
        std::sync::Arc::new(SharedSession(self))
    }
}

/// Default [`Authenticator::session_scoped`] adapter: forwards every call to
/// the wrapped (shared) authenticator unchanged.
struct SharedSession<T: ?Sized>(std::sync::Arc<T>);

impl<T: Authenticator + ?Sized> fmt::Debug for SharedSession<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

#[async_trait]
impl<T: Authenticator + ?Sized> Authenticator for SharedSession<T> {
    fn session_scoped(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn Authenticator> {
        self
    }

    async fn supported_methods(&self) -> Vec<AuthMethod> {
        self.0.supported_methods().await
    }

    async fn check_public_key(&self, user: &str, key: &PublicKey) -> Result<bool> {
        self.0.check_public_key(user, key).await
    }

    async fn check_password(&self, user: &str, password: &str) -> Result<bool> {
        self.0.check_password(user, password).await
    }
}

mod credentials;
mod key;
mod password;
mod prompter;
mod unified;

pub use credentials::Credentials;
pub use key::{CombinedAuth, KeyOnlyAuth};
pub use password::{PasswordAuth, hash_password};
pub use prompter::{ConfirmationCallback, PasswordPrompter};
pub use unified::{PairingMonitor, UnifiedAuthenticator};

#[cfg(test)]
pub(crate) fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Runtime::new().unwrap().block_on(f)
}

#[cfg(test)]
pub(crate) fn temp_state(name: &str) -> crate::config::StateConfig {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "irosh-auth-test-{}-{}",
        name,
        rand::random::<u32>()
    ));
    crate::config::StateConfig::new(path)
}

#[cfg(test)]
mod send_sync_tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn authenticators_are_send_sync() {
        assert_send_sync::<KeyOnlyAuth>();
        assert_send_sync::<PasswordAuth>();
        assert_send_sync::<CombinedAuth>();
        assert_send_sync::<UnifiedAuthenticator>();
    }
}

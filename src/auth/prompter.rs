//! Client-side interactive prompting callbacks.

use russh::keys::ssh_key::PublicKey;

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

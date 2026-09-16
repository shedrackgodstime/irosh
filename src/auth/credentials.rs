//! Client-side credentials.

use secrecy::SecretString;

/// Credentials for password-based authentication on the client side.
///
/// When provided to [`ClientOptions`](crate::ClientOptions), the client will
/// attempt password authentication if public key authentication is rejected.
///
/// The password is stored in a zeroizing wrapper ([`SecretString`]) so that the
/// plaintext is overwritten in memory when the value is dropped.
#[derive(Debug, Clone)]
pub struct Credentials {
    /// The username to authenticate as.
    pub user: String,
    /// The password (zeroized on drop).
    pub password: SecretString,
}

impl Credentials {
    /// Creates a new credentials pair.
    ///
    /// The password is immediately converted into a [`SecretString`] and will
    /// be zeroed when the credentials are dropped.
    pub fn new(user: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            password: SecretString::from(password.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn credentials_construction() {
        let creds = Credentials::new("admin", "pass123");
        assert_eq!(creds.user, "admin");
        assert_eq!(creds.password.expose_secret(), "pass123");
    }
}

//! Authentication and credential errors.

/// Authentication and credential errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuthError {
    /// Password verification failed due to an incorrect password.
    #[error("invalid password provided")]
    InvalidPassword,

    /// Password verification failed due to a cryptographic or format error.
    #[error("password verification failed: {reason}")]
    VerificationFailed {
        /// The underlying error from the argon2 crate.
        ///
        /// NOTE: This does not use `#[source]` because `argon2::password_hash::Error`
        /// does not currently implement `std::error::Error`.
        reason: argon2::password_hash::Error,
    },

    /// The required authentication method is not supported by the client or server.
    #[error("unsupported authentication method: {0}")]
    UnsupportedMethod(String),

    /// A required credential (like a password) was not provided.
    #[error("missing required credential: {0}")]
    MissingCredential(String),
}

//! Session state types shared by the client and CLI layers.

/// The lifecycle state of an irosh client session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// A connection attempt has started but transport is not yet established.
    Dialing,
    /// The Iroh transport connection is established.
    TransportConnected,
    /// The SSH layer is negotiating and authenticating.
    SshHandshaking,
    /// SSH authentication succeeded and a session channel is open.
    Authenticated,
    /// The remote shell has been requested and the session is ready for I/O.
    ShellReady,
    /// The connection attempt failed because the remote rejected authentication.
    AuthRejected,
    /// The connection attempt failed because the remote host key mismatched trust state.
    TrustMismatch,
    /// The session is closed locally or remotely.
    Closed,
}

impl SessionState {
    /// Returns whether this state terminates the current session lifecycle.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            SessionState::AuthRejected | SessionState::TrustMismatch | SessionState::Closed
        )
    }
}

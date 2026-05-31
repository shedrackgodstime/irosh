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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialing_is_not_terminal() {
        assert!(!SessionState::Dialing.is_terminal());
    }

    #[test]
    fn transport_connected_is_not_terminal() {
        assert!(!SessionState::TransportConnected.is_terminal());
    }

    #[test]
    fn ssh_handshaking_is_not_terminal() {
        assert!(!SessionState::SshHandshaking.is_terminal());
    }

    #[test]
    fn authenticated_is_not_terminal() {
        assert!(!SessionState::Authenticated.is_terminal());
    }

    #[test]
    fn shell_ready_is_not_terminal() {
        assert!(!SessionState::ShellReady.is_terminal());
    }

    #[test]
    fn auth_rejected_is_terminal() {
        assert!(SessionState::AuthRejected.is_terminal());
    }

    #[test]
    fn trust_mismatch_is_terminal() {
        assert!(SessionState::TrustMismatch.is_terminal());
    }

    #[test]
    fn closed_is_terminal() {
        assert!(SessionState::Closed.is_terminal());
    }

    #[test]
    fn session_state_copy_semantics() {
        let state = SessionState::Dialing;
        let copied = state;
        assert_eq!(state, copied);
    }

    #[test]
    fn session_state_debug_output() {
        let debug = format!("{:?}", SessionState::ShellReady);
        assert_eq!(debug, "ShellReady");
    }
}

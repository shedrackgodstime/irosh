use super::*;
use crate::config::HostKeyPolicy;
use crate::error::ClientError;
use crate::storage::{PeerProfile, save_peer};
use crate::transport::ticket::Ticket;

#[test]
fn parse_target_resolves_saved_peer_alias() {
    let state = temp_state_dir("peer-alias");
    let node_id = iroh::SecretKey::generate(&mut rand::rng()).public();
    let ticket_text = Ticket::new(iroh::EndpointAddr::new(node_id)).to_string();

    save_peer(
        &state,
        &PeerProfile {
            name: "local-server".to_string(),
            ticket: ticket_text.parse().unwrap(),
        },
    )
    .unwrap();

    let resolved = Client::parse_target(&state, "local-server").unwrap();
    let expected: Ticket = ticket_text.parse().unwrap();
    assert_eq!(resolved, expected);
}

#[test]
fn classify_connect_error_maps_security_failures_to_terminal_states() {
    assert_eq!(
        Client::classify_connect_error(&IroshError::AuthenticationFailed),
        SessionState::AuthRejected
    );
    assert_eq!(
        Client::classify_connect_error(&IroshError::ServerKeyMismatch {
            expected: "a".to_string(),
            actual: "b".to_string(),
        }),
        SessionState::TrustMismatch
    );
    assert_eq!(
        Client::classify_connect_error(&IroshError::Client(ClientError::TransportUnavailable {
            details: "connection dropped"
        })),
        SessionState::Closed
    );
}

#[test]
fn pty_options_builder_retains_term_size_and_modes() {
    let size = crate::session::pty::pty_size(120, 40, 10, 20);
    let options = PtyOptions::new("xterm-256color", size).modes(vec![(russh::Pty::ECHO, 1)]);

    assert_eq!(options.term(), "xterm-256color");
    assert_eq!(options.size(), size);
    assert_eq!(options.modes_slice(), &[(russh::Pty::ECHO, 1)]);
}

#[test]
fn client_options_builder_retains_state_security_and_secret() {
    let state = temp_state_dir("client-options");
    let options = ClientOptions::new(state.clone())
        .security(SecurityConfig {
            host_key_policy: HostKeyPolicy::AcceptAll,
        })
        .secret("shared-secret");

    assert_eq!(options.state().root(), state.root());
    assert_eq!(
        options.security_config().host_key_policy,
        HostKeyPolicy::AcceptAll
    );
    assert_eq!(options.secret_value(), Some("shared-secret"));
}

#[test]
fn public_config_and_ticket_types_support_value_comparison() {
    let state_a = StateConfig::new("/tmp/irosh-state-a".into());
    let state_b = StateConfig::new("/tmp/irosh-state-a".into());
    assert_eq!(state_a, state_b);

    let security_a = SecurityConfig {
        host_key_policy: HostKeyPolicy::Strict,
    };
    let security_b = SecurityConfig {
        host_key_policy: HostKeyPolicy::Strict,
    };
    assert_eq!(security_a, security_b);

    let node_id = iroh::SecretKey::generate(&mut rand::rng()).public();
    let ticket_a = Ticket::new(iroh::EndpointAddr::new(node_id));
    let ticket_b = ticket_a.clone();
    assert_eq!(ticket_a, ticket_b);
}

#[test]
fn public_runtime_types_implement_debug() {
    fn assert_debug<T: std::fmt::Debug>() {}

    assert_debug::<Client>();
    assert_debug::<Session>();
    assert_debug::<crate::Server>();
    assert_debug::<crate::ServerShutdown>();
    assert_debug::<crate::storage::NodeIdentity>();
    assert_debug::<crate::session::RawTerminal>();
}

#[test]
fn test_session_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Session>();
}

#[test]
fn ticket_supports_try_from_str_and_string() {
    let node_id = iroh::SecretKey::generate(&mut rand::rng()).public();
    let ticket_text = Ticket::new(iroh::EndpointAddr::new(node_id)).to_string();

    let from_str = Ticket::try_from(ticket_text.as_str()).unwrap();
    let from_string = Ticket::try_from(ticket_text.clone()).unwrap();

    assert_eq!(from_str, from_string);
}

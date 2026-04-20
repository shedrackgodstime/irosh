use super::*;
use crate::config::HostKeyPolicy;
use crate::storage::load_or_generate_identity;
use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn temp_state_dir(label: &str) -> StateConfig {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("irosh-test-{label}-{unique}"));
    fs::create_dir_all(&path).unwrap();
    StateConfig::new(path)
}

#[tokio::test]
async fn shutdown_handle_stops_run_loop_cleanly() {
    let state = temp_state_dir("server-shutdown");
    let options = ServerOptions::new(state).security(SecurityConfig {
        host_key_policy: HostKeyPolicy::Strict,
    });

    let Ok((_ready, server)) = Server::bind(options).await else {
        return;
    };
    let shutdown = server.shutdown_handle();
    let run_task = tokio::spawn(async move { server.run().await });

    shutdown.close().await;

    let result = tokio::time::timeout(Duration::from_secs(10), run_task)
        .await
        .expect("server run task timed out")
        .expect("server run task panicked");

    assert!(result.is_ok());
}

#[tokio::test]
async fn server_options_builder_retains_state_security_secret_and_keys() {
    let state = temp_state_dir("server-options");
    let authorized_identity = load_or_generate_identity(&temp_state_dir("authorized-client"))
        .await
        .unwrap();
    let authorized_key = authorized_identity.ssh_key.public_key().clone();

    let options = ServerOptions::new(state.clone())
        .security(SecurityConfig {
            host_key_policy: HostKeyPolicy::AcceptAll,
        })
        .secret("shared-secret")
        .authorized_key(authorized_key.clone());

    assert_eq!(options.state().root(), state.root());
    assert_eq!(
        options.security_config().host_key_policy,
        HostKeyPolicy::AcceptAll
    );
    assert_eq!(options.secret_value(), Some("shared-secret"));
    assert_eq!(options.authorized_key_list(), &[authorized_key]);
}

#[test]
fn test_server_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Server>();
    assert_send_sync::<ServerShutdown>();
}

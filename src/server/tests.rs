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
        .authorized_keys(vec![authorized_key.clone()]);

    assert_eq!(options.state().root(), state.root());
    assert_eq!(
        options.security_config().host_key_policy,
        HostKeyPolicy::AcceptAll
    );
    assert_eq!(options.secret_value(), Some("shared-secret"));
}

#[test]
fn test_server_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Server>();
    assert_send_sync::<ServerShutdown>();
}

#[tokio::test]
async fn wormhole_rate_limit_burns_after_three_failed_attempts() {
    use crate::auth::Credentials;
    use crate::client::ipc::IpcClient;
    use crate::client::{Client, ClientOptions};
    use crate::server::ipc::{IpcCommand, IpcResponse};

    let state = temp_state_dir("server-rate-limit");
    let options = ServerOptions::new(state.clone());
    let (ready, server) = Server::bind(options).await.unwrap();
    let shutdown = server.shutdown_handle();
    let run_task = tokio::spawn(server.run());

    // Give IPC server time to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    let ipc = IpcClient::new(state.root().to_path_buf());

    let password_hash = crate::auth::hash_password("correct-password").unwrap();

    ipc.send(IpcCommand::EnableWormhole {
        code: "test-code".to_string(),
        password: Some(password_hash),
        persistent: false,
    })
    .await
    .unwrap();

    // Verify it's active
    let status = ipc.send(IpcCommand::GetStatus).await.unwrap();
    if let IpcResponse::Status(info) = status {
        assert!(info.wormhole_active, "Wormhole should be active");
    } else {
        panic!("Expected Status response");
    }

    for i in 0..3 {
        let client_options =
            ClientOptions::new(temp_state_dir(&format!("client-rate-limit-{}", i)))
                .security(crate::config::SecurityConfig {
                    host_key_policy: HostKeyPolicy::AcceptAll,
                })
                .credentials(Credentials {
                    user: "irosh".to_string(),
                    password: "wrong-password".to_string(),
                });
        let connection_info = Client::dial_p2p(&client_options, ready.ticket().clone(), true)
            .await
            .unwrap();
        let result = Client::establish_session(&client_options, connection_info).await;

        assert!(
            result.is_err(),
            "Expected auth to fail on attempt {}",
            i + 1
        );
    }

    // Give the server a moment to process the third failure and burn the wormhole
    // We wait until active_sessions is 0 to ensure the join_next() has run.
    let mut success = false;
    for _ in 0..50 {
        let status = ipc.send(IpcCommand::GetStatus).await.unwrap();
        if let IpcResponse::Status(info) = status {
            if !info.wormhole_active && info.active_sessions == 0 {
                success = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    assert!(
        success,
        "Wormhole should be disabled and sessions cleared after 3 failures"
    );

    shutdown.close().await;
    let _ = run_task.await;
}

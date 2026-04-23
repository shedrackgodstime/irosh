use super::*;

#[tokio::test]
async fn publickey_auth_round_trip_succeeds_over_duplex_stream() {
    let server_state = temp_state_dir("server");
    let client_state = temp_state_dir("client");

    let server_identity = load_or_generate_identity(&server_state).await.unwrap();
    let client_identity = load_or_generate_identity(&client_state).await.unwrap();
    let (client_stream, server_stream) = duplex(1024 * 1024);

    let server_config = Arc::new(server::Config {
        auth_rejection_time: std::time::Duration::from_secs(1),
        keys: vec![server_identity.ssh_key],
        ..Default::default()
    });
    let server_handler = ServerHandler::new(
        Vec::new(),
        SecurityConfig {
            host_key_policy: crate::config::HostKeyPolicy::Tofu,
        },
        server_state.clone(),
        crate::server::transfer::ConnectionShellState::new(),
    );
    let server_task = tokio::spawn(async move {
        server::run_stream(server_config, server_stream, server_handler).await
    });

    let client_config = Arc::new(client::Config::default());
    let last_disconnect = Arc::new(StdMutex::new(None));
    let client_handler = ClientHandler::new(
        "test-node".to_string(),
        None,
        last_disconnect.clone(),
        SecurityConfig {
            host_key_policy: crate::config::HostKeyPolicy::Tofu,
        },
        client_state.clone(),
    );

    let mut handle = client::connect_stream(client_config, client_stream, client_handler)
        .await
        .unwrap();
    let auth = handle
        .authenticate_publickey(
            "demo",
            russh::keys::PrivateKeyWithHashAlg::new(Arc::new(client_identity.ssh_key), None),
        )
        .await
        .unwrap();
    assert!(matches!(auth, client::AuthResult::Success));

    let _ = handle
        .disconnect(russh::Disconnect::ByApplication, "", "English")
        .await;
    let _ = server_task.await.unwrap();
}

#[tokio::test]
async fn publickey_auth_is_rejected_for_untrusted_client_key() {
    let server_state = temp_state_dir("server-auth-reject");
    let client_state = temp_state_dir("client-auth-reject");
    let authorized_state = temp_state_dir("authorized-client");

    let server_identity = load_or_generate_identity(&server_state).await.unwrap();
    let client_identity = load_or_generate_identity(&client_state).await.unwrap();
    let authorized_identity = load_or_generate_identity(&authorized_state).await.unwrap();
    let (client_stream, server_stream) = duplex(1024 * 1024);

    let server_config = Arc::new(server::Config {
        auth_rejection_time: std::time::Duration::from_secs(1),
        keys: vec![server_identity.ssh_key],
        ..Default::default()
    });
    let server_handler = ServerHandler::new(
        vec![authorized_identity.ssh_key.public_key().clone()],
        SecurityConfig {
            host_key_policy: crate::config::HostKeyPolicy::Tofu,
        },
        server_state.clone(),
        crate::server::transfer::ConnectionShellState::new(),
    );
    let server_task = tokio::spawn(async move {
        server::run_stream(server_config, server_stream, server_handler).await
    });

    let client_config = Arc::new(client::Config::default());
    let last_disconnect = Arc::new(StdMutex::new(None));
    let client_handler = ClientHandler::new(
        "test-node".to_string(),
        None,
        last_disconnect,
        SecurityConfig {
            host_key_policy: crate::config::HostKeyPolicy::Tofu,
        },
        client_state.clone(),
    );

    let mut handle = client::connect_stream(client_config, client_stream, client_handler)
        .await
        .unwrap();
    let auth = handle
        .authenticate_publickey(
            "demo",
            russh::keys::PrivateKeyWithHashAlg::new(Arc::new(client_identity.ssh_key), None),
        )
        .await
        .unwrap();

    assert!(matches!(
        auth,
        client::AuthResult::Failure {
            partial_success: false,
            ..
        }
    ));

    let _ = handle
        .disconnect(russh::Disconnect::ByApplication, "", "English")
        .await;
    let _ = server_task.await.unwrap();
}

#[tokio::test]
async fn connect_stream_fails_on_server_key_mismatch() {
    let server_state = temp_state_dir("server-key-mismatch");
    let client_state = temp_state_dir("client-key-mismatch");
    let wrong_server_state = temp_state_dir("wrong-server-key");

    let server_identity = load_or_generate_identity(&server_state).await.unwrap();
    let _client_identity = load_or_generate_identity(&client_state).await.unwrap();
    let wrong_server_identity = load_or_generate_identity(&wrong_server_state)
        .await
        .unwrap();
    let (client_stream, server_stream) = duplex(1024 * 1024);

    let server_config = Arc::new(server::Config {
        auth_rejection_time: std::time::Duration::from_secs(1),
        keys: vec![server_identity.ssh_key],
        ..Default::default()
    });
    let server_handler = ServerHandler::new(
        Vec::new(),
        SecurityConfig {
            host_key_policy: crate::config::HostKeyPolicy::Tofu,
        },
        server_state.clone(),
        crate::server::transfer::ConnectionShellState::new(),
    );
    let server_task = tokio::spawn(async move {
        server::run_stream(server_config, server_stream, server_handler).await
    });

    let client_config = Arc::new(client::Config::default());
    let last_disconnect = Arc::new(StdMutex::new(None));
    let client_handler = ClientHandler::new(
        "test-node".to_string(),
        Some(wrong_server_identity.ssh_key.public_key().clone()),
        last_disconnect,
        SecurityConfig {
            host_key_policy: crate::config::HostKeyPolicy::Tofu,
        },
        client_state.clone(),
    );

    let result = client::connect_stream(client_config, client_stream, client_handler).await;
    match result {
        Ok(_) => panic!("expected server key mismatch"),
        Err(IroshError::ServerKeyMismatch { .. }) => {}
        Err(other) => panic!("unexpected error: {other}"),
    }

    let _ = server_task.await.unwrap();
}

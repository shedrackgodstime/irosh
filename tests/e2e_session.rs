//! End-to-end session lifecycle: connect, metadata, shell exit, idle reaping.

mod common;

use common::{expect_shell_closed, expect_shell_output, init_tracing, temp_state};
use iroh::RelayMode;
use irosh::config::HostKeyPolicy;
use irosh::{Client, ClientOptions, SecurityConfig, Server, ServerOptions};
use std::time::Duration;
use tokio::fs;

#[tokio::test]
async fn test_e2e_p2p_connection_and_metadata() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(60), async {
        let server_state = temp_state("server");
        let client_state = temp_state("client");

        // 1. Start Server
        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled, None);

        let (ready, server) = Server::bind(server_opts)
            .await
            .expect("Failed to bind server");
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();

        let server_handle = tokio::spawn(async move { server.run().await });

        // 2. Connect Client
        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled);

        // Give the server a moment to be "online" in the Iroh network
        tokio::time::sleep(Duration::from_millis(500)).await;

        let session = Client::connect(&client_opts, ticket)
            .await
            .expect("Failed to connect client");

        // 3. Verify Metadata
        let metadata = session.remote_metadata();
        assert!(
            metadata.is_some(),
            "Metadata should be retrieved automatically"
        );

        // 4. Cleanup
        session.close().await.expect("Failed to close session");
        shutdown.close().await;
        let _ = server_handle.await;

        // Cleanup filesystem
        let _ = fs::remove_dir_all(server_state.root()).await;
        let _ = fs::remove_dir_all(client_state.root()).await;
    })
    .await
    .expect("Test timed out");
}

#[tokio::test]
async fn test_clean_shell_exit_releases_transport_resources() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(90), async {
        let server_state = temp_state("server-exit");
        let client_state = temp_state("client-exit");

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled, None);

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(500)).await;

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled);
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();
        session.start_shell().await.unwrap();

        // Sanity: the channel works before the shell exits.
        session.send(b"echo exit-probe\r\n").await.unwrap();
        expect_shell_output(&mut session, "exit-probe").await;

        // Ask the remote shell to exit, then kill the remote transport so the
        // client stream ends (the "Session::next_event returns None" path that
        // marks the session Closed).
        let _ = session.send(b"exit\r\n").await;
        shutdown.close().await;
        expect_shell_closed(&mut session).await;

        // A clean remote end must still tear down the iroh transport. Before
        // the fix, `disconnect()` early-returned on the terminal state and left
        // the endpoint open until drop, which made iroh log the spurious
        // "Endpoint dropped without calling `Endpoint::close`" error.
        session.disconnect().await.unwrap();
        assert!(
            session.transport_resources_released(),
            "a clean remote close must release all iroh transport resources"
        );

        let _ = session.close().await;
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(server_state.root()).await;
        let _ = fs::remove_dir_all(client_state.root()).await;
    })
    .await
    .expect("Test timed out");
}

#[tokio::test]
async fn test_idle_timeout_closes_quiet_shell() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(90), async {
        let server_state = temp_state("server-idle");
        let client_state = temp_state("client-idle");

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled, None)
            .idle_timeout(Duration::from_secs(2));

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(500)).await;
        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled);
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();
        session.start_shell().await.expect("Failed to start shell");

        // Sanity: the channel works before going quiet.
        session.send(b"echo idle-probe\r\n").await.unwrap();
        expect_shell_output(&mut session, "idle-probe").await;

        // Go quiet for several times the timeout, then expect the reap.
        tokio::time::sleep(Duration::from_secs(8)).await;
        expect_shell_closed(&mut session).await;

        let _ = session.close().await;
        shutdown.close().await;
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(server_state.root()).await;
        let _ = fs::remove_dir_all(client_state.root()).await;
    })
    .await
    .expect("Test timed out");
}

#[tokio::test]
async fn test_idle_timeout_resets_on_traffic() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(90), async {
        let server_state = temp_state("server-idle-reset");
        let client_state = temp_state("client-idle-reset");

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled, None)
            .idle_timeout(Duration::from_secs(2));

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(500)).await;
        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled);
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();
        session.start_shell().await.expect("Failed to start shell");

        // Each command lands inside the 2s window while the total active
        // period exceeds it, so observing both outputs proves the timer
        // resets on traffic.
        session.send(b"echo tick-one\r\n").await.unwrap();
        expect_shell_output(&mut session, "tick-one").await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        session.send(b"echo tick-two\r\n").await.unwrap();
        expect_shell_output(&mut session, "tick-two").await;

        // Now go quiet: the reaper must still fire afterwards.
        tokio::time::sleep(Duration::from_secs(8)).await;
        expect_shell_closed(&mut session).await;

        let _ = session.close().await;
        shutdown.close().await;
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(server_state.root()).await;
        let _ = fs::remove_dir_all(client_state.root()).await;
    })
    .await
    .expect("Test timed out");
}

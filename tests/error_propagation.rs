use iroh::RelayMode;
use irosh::config::HostKeyPolicy;
use irosh::error::{ClientError, IroshError};
use irosh::transport::transfer::TransferFailureCode;
use irosh::{Client, ClientOptions, SecurityConfig, Server, ServerOptions, StateConfig};
use std::time::Duration;
use tokio::fs;

fn temp_state(name: &str) -> StateConfig {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "irosh-integ-err-{}-{}",
        name,
        rand::random::<u32>()
    ));
    StateConfig::new(path)
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("irosh=debug,info")
        .with_test_writer()
        .try_init();
}

#[tokio::test]
async fn test_transfer_not_found_error() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(60), async {
        let server_state = temp_state("server");
        let client_state = temp_state("client");

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled);

        tokio::time::sleep(Duration::from_millis(1000)).await;
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();

        // Try to download a non-existent file
        let remote_path = server_state.root().join("non-existent-file");
        let err = session
            .download(remote_path.display().to_string(), "local-target", false)
            .await
            .unwrap_err();

        match err {
            IroshError::Client(ClientError::TransferRejected { failure }) => {
                assert_eq!(failure.code, TransferFailureCode::NotFound);
            }
            _ => panic!(
                "Expected TransferRejected with NotFound code, got: {:?}",
                err
            ),
        }

        session.close().await.unwrap();
        shutdown.close().await;
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(server_state.root()).await;
        let _ = fs::remove_dir_all(client_state.root()).await;
    })
    .await
    .expect("Test timed out");
}

#[tokio::test]
async fn test_transfer_is_directory_error() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(60), async {
        let server_state = temp_state("server-dir");
        let client_state = temp_state("client-dir");

        // Create a directory on the server
        let server_dir = server_state.root().join("test-dir");
        fs::create_dir_all(&server_dir).await.unwrap();

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled);

        tokio::time::sleep(Duration::from_millis(1000)).await;
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();

        // Try to download a directory without recursive flag
        let remote_path = server_dir.display().to_string();
        let err = session
            .download(&remote_path, "local-target", false)
            .await
            .unwrap_err();

        match err {
            // The client-side pre-check (is_remote_dir) might catch this first
            IroshError::Client(ClientError::TransferTargetInvalid { reason })
                if reason.contains("remote is a directory") => {}
            // Or the server might catch it during the actual transfer if the pre-check is bypassed
            IroshError::Client(ClientError::TransferRejected { failure })
                if failure.code == TransferFailureCode::IsDirectory => {}
            _ => panic!("Expected IsDirectory failure, got: {:?}", err),
        }

        session.close().await.unwrap();
        shutdown.close().await;
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(server_state.root()).await;
        let _ = fs::remove_dir_all(client_state.root()).await;
    })
    .await
    .expect("Test timed out");
}

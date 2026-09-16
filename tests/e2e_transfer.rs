//! End-to-end file transfer: stateless, shell-backed, completion, blob round-trip.

mod common;

use common::{init_tracing, temp_state};
use iroh::RelayMode;
use irosh::config::HostKeyPolicy;
use irosh::{Client, ClientOptions, SecurityConfig, Server, ServerOptions};
use std::time::Duration;
use tokio::fs;

#[tokio::test]
async fn test_e2e_file_transfer() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(60), async {
        let server_state = temp_state("server-fs");
        let client_state = temp_state("client-fs");

        println!("[DEBUG] Preparing local file...");
        let local_dir = client_state.root().join("files");
        fs::create_dir_all(&local_dir).await.unwrap();
        let local_file = local_dir.join("hello.txt");
        fs::write(&local_file, b"hello irosh").await.unwrap();

        println!("[DEBUG] Binding server...");
        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled, None);

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        println!("[DEBUG] Waiting for server to be online...");
        tokio::time::sleep(Duration::from_millis(1000)).await;

        println!("[DEBUG] Connecting client...");
        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled);
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();

        println!("[DEBUG] Starting shell...");
        session.start_shell().await.expect("Failed to start shell");

        // Give shell a moment to spawn and set PID
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("[DEBUG] Uploading file...");
        let remote_path = server_state.root().join("uploaded.txt");
        session
            .upload_file(&local_file, &remote_path)
            .await
            .expect("File upload failed");

        println!("[DEBUG] Downloading file...");
        let downloaded_file = local_dir.join("downloaded.txt");
        session
            .download_file(&remote_path, &downloaded_file)
            .await
            .expect("File download failed");

        println!("[DEBUG] Verifying content...");
        let content = fs::read_to_string(&downloaded_file).await.unwrap();
        assert_eq!(content, "hello irosh");

        println!("[DEBUG] Closing session...");
        session.close().await.unwrap();

        println!("[DEBUG] Shutting down server...");
        shutdown.close().await;

        println!("[DEBUG] Awaiting server task...");
        let _ = server_handle.await;

        println!("[DEBUG] Cleaning up filesystem...");
        let _ = fs::remove_dir_all(server_state.root()).await;
        let _ = fs::remove_dir_all(client_state.root()).await;
        println!("[DEBUG] Integration test finished successfully. EXITING NOW.");
    })
    .await
    .expect("Test timed out");
}

#[tokio::test]
async fn test_stateless_file_transfer() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(60), async {
        let server_state = temp_state("server-stateless");
        let client_state = temp_state("client-stateless");

        println!("[DEBUG] Preparing local file...");
        let local_dir = client_state.root().join("files");
        fs::create_dir_all(&local_dir).await.unwrap();
        let local_file = local_dir.join("hello_stateless.txt");
        fs::write(&local_file, b"hello irosh stateless")
            .await
            .unwrap();

        println!("[DEBUG] Binding server...");
        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled, None);

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        println!("[DEBUG] Waiting for server to be online...");
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("[DEBUG] Connecting client...");
        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled);
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();

        // NO shell start here! Testing stateless transfer.

        println!("[DEBUG] Uploading file (stateless)...");
        let remote_path = server_state.root().join("uploaded_stateless.txt");
        session
            .upload_file(&local_file, &remote_path)
            .await
            .expect("Stateless file upload failed");

        println!("[DEBUG] Downloading file (stateless)...");
        let downloaded_file = local_dir.join("downloaded_stateless.txt");
        session
            .download_file(&remote_path, &downloaded_file)
            .await
            .expect("Stateless file download failed");

        println!("[DEBUG] Verifying content...");
        let content = fs::read_to_string(&downloaded_file).await.unwrap();
        assert_eq!(content, "hello irosh stateless");

        println!("[DEBUG] Closing session...");
        session.close().await.unwrap();

        println!("[DEBUG] Shutting down server...");
        shutdown.close().await;
        let _ = server_handle.await;

        println!("[DEBUG] Cleaning up filesystem...");
        let _ = fs::remove_dir_all(server_state.root()).await;
        let _ = fs::remove_dir_all(client_state.root()).await;
        println!("[DEBUG] Stateless integration test finished successfully. EXITING NOW.");
    })
    .await
    .expect("Test timed out");
}

#[tokio::test]
async fn test_empty_file_transfer() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(60), async {
        let server_state = temp_state("server-empty");
        let client_state = temp_state("client-empty");

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
        tokio::time::sleep(Duration::from_millis(500)).await;

        let local_file = client_state.root().join("empty.txt");
        fs::write(&local_file, b"").await.unwrap();

        let remote_path = server_state.root().join("empty_uploaded.txt");
        session
            .upload_file(&local_file, &remote_path)
            .await
            .expect("Empty file upload failed");

        let downloaded_file = client_state.root().join("empty_downloaded.txt");
        session
            .download_file(&remote_path, &downloaded_file)
            .await
            .expect("Empty file download failed");

        let content = fs::read_to_string(&downloaded_file).await.unwrap();
        assert_eq!(content, "", "empty file content mismatch");

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
async fn test_remote_exists() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(60), async {
        let server_state = temp_state("server-exists");
        let client_state = temp_state("client-exists");

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
        let session = Client::connect(&client_opts, ticket).await.unwrap();

        let existing = server_state.root().join("i_exist.txt");
        fs::write(&existing, b"present").await.unwrap();

        let exists = session
            .remote_exists(&existing)
            .await
            .expect("remote_exists call failed");
        assert!(exists, "existing file should report exists");

        let missing = server_state.root().join("i_do_not_exist.txt");
        let not_found = session
            .remote_exists(&missing)
            .await
            .expect("remote_exists call failed");
        assert!(!not_found, "missing file should report not found");

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
async fn test_upload_nonexistent_source() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(60), async {
        let server_state = temp_state("server-no-source");
        let client_state = temp_state("client-no-source");

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
        tokio::time::sleep(Duration::from_millis(500)).await;

        let nonexistent = client_state.root().join("does_not_exist.txt");
        let remote_path = server_state.root().join("should_not_appear.txt");
        let result = session.upload_file(&nonexistent, &remote_path).await;
        assert!(result.is_err(), "uploading nonexistent source should fail");

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
async fn test_completion_request() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(60), async {
        let server_state = temp_state("server-completion");
        let client_state = temp_state("client-completion");

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
        tokio::time::sleep(Duration::from_millis(500)).await;

        let local_file = client_state.root().join("completion_test.txt");
        fs::write(&local_file, b"completion check").await.unwrap();

        let remote_path = server_state.root().join("completion_remote.txt");
        session
            .upload_file(&local_file, &remote_path)
            .await
            .expect("Upload for completion test failed");

        let remote_str = remote_path.display().to_string();
        let matches = session
            .remote_completion(&remote_str)
            .await
            .expect("Completion request failed");
        assert!(
            !matches.is_empty(),
            "completion should return at least one path"
        );
        assert!(
            matches.iter().any(|m| m.contains("completion_remote.txt")),
            "completion result should contain the uploaded file path: {:?}",
            matches
        );

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
async fn test_large_file_transfer() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(120), async {
        let server_state = temp_state("server-large");
        let client_state = temp_state("client-large");

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(1000)).await;

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled);
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();
        session.start_shell().await.unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;

        let local_file = client_state.root().join("large.bin");
        let content = vec![0xABu8; 5 * 1024 * 1024]; // 5MB, exercises ~80 chunks
        fs::write(&local_file, &content).await.unwrap();

        let remote_path = server_state.root().join("large_uploaded.bin");
        session
            .upload_file(&local_file, &remote_path)
            .await
            .expect("Large file upload failed");

        let downloaded_file = client_state.root().join("large_downloaded.bin");
        session
            .download_file(&remote_path, &downloaded_file)
            .await
            .expect("Large file download failed");

        let downloaded_content = fs::read(&downloaded_file).await.unwrap();
        assert_eq!(downloaded_content.len(), content.len(), "size mismatch");
        assert_eq!(downloaded_content, content, "content mismatch");

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
async fn test_blob_put_get_roundtrip() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(120), async {
        let server_state = temp_state("server-blob");
        let client_state = temp_state("client-blob");

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(1000)).await;

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled);
        let session = Client::connect(&client_opts, ticket).await.unwrap();

        // 1. Upload a file via blob protocol
        let local_file = client_state.root().join("blob_source.txt");
        fs::write(&local_file, b"hello irosh blob world")
            .await
            .unwrap();
        let remote_path = server_state.root().join("blob_uploaded.txt");

        let hash = session
            .upload_blob(&local_file, &remote_path, |_| {})
            .await
            .expect("Blob upload failed");

        // 2. Download the same file via blob protocol
        let downloaded_file = client_state.root().join("blob_downloaded.txt");
        let downloaded_hash = session
            .download_blob(&remote_path, &downloaded_file, |_| {})
            .await
            .expect("Blob download failed");

        assert_eq!(hash, downloaded_hash, "content hash should match");
        let content = fs::read_to_string(&downloaded_file).await.unwrap();
        assert_eq!(content, "hello irosh blob world");

        session.close().await.unwrap();
        shutdown.close().await;
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(server_state.root()).await;
        let _ = fs::remove_dir_all(client_state.root()).await;
    })
    .await
    .expect("Test timed out");
}

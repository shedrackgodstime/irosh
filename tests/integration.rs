use irosh::config::HostKeyPolicy;
use irosh::{Client, ClientOptions, SecurityConfig, Server, ServerOptions, StateConfig};
use std::time::Duration;
use tokio::fs;

/// Helper to create a temporary state directory for tests.
fn temp_state(name: &str) -> StateConfig {
    let mut path = std::env::temp_dir();
    path.push(format!("irosh-integ-{}-{}", name, rand::random::<u32>()));
    StateConfig::new(path)
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("irosh=debug,info")
        .with_test_writer()
        .try_init();
}

#[tokio::test]
async fn test_e2e_p2p_connection_and_metadata() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(30), async {
        let server_state = temp_state("server");
        let client_state = temp_state("client");

        // 1. Start Server
        let server_opts = ServerOptions::new(server_state.clone()).security(SecurityConfig {
            host_key_policy: HostKeyPolicy::AcceptAll,
        });

        let (ready, server) = Server::bind(server_opts)
            .await
            .expect("Failed to bind server");
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();

        let server_handle = tokio::spawn(async move { server.run().await });

        // 2. Connect Client
        let client_opts = ClientOptions::new(client_state.clone()).security(SecurityConfig {
            host_key_policy: HostKeyPolicy::AcceptAll,
        });

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
async fn test_e2e_file_transfer() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(30), async {
        let server_state = temp_state("server-fs");
        let client_state = temp_state("client-fs");

        println!("[DEBUG] Preparing local file...");
        let local_dir = client_state.root().join("files");
        fs::create_dir_all(&local_dir).await.unwrap();
        let local_file = local_dir.join("hello.txt");
        fs::write(&local_file, b"hello irosh").await.unwrap();

        println!("[DEBUG] Binding server...");
        let server_opts = ServerOptions::new(server_state.clone()).security(SecurityConfig {
            host_key_policy: HostKeyPolicy::AcceptAll,
        });

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        println!("[DEBUG] Waiting for server to be online...");
        tokio::time::sleep(Duration::from_millis(1000)).await;

        println!("[DEBUG] Connecting client...");
        let client_opts = ClientOptions::new(client_state.clone()).security(SecurityConfig {
            host_key_policy: HostKeyPolicy::AcceptAll,
        });
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();

        println!("[DEBUG] Starting shell...");
        session.start_shell().await.expect("Failed to start shell");

        // Give shell a moment to spawn and set PID
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("[DEBUG] Uploading file...");
        let remote_path = server_state.root().join("uploaded.txt");
        session
            .put_file(&local_file, &remote_path)
            .await
            .expect("File upload failed");

        println!("[DEBUG] Downloading file...");
        let downloaded_file = local_dir.join("downloaded.txt");
        session
            .get_file(&remote_path, &downloaded_file)
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
    tokio::time::timeout(Duration::from_secs(30), async {
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
        let server_opts = ServerOptions::new(server_state.clone()).security(SecurityConfig {
            host_key_policy: HostKeyPolicy::AcceptAll,
        });

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        println!("[DEBUG] Waiting for server to be online...");
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("[DEBUG] Connecting client...");
        let client_opts = ClientOptions::new(client_state.clone()).security(SecurityConfig {
            host_key_policy: HostKeyPolicy::AcceptAll,
        });
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();

        // NO shell start here! Testing stateless transfer.

        println!("[DEBUG] Uploading file (stateless)...");
        let remote_path = server_state.root().join("uploaded_stateless.txt");
        session
            .put_file(&local_file, &remote_path)
            .await
            .expect("Stateless file upload failed");

        println!("[DEBUG] Downloading file (stateless)...");
        let downloaded_file = local_dir.join("downloaded_stateless.txt");
        session
            .get_file(&remote_path, &downloaded_file)
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
async fn test_recursive_directory_transfer() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(45), async {
        let server_state = temp_state("server-recursive");
        let client_state = temp_state("client-recursive");

        println!("[DEBUG] Preparing local directory structure...");
        let local_root = client_state.root().join("recursive_source");
        fs::create_dir_all(local_root.join("nested/deep"))
            .await
            .unwrap();

        fs::write(local_root.join("file1.txt"), b"content 1")
            .await
            .unwrap();
        fs::write(local_root.join("file2.txt"), b"content 2")
            .await
            .unwrap();
        fs::write(local_root.join("nested/file3.txt"), b"content 3")
            .await
            .unwrap();
        fs::write(local_root.join("nested/deep/file4.txt"), b"content 4")
            .await
            .unwrap();

        println!("[DEBUG] Binding server...");
        let server_opts = ServerOptions::new(server_state.clone()).security(SecurityConfig {
            host_key_policy: HostKeyPolicy::AcceptAll,
        });

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        println!("[DEBUG] Waiting for server to be online...");
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("[DEBUG] Connecting client...");
        let client_opts = ClientOptions::new(client_state.clone()).security(SecurityConfig {
            host_key_policy: HostKeyPolicy::AcceptAll,
        });
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();

        // Remote recursive transfers currently need a shell for 'find' (for download)
        // and namespaces/cwd (for upload).
        session.start_shell().await.expect("Failed to start shell");
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("[DEBUG] Uploading directory (recursive)...");
        let remote_path = server_state.root().join("remote_dir");
        session
            .put(&local_root, &remote_path, true)
            .await
            .expect("Recursive upload failed");

        println!("[DEBUG] Downloading directory (recursive)...");
        let downloaded_root = client_state.root().join("recursive_download");
        session
            .get(&remote_path, &downloaded_root, true)
            .await
            .expect("Recursive download failed");

        println!("[DEBUG] Verifying contents...");
        assert_eq!(
            fs::read_to_string(downloaded_root.join("file1.txt"))
                .await
                .unwrap(),
            "content 1"
        );
        assert_eq!(
            fs::read_to_string(downloaded_root.join("file2.txt"))
                .await
                .unwrap(),
            "content 2"
        );
        assert_eq!(
            fs::read_to_string(downloaded_root.join("nested/file3.txt"))
                .await
                .unwrap(),
            "content 3"
        );
        assert_eq!(
            fs::read_to_string(downloaded_root.join("nested/deep/file4.txt"))
                .await
                .unwrap(),
            "content 4"
        );

        println!("[DEBUG] Closing session...");
        session.close().await.unwrap();

        println!("[DEBUG] Shutting down server...");
        shutdown.close().await;
        let _ = server_handle.await;

        println!("[DEBUG] Cleaning up filesystem...");
        let _ = fs::remove_dir_all(server_state.root()).await;
        let _ = fs::remove_dir_all(client_state.root()).await;
        println!("[DEBUG] Recursive integration test finished successfully. EXITING NOW.");
    })
    .await
    .expect("Test timed out");
}

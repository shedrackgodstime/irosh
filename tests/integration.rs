use iroh::RelayMode;
use irosh::config::HostKeyPolicy;
use irosh::{Client, ClientOptions, SecurityConfig, Server, ServerOptions, StateConfig};
use std::time::Duration;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
    tokio::time::timeout(Duration::from_secs(60), async {
        let server_state = temp_state("server");
        let client_state = temp_state("client");

        // 1. Start Server
        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);

        let (ready, server) = Server::bind(server_opts)
            .await
            .expect("Failed to bind server");
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();

        let server_handle = tokio::spawn(async move { server.run().await });

        // 2. Connect Client
        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
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
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        println!("[DEBUG] Waiting for server to be online...");
        tokio::time::sleep(Duration::from_millis(1000)).await;

        println!("[DEBUG] Connecting client...");
        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
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
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        println!("[DEBUG] Waiting for server to be online...");
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("[DEBUG] Connecting client...");
        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
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
async fn test_recursive_directory_transfer() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(300), async {
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
        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);

        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        println!("[DEBUG] Waiting for server to be online...");
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("[DEBUG] Connecting client...");
        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled);
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();

        // Remote recursive transfers currently need a shell for 'find' (for download)
        // and namespaces/cwd (for upload).
        session.start_shell().await.expect("Failed to start shell");
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("[DEBUG] Uploading directory (recursive)...");
        let remote_path = server_state.root().join("remote_dir");
        session
            .upload(&local_root, &remote_path, true)
            .await
            .expect("Recursive upload failed");

        println!("[DEBUG] Downloading directory (recursive)...");
        let downloaded_root = client_state.root().join("recursive_download");
        session
            .download(&remote_path, &downloaded_root, true)
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

#[tokio::test]
async fn test_port_forwarding() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(60), async {
        let server_state = temp_state("server-tunnel");
        let client_state = temp_state("client-tunnel");

        // 1. Start an echo server on the server side to be our tunnel target
        let echo_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_addr = echo_listener.local_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((stream, _)) = echo_listener.accept().await {
                let (mut reader, mut writer) = tokio::io::split(stream);
                let _ = tokio::io::copy(&mut reader, &mut writer).await;
            }
        });

        // 2. Start Irosh Server
        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(500)).await;

        // 3. Connect Irosh Client
        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled);
        let session = Client::connect(&client_opts, ticket).await.unwrap();

        // 4. Setup Local Forwarding:
        // Local (random port) -> Remote Echo Server
        let (_, bound_addr) = session
            .local_forward(
                "127.0.0.1:0",
                echo_addr.ip().to_string(),
                echo_addr.port() as u32,
            )
            .await
            .unwrap();

        // 5. Test the tunnel
        let mut tunnel_stream = tokio::net::TcpStream::connect(bound_addr).await.unwrap();
        let msg = b"hello tunnel";
        tunnel_stream.write_all(msg).await.unwrap();

        let mut response = vec![0u8; msg.len()];
        tunnel_stream.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, msg);

        // 6. Cleanup
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
async fn test_empty_file_transfer() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(60), async {
        let server_state = temp_state("server-empty");
        let client_state = temp_state("client-empty");

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(500)).await;

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
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
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(500)).await;

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
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
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(500)).await;

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
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
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(500)).await;

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
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
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(1000)).await;

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
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
async fn test_concurrent_transfers() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(120), async {
        let server_state = temp_state("server-concurrent");
        let client_state_a = temp_state("client-conc-a");
        let client_state_b = temp_state("client-conc-b");

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Use separate client states so each session has a distinct identity
        let opts_a = ClientOptions::new(client_state_a.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled);
        let opts_b = ClientOptions::new(client_state_b.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled);

        // Connect sessions sequentially (each generates its own key)
        let mut session_a = Client::connect(&opts_a, ticket.clone()).await.unwrap();
        let mut session_b = Client::connect(&opts_b, ticket).await.unwrap();

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Upload both files concurrently on separate sessions
        let file_a = client_state_a.root().join("conc_a.txt");
        fs::write(&file_a, b"file A content").await.unwrap();
        let remote_a = server_state.root().join("conc_a.txt");

        let file_b = client_state_b.root().join("conc_b.txt");
        fs::write(&file_b, b"file B content").await.unwrap();
        let remote_b = server_state.root().join("conc_b.txt");

        let (r1, r2) = tokio::join!(
            session_a.upload_file(&file_a, &remote_a),
            session_b.upload_file(&file_b, &remote_b),
        );
        r1.expect("Concurrent upload A failed");
        r2.expect("Concurrent upload B failed");

        // Download both concurrently
        let dl_a = client_state_a.root().join("conc_dl_a.txt");
        let dl_b = client_state_b.root().join("conc_dl_b.txt");
        let (r1, r2) = tokio::join!(
            session_a.download_file(&remote_a, &dl_a),
            session_b.download_file(&remote_b, &dl_b),
        );
        r1.expect("Concurrent download A failed");
        r2.expect("Concurrent download B failed");

        assert_eq!(fs::read_to_string(&dl_a).await.unwrap(), "file A content");
        assert_eq!(fs::read_to_string(&dl_b).await.unwrap(), "file B content");

        session_a.close().await.unwrap();
        session_b.close().await.unwrap();
        shutdown.close().await;
        let _ = server_handle.await;
        let _ = fs::remove_dir_all(server_state.root()).await;
        let _ = fs::remove_dir_all(client_state_a.root()).await;
        let _ = fs::remove_dir_all(client_state_b.root()).await;
    })
    .await
    .expect("Test timed out");
}

#[tokio::test]
async fn test_transfer_cancellation() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(120), async {
        let server_state = temp_state("server-cancel");
        let client_state = temp_state("client-cancel");

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(1000)).await;

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled);
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();

        // Create a large file that will take time to transfer
        let large_file = client_state.root().join("cancel_upload.bin");
        let content = vec![0xCDu8; 200 * 1024 * 1024]; // 200 MB
        fs::write(&large_file, &content).await.unwrap();
        let remote_path = server_state.root().join("cancel_upload.bin");

        // Cancel the upload mid-transfer using select!
        let upload = session.upload_file(&large_file, &remote_path);
        let cancel_delay = tokio::time::sleep(Duration::from_millis(200));
        tokio::select! {
            _ = cancel_delay => {
                // Upload cancelled — the future was dropped and the transfer stream closed
            }
            r = upload => {
                panic!("Upload completed before it could be cancelled: {:?}", r);
            }
        }

        // Give the server a moment to clean up the aborted transfer
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the server is still operational
        let verify_file = client_state.root().join("verify_after_cancel.txt");
        fs::write(&verify_file, b"server still works")
            .await
            .unwrap();
        let verify_remote = server_state.root().join("verify_after_cancel.txt");
        session
            .upload_file(&verify_file, &verify_remote)
            .await
            .expect("Upload after cancellation failed");

        let verify_dl = client_state.root().join("verify_dl_after_cancel.txt");
        session
            .download_file(&verify_remote, &verify_dl)
            .await
            .expect("Download after cancellation failed");
        assert_eq!(
            fs::read_to_string(&verify_dl).await.unwrap(),
            "server still works"
        );

        // The cancelled file should NOT exist (partial file cleaned up)
        let cancelled_exists = tokio::fs::metadata(&remote_path).await.is_ok();
        assert!(
            !cancelled_exists,
            "Cancelled upload partial file should have been cleaned up"
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
async fn test_blob_put_get_roundtrip() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(120), async {
        let server_state = temp_state("server-blob");
        let client_state = temp_state("client-blob");

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(1000)).await;

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
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

#[tokio::test]
async fn test_blob_dir_upload() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(120), async {
        let server_state = temp_state("server-blob-dir");
        let client_state = temp_state("client-blob-dir");

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(1000)).await;

        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig {
                host_key_policy: HostKeyPolicy::AcceptAll,
            })
            .relay_mode(RelayMode::Disabled);
        let session = Client::connect(&client_opts, ticket).await.unwrap();

        // Upload a directory via blob protocol
        let local_dir = client_state.root().join("blob_dir_source");
        fs::create_dir_all(local_dir.join("nested")).await.unwrap();
        fs::write(local_dir.join("file_a.txt"), b"alpha")
            .await
            .unwrap();
        fs::write(local_dir.join("file_b.txt"), b"beta")
            .await
            .unwrap();
        fs::write(local_dir.join("nested/file_c.txt"), b"gamma")
            .await
            .unwrap();

        let remote_dir = server_state.root().join("blob_dir_dest");
        let dir_hash = session
            .upload_blob(&local_dir, &remote_dir, |_| {})
            .await
            .expect("Blob directory upload failed");

        // Download the directory via blob protocol
        let downloaded_dir = client_state.root().join("blob_dir_downloaded");
        let downloaded_dir_hash = session
            .download_blob(&remote_dir, &downloaded_dir, |_| {})
            .await
            .expect("Blob directory download failed");

        assert_eq!(
            dir_hash, downloaded_dir_hash,
            "directory content hash should match"
        );
        assert_eq!(
            fs::read_to_string(downloaded_dir.join("file_a.txt"))
                .await
                .unwrap(),
            "alpha"
        );
        assert_eq!(
            fs::read_to_string(downloaded_dir.join("file_b.txt"))
                .await
                .unwrap(),
            "beta"
        );
        assert_eq!(
            fs::read_to_string(downloaded_dir.join("nested/file_c.txt"))
                .await
                .unwrap(),
            "gamma"
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
async fn test_wormhole_rendezvous() {
    let _ = tracing_subscriber::fmt::try_init();
    let server_state = temp_state("wormhole-server");
    let client_state = temp_state("wormhole-client");
    let code = "crystal-piano-7";

    // 1. Start Server
    let server_opts =
        ServerOptions::new(server_state.clone()).relay_mode(RelayMode::Disabled, None);
    let (_ready, server) = Server::bind(server_opts).await.unwrap();
    let shutdown_handle = server.shutdown_handle();
    let control_tx = server.control_handle();

    let server_task = tokio::spawn(async move {
        server.run().await.unwrap();
    });

    // 2. Enable Wormhole on Server
    let (tx, _) = tokio::sync::oneshot::channel();
    control_tx
        .send(irosh::InternalCommand::EnableWormhole {
            code: code.to_string(),
            password: None,
            persistent: false,
            tx,
        })
        .await
        .unwrap();

    // 3. Connect Client using the code (retry for network flakiness)
    let client_opts = ClientOptions::new(client_state.clone())
        .security(SecurityConfig {
            host_key_policy: HostKeyPolicy::AcceptAll,
        })
        .relay_mode(RelayMode::Disabled);

    let session = 'retry: loop {
        for attempt in 1..=3 {
            match Client::connect(
                &client_opts,
                irosh::ResolvedTarget::WormholeCode(code.to_string()),
            )
            .await
            {
                Ok(session) => break 'retry session,
                Err(e) if attempt < 3 => {
                    tracing::warn!("Wormhole attempt {attempt} failed: {e}. Retrying...");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                Err(e) => panic!("Wormhole discovery failed after 3 attempts: {e}"),
            }
        }
    };

    // 4. Verify Connection
    assert!(session.remote_metadata().is_some());

    // 5. Cleanup
    session.close().await.unwrap();
    shutdown_handle.close().await;
    server_task.await.unwrap();
}

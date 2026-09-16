//! End-to-end recursive transfer (dirs, symlink escaping) and blob dir upload.
//!
//! Recursive transfers go through the remote `find`/namespace machinery and are
//! unix-only; blob dir uploads work on any platform.

mod common;

use common::{init_tracing, temp_state};
use iroh::RelayMode;
use irosh::config::HostKeyPolicy;
use irosh::{Client, ClientOptions, SecurityConfig, Server, ServerOptions};
use std::time::Duration;
use tokio::fs;

#[cfg(unix)]
#[tokio::test]
async fn test_recursive_download_skips_symlink_escape() {
    use std::os::unix::fs::symlink;

    init_tracing();
    tokio::time::timeout(Duration::from_secs(300), async {
        let server_state = temp_state("server-symlink");
        let client_state = temp_state("client-symlink");

        // Secret file OUTSIDE the served directory. A symlink inside the served
        // directory points at it; recursive download must NOT follow the link.
        fs::create_dir_all(server_state.root()).await.unwrap();
        let secret = server_state.root().join("secret_outside.txt");
        fs::write(&secret, b"top-secret").await.unwrap();

        println!("[DEBUG] Binding server...");
        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        println!("[DEBUG] Connecting client...");
        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled);
        let mut session = Client::connect(&client_opts, ticket).await.unwrap();
        session.start_shell().await.expect("Failed to start shell");
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Prepare the served directory: one regular file + one symlink to the secret.
        let served = server_state.root().join("served_dir");
        fs::create_dir_all(&served).await.unwrap();
        fs::write(served.join("benign.txt"), b"benign")
            .await
            .unwrap();
        symlink(&secret, served.join("leak.txt")).expect("create symlink");

        println!("[DEBUG] Downloading served directory (recursive)...");
        let downloaded_root = client_state.root().join("recursive_download");
        session
            .download(&served, &downloaded_root, true)
            .await
            .expect("Recursive download failed");

        // The benign file is present; the symlink entry itself is skipped entirely
        // (neither its name nor the secret's contents leak).
        assert_eq!(
            fs::read_to_string(downloaded_root.join("benign.txt"))
                .await
                .unwrap(),
            "benign"
        );
        assert!(
            !tokio::fs::try_exists(downloaded_root.join("leak.txt"))
                .await
                .unwrap_or(false),
            "symlink entry must not be materialized on the client"
        );

        // Belt-and-braces: the secret content must not appear anywhere in the tree.
        let secret_data = std::fs::read_to_string(&secret).unwrap();
        let mut walker = walkdir::WalkDir::new(&downloaded_root).into_iter();
        while let Some(entry) = walker.next() {
            let entry = entry.unwrap();
            if entry.file_type().is_file() {
                let content = std::fs::read_to_string(entry.path()).unwrap_or_default();
                assert!(
                    content != secret_data,
                    "secret content leaked via {:?}",
                    entry.path()
                );
            }
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

#[cfg(unix)]
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
async fn test_blob_dir_upload() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(120), async {
        let server_state = temp_state("server-blob-dir");
        let client_state = temp_state("client-blob-dir");

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

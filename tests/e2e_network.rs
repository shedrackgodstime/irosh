//! End-to-end network behaviors: port forwarding, concurrent sessions,
//! cancellation resilience, and the (disabled-by-default) wormhole rendezvous.

mod common;

use common::{init_tracing, temp_state};
use iroh::RelayMode;
use irosh::config::HostKeyPolicy;
use irosh::{Client, ClientOptions, SecurityConfig, Server, ServerOptions};
use std::time::Duration;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(500)).await;

        // 3. Connect Irosh Client
        let client_opts = ClientOptions::new(client_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
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
async fn test_concurrent_transfers() {
    init_tracing();
    tokio::time::timeout(Duration::from_secs(120), async {
        let server_state = temp_state("server-concurrent");
        let client_state_a = temp_state("client-conc-a");
        let client_state_b = temp_state("client-conc-b");

        let server_opts = ServerOptions::new(server_state.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled, None);
        let (ready, server) = Server::bind(server_opts).await.unwrap();
        let ticket = ready.ticket().clone();
        let shutdown = server.shutdown_handle();
        let server_handle = tokio::spawn(async move { server.run().await });

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Use separate client states so each session has a distinct identity
        let opts_a = ClientOptions::new(client_state_a.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled);
        let opts_b = ClientOptions::new(client_state_b.clone())
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
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
#[ignore = "requires reachability of the iroh derp relay (derp1.iroh.network); run with `-- --ignored`"]
async fn test_wormhole_rendezvous() {
    let test = async {
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
            .security(SecurityConfig::new(HostKeyPolicy::AcceptAll))
            .relay_mode(RelayMode::Disabled);

        let session = 'retry: loop {
            for attempt in 1..=3 {
                match Client::connect(
                    &client_opts,
                    irosh::ResolvedTarget::WormholeCode(
                        irosh::WormholeCode::new(code.to_string()).unwrap(),
                    ),
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
    };

    // The rendezvous path depends on the public iroh relay (pkarr + DERP), so
    // bound it explicitly to avoid multi-minute hangs when the relay is
    // unreachable (e.g. networks without outbound access).
    tokio::time::timeout(Duration::from_secs(180), test)
        .await
        .expect("test_wormhole_rendezvous timed out");
}

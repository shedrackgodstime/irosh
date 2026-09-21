//! Handler-level regression tests for transfer rejection paths (A4, A5).
//!
//! These drive [`super::handle_transfer_stream`] over a real in-process iroh
//! endpoint pair so the server handlers observe genuine [`IrohDuplex`]
//! streams: stateless destination staging, chunk accounting and error frames
//! all exercise the production code paths.
//!
//! Stateless mode (no live shell PID) resolves absolute paths directly and
//! stages uploads with plain [`tokio::fs`] file writes, so these tests run
//! cross-platform. Only dangling-symlink creation is Unix-gated, matching the
//! repository's existing symlink gating.

use std::time::Duration;

use iroh::{Endpoint, RelayMode, SecretKey};
use tokio::time::timeout;

use crate::auth::temp_state;
use crate::error::Result;
use crate::metrics::Metrics;
use crate::transport::iroh::derive_alpn;
use crate::transport::stream::IrohDuplex;
use crate::transport::transfer::{
    BlobPutRequest, EntryComplete, EntryHeader, PutRequest, TransferComplete, TransferFailureCode,
    TransferFrame, read_next_frame, read_put_ready, read_transfer_error, write_blob_put_request,
    write_entry_complete, write_new_entry, write_put_chunk, write_put_complete, write_put_request,
};

use super::{ConnectionShellState, handle_transfer_stream};

/// Client-side drive handle plus the spawned server task.
///
/// The endpoints are kept alive for the whole test: connection health depends
/// on the endpoint driver task, which must outlive the streams.
struct TransferHarness {
    /// Client half of the transfer bi-stream.
    client: IrohDuplex,
    /// Server-side [`handle_transfer_stream`] task.
    server_task: tokio::task::JoinHandle<Result<()>>,
    /// Kept alive so the in-process connection stays up.
    _server_endpoint: Endpoint,
    /// Kept alive so the in-process connection stays up.
    _client_endpoint: Endpoint,
    /// Keeps the server-side accept loop alive for the whole test.
    _router: iroh::protocol::Router,
    /// Holds a server-side connection clone until the test ends.
    ///
    /// Mirrors production, where `spawn_side_stream_listener` keeps a clone
    /// for the session lifetime: without this, the server task ending would
    /// drop the last handle and gracefully close the connection, racing the
    /// delivery of the terminal error frame the test is about to read.
    _server_conn: iroh::endpoint::Connection,
}

/// Binds a bare in-process endpoint.
///
/// Unlike the production bind helpers this skips the `online()` wait, which
/// can stall for the full timeout with relays disabled and only slows tests.
async fn bind_test_endpoint(alpn: Vec<u8>) -> Endpoint {
    Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(SecretKey::generate())
        .alpns(vec![alpn])
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .expect("failed to bind test endpoint")
}

/// Protocol handler that captures the server-side connection and hands it to
/// the test through a oneshot channel, then parks until the router shuts
/// down. Parking (instead of returning immediately) mirrors long-lived
/// production handlers and keeps the router's per-connection task alive for
/// the whole test.
#[derive(Debug)]
struct CaptureConnection {
    tx: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<iroh::endpoint::Connection>>>,
}

impl iroh::protocol::ProtocolHandler for CaptureConnection {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> std::result::Result<(), iroh::protocol::AcceptError> {
        if let Some(tx) = self
            .tx
            .lock()
            .expect("test connection mutex poisoned")
            .take()
        {
            let _ = tx.send(connection);
        }
        std::future::pending::<()>().await;
        Ok(())
    }
}

/// Opens a fresh in-process endpoint pair and spawns `handle_transfer_stream`
/// on the server half with a stateless shell state rooted at a temp dir.
///
/// The server side goes through a [`iroh::protocol::Router`] exactly like the
/// production server: the router routes the test ALPN to a capture handler
/// and the test receives the server-side connection from it. This mirrors
/// the `Server::bind` + `dial_p2p` flow proven by the e2e suite instead of
/// hand-rolling endpoint acceptance.
async fn spawn_transfer_harness(name: &str) -> TransferHarness {
    let state = temp_state(name);
    let blobs = iroh_blobs::store::fs::FsStore::load(state.blobs_path())
        .await
        .expect("failed to load test blobs store");
    let shell_state = ConnectionShellState::new(state.root().to_path_buf(), blobs);

    let alpn = derive_alpn(None);
    let server_ep = bind_test_endpoint(alpn.clone()).await;
    let client_ep = bind_test_endpoint(alpn.clone()).await;

    let (conn_tx, conn_rx) = tokio::sync::oneshot::channel();
    let router = iroh::protocol::Router::builder(server_ep.clone())
        .accept(
            alpn.clone(),
            CaptureConnection {
                tx: std::sync::Mutex::new(Some(conn_tx)),
            },
        )
        .spawn();

    // Use the server's advertised address as-is, exactly like the e2e
    // ticket flow (`Server::bind` + `dial_p2p`), which is proven to work
    // in-process on all CI platforms.
    let server_addr = server_ep.addr();

    // Retry a few times like `dial_p2p`: a freshly bound endpoint can drop
    // the first handshake flight.
    let mut attempt = 0;
    let client_conn = loop {
        attempt += 1;
        let attempt_res = tokio::time::timeout(
            Duration::from_secs(20),
            client_ep.connect(server_addr.clone(), &alpn),
        )
        .await;
        match attempt_res {
            Ok(Ok(conn)) => {
                break conn;
            }
            other => {
                if attempt >= 3 {
                    panic!("test client failed to connect after {attempt} attempts: {other:?}");
                }
                tracing::debug!("test connect attempt {attempt}/3 failed");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    };
    let server_conn = conn_rx
        .await
        .expect("test server never routed the connection");
    let (client_send, client_recv) = client_conn
        .open_bi()
        .await
        .expect("test client failed to open bi stream");
    let client = IrohDuplex::new(client_send, client_recv);

    // Accept the stream on the server side concurrently: `open_bi` is lazy
    // (no STREAM frame flows until the first write), so the server must not
    // wait for `accept_bi` before the test gets the client half to write
    // with. This mirrors production, where the client writes its request
    // frame immediately after opening while the server listener pends.
    let server_conn_for_task = server_conn.clone();
    let server_task = tokio::spawn(async move {
        let (server_send, server_recv) = server_conn_for_task
            .accept_bi()
            .await
            .expect("test server failed to accept bi stream");
        let server = IrohDuplex::new(server_send, server_recv);
        handle_transfer_stream(
            server_conn_for_task,
            server,
            shell_state,
            Metrics::default(),
        )
        .await
    });

    TransferHarness {
        client,
        server_task,
        _server_endpoint: server_ep,
        _client_endpoint: client_ep,
        _router: router,
        _server_conn: server_conn,
    }
}

/// A4: a non-recursive upload whose `PutComplete` byte count disagrees with
/// the declared `request.size` must be rejected with `SizeMismatch` and must
/// leave no final file behind.
#[tokio::test]
async fn put_complete_size_mismatch_is_rejected() {
    timeout(Duration::from_secs(60), async {
        let state = temp_state("transfer-put-mismatch");
        let target_path = state.root().join("uploaded-mismatch.bin");
        let harness = spawn_transfer_harness("xfer-put-mismatch").await;
        let mut client = harness.client;

        write_put_request(
            &mut client,
            &PutRequest::new(target_path.display().to_string(), 100, None, false),
        )
        .await
        .expect("failed to write test PutRequest");
        let ready = read_put_ready(&mut client)
            .await
            .expect("failed to read test PutReady");
        assert_eq!(ready.size, 100);

        write_put_chunk(&mut client, b"short")
            .await
            .expect("failed to write test PutChunk");
        write_put_complete(&mut client, &TransferComplete { size: 5 })
            .await
            .expect("failed to write test PutComplete");

        let failure = read_transfer_error(&mut client)
            .await
            .expect("expected a transfer error for the size mismatch");
        assert_eq!(
            failure.code,
            TransferFailureCode::SizeMismatch,
            "declared 100 bytes but sent 5: {failure:?}"
        );

        assert!(
            !target_path.exists(),
            "size-mismatched upload must not leave a final file"
        );

        harness
            .server_task
            .await
            .expect("server task panicked")
            .expect("server handler failed");
    })
    .await
    .expect("test timed out");
}

/// A4: a recursive entry whose `EntryHeader.size` disagrees with the received
/// bytes must be rejected with `SizeMismatch` and must write no file.
#[tokio::test]
async fn recursive_entry_size_mismatch_is_rejected() {
    timeout(Duration::from_secs(60), async {
        let state = temp_state("transfer-recursive-mismatch");
        let dest_root = state.root().join("recursive-dest");
        let harness = spawn_transfer_harness("xfer-recursive-mismatch").await;
        let mut client = harness.client;

        write_put_request(
            &mut client,
            &PutRequest::new(dest_root.display().to_string(), 0, None, true),
        )
        .await
        .expect("failed to write test recursive PutRequest");
        read_put_ready(&mut client)
            .await
            .expect("failed to read test recursive PutReady");

        write_new_entry(
            &mut client,
            &EntryHeader {
                path: "sub/file.txt".to_string(),
                size: 100,
                mode: None,
                is_dir: false,
            },
        )
        .await
        .expect("failed to write test NewEntry");
        write_put_chunk(&mut client, b"xy")
            .await
            .expect("failed to write test entry chunk");
        write_entry_complete(&mut client, &EntryComplete)
            .await
            .expect("failed to write test EntryComplete");

        let frame = read_next_frame(&mut client)
            .await
            .expect("expected a frame after the undersized recursive entry");
        match frame {
            TransferFrame::Error(failure) => assert_eq!(
                failure.code,
                TransferFailureCode::SizeMismatch,
                "entry declared 100 bytes but sent 2: {failure:?}"
            ),
            other => panic!("expected SizeMismatch error, got {other:?}"),
        }

        assert!(
            !dest_root.join("sub").join("file.txt").exists(),
            "size-mismatched recursive entry must not be written"
        );

        let server_res = harness.server_task.await.expect("server task panicked");
        server_res.expect("server handler failed");
    })
    .await
    .expect("test timed out");
}

/// A5: a blob PUT whose destination already exists must be refused with
/// `TargetAlreadyExists` and must leave the existing file untouched.
#[tokio::test]
async fn blob_put_refuses_existing_target() {
    timeout(Duration::from_secs(60), async {
        let state = temp_state("transfer-blob-existing");
        std::fs::create_dir_all(state.root()).expect("failed to create test state root");
        let target_path = state.root().join("existing-blob.bin");
        std::fs::write(&target_path, b"original").expect("failed to seed existing target");

        let harness = spawn_transfer_harness("xfer-blob-existing").await;
        let mut client = harness.client;

        let hash = iroh_blobs::Hash::new(b"new-blob-bytes").to_string();
        write_blob_put_request(
            &mut client,
            &BlobPutRequest {
                path: target_path.display().to_string(),
                hash,
                format: "raw".to_string(),
                size: 14,
            },
        )
        .await
        .expect("failed to write test BlobPutRequest");

        let frame = read_next_frame(&mut client)
            .await
            .expect("expected a frame after the colliding blob PUT");
        match frame {
            TransferFrame::Error(failure) => assert_eq!(
                failure.code,
                TransferFailureCode::TargetAlreadyExists,
                "blob PUT onto an existing file must be refused: {failure:?}"
            ),
            other => panic!("expected TargetAlreadyExists error, got {other:?}"),
        }

        assert_eq!(
            std::fs::read(&target_path).expect("failed to read existing target"),
            b"original",
            "refused blob PUT must not modify the existing target"
        );

        harness
            .server_task
            .await
            .expect("server task panicked")
            .expect("server handler failed");
    })
    .await
    .expect("test timed out");
}

/// A5: a blob PUT onto a dangling symlink must be refused - a symlink counts
/// as present even when its target does not exist, so uploads can never
/// write *through* it.
#[cfg(unix)]
#[tokio::test]
async fn blob_put_refuses_dangling_symlink_target() {
    timeout(Duration::from_secs(60), async {
        let state = temp_state("transfer-blob-symlink");
        std::fs::create_dir_all(state.root()).expect("failed to create test state root");
        let missing = state.root().join("missing-target.bin");
        let link_path = state.root().join("dangling-link.bin");
        std::os::unix::fs::symlink(&missing, &link_path)
            .expect("failed to create dangling test symlink");
        assert!(
            !missing.exists(),
            "test precondition: symlink target is missing"
        );

        let harness = spawn_transfer_harness("xfer-blob-symlink").await;
        let mut client = harness.client;

        let hash = iroh_blobs::Hash::new(b"link-blob-bytes").to_string();
        write_blob_put_request(
            &mut client,
            &BlobPutRequest {
                path: link_path.display().to_string(),
                hash,
                format: "raw".to_string(),
                size: 15,
            },
        )
        .await
        .expect("failed to write test BlobPutRequest");

        let frame = read_next_frame(&mut client)
            .await
            .expect("expected a frame after the symlink-target blob PUT");
        match frame {
            TransferFrame::Error(failure) => assert_eq!(
                failure.code,
                TransferFailureCode::TargetAlreadyExists,
                "blob PUT onto a dangling symlink must be refused: {failure:?}"
            ),
            other => panic!("expected TargetAlreadyExists error, got {other:?}"),
        }

        assert!(
            !missing.exists(),
            "refused blob PUT must not create the symlink target"
        );

        harness
            .server_task
            .await
            .expect("server task panicked")
            .expect("server handler failed");
    })
    .await
    .expect("test timed out");
}

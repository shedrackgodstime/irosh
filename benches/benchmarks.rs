use criterion::{Criterion, criterion_group, criterion_main};
use irosh::auth::Authenticator;
use std::sync::Arc;

// ── Ticket parsing ──────────────────────────────────────────────

fn bench_ticket_parse_junk(c: &mut Criterion) {
    let ticket_str = "abcdef0123456789".to_string();
    c.bench_function("ticket/parse_junk", |b| {
        b.iter(|| {
            let _ = ticket_str.parse::<irosh::Ticket>();
        })
    });
}

fn bench_ticket_parse_valid(c: &mut Criterion) {
    let ticket_str = "endpoint_test_ticket_placeholder".to_string();
    c.bench_function("ticket/parse_invalid", |b| {
        b.iter(|| {
            let _ = ticket_str.parse::<irosh::Ticket>();
        })
    });
}

// ── IPC serialization ───────────────────────────────────────────

fn bench_ipc_serialize(c: &mut Criterion) {
    let cmd = irosh::server::ipc::IpcCommand::EnableWormhole {
        code: "alpha-bravo-charlie".into(),
        password: Some("hunter2".into()),
        persistent: true,
    };
    c.bench_function("ipc/serialize_enable_wormhole", |b| {
        b.iter(|| serde_json::to_vec(&cmd))
    });
}

fn bench_ipc_deserialize(c: &mut Criterion) {
    let json = br#"{"EnableWormhole":{"code":"alpha-bravo-charlie","password":"hunter2","persistent":true}}"#;
    c.bench_function("ipc/deserialize_enable_wormhole", |b| {
        b.iter(|| {
            let _: irosh::server::ipc::IpcCommand = serde_json::from_slice(json).unwrap();
        })
    });
}

// ── SSH handshake setup (async) ─────────────────────────────────

fn bench_ssh_handshake(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("ssh/handshake_and_auth", |b| {
        b.to_async(&rt).iter_batched(
            || {
                let state_root = std::env::temp_dir().join(format!(
                    "irosh-bench-ssh-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                std::fs::create_dir_all(&state_root).unwrap();
                irosh::StateConfig::new(state_root)
            },
            |state| async {
                let server_state = state.clone();
                let client_state = state;

                let server_identity =
                    irosh::storage::load_or_generate_identity(&server_state)
                        .await
                        .unwrap();
                let client_identity =
                    irosh::storage::load_or_generate_identity(&client_state)
                        .await
                        .unwrap();

                let (client_stream, server_stream) = tokio::io::duplex(1024 * 1024);

                let server_config = Arc::new(irosh::russh::server::Config {
                    auth_rejection_time: std::time::Duration::from_secs(1),
                    keys: vec![server_identity.ssh_key],
                    ..Default::default()
                });
                let authenticator: Arc<dyn irosh::auth::Authenticator> =
                    Arc::new(irosh::auth::KeyOnlyAuth::new(
                        irosh::SecurityConfig {
                            host_key_policy: irosh::config::HostKeyPolicy::Tofu,
                        },
                        Vec::new(),
                        server_state.clone(),
                    ));
                let server_blobs =
                    iroh_blobs::store::fs::FsStore::load(server_state.blobs_path())
                        .await
                        .unwrap();
                let server_handler = irosh::server::handler::ServerHandler::new(
                    authenticator,
                    irosh::server::ConnectionShellState::new(
                        server_state.root().to_path_buf(),
                        server_blobs,
                    ),
                );
                let _server_handle = tokio::spawn(async move {
                    let _ = irosh::russh::server::run_stream(
                        server_config,
                        server_stream,
                        server_handler,
                    )
                    .await;
                });

                let client_config = Arc::new(irosh::russh::client::Config::default());
                let client_handler = irosh::client::handler::ClientHandler::new(
                    "bench-node".to_string(),
                    None,
                    Arc::new(std::sync::Mutex::new(None)),
                    irosh::SecurityConfig {
                        host_key_policy: irosh::config::HostKeyPolicy::Tofu,
                    },
                    client_state.clone(),
                );

                let mut handle = irosh::russh::client::connect_stream(
                    client_config,
                    client_stream,
                    client_handler,
                )
                .await
                .unwrap();

                let _auth = handle
                    .authenticate_publickey(
                        "bench",
                        irosh::russh::keys::PrivateKeyWithHashAlg::new(
                            Arc::new(client_identity.ssh_key),
                            None,
                        ),
                    )
                    .await;

                let _ = std::fs::remove_dir_all(server_state.root());
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

// ── Transfer frame round-trip (async) ────────────────────────────

fn bench_transfer_put_request_roundtrip(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("transfer/put_request_roundtrip", |b| {
        b.to_async(&rt).iter_batched(
            || irosh::transport::transfer::PutRequest {
                path: "/remote/path/file.txt".into(),
                size: 1024,
                mode: Some(0o644),
                recursive: false,
            },
            |req| async {
                let (mut client, mut server) = tokio::io::duplex(2048);
                let write = tokio::spawn(async move {
                    irosh::transport::transfer::write_put_request(&mut client, &req).await
                });
                let read = tokio::spawn(async move {
                    irosh::transport::transfer::read_put_request(&mut server).await
                });
                write.await.unwrap().unwrap();
                read.await.unwrap().unwrap();
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

fn bench_transfer_get_request_roundtrip(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("transfer/get_request_roundtrip", |b| {
        b.to_async(&rt).iter_batched(
            || irosh::transport::transfer::GetRequest {
                path: "/remote/path/file.txt".into(),
                recursive: false,
            },
            |req| async {
                let (mut client, mut server) = tokio::io::duplex(2048);
                let write = tokio::spawn(async move {
                    irosh::transport::transfer::write_get_request(&mut client, &req).await
                });
                let read = tokio::spawn(async move {
                    irosh::transport::transfer::read_get_request(&mut server).await
                });
                write.await.unwrap().unwrap();
                read.await.unwrap().unwrap();
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

fn bench_transfer_chunk_roundtrip(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("transfer/chunk_64k_roundtrip", |b| {
        b.to_async(&rt).iter_batched(
            || vec![0u8; 64 * 1024],
            |chunk| async {
                let (mut client, mut server) = tokio::io::duplex(128 * 1024);
                let write = tokio::spawn(async move {
                    irosh::transport::transfer::write_put_chunk(&mut client, &chunk).await
                });
                let read = tokio::spawn(async move {
                    irosh::transport::transfer::read_put_chunk(&mut server).await
                });
                write.await.unwrap().unwrap();
                read.await.unwrap().unwrap();
            },
            criterion::BatchSize::LargeInput,
        )
    });
}

// ── Argon2 password hashing ──────────────────────────────────────

fn bench_password_hash(c: &mut Criterion) {
    c.bench_function("auth/argon2_hash", |b| {
        b.iter(|| {
            let _ = irosh::auth::hash_password("correct-horse-battery-staple");
        })
    });
}

fn bench_password_verify(c: &mut Criterion) {
    let hash = irosh::auth::hash_password("correct-horse-battery-staple").unwrap();
    c.bench_function("auth/argon2_verify", |b| {
        b.iter(|| {
            let auth = irosh::auth::PasswordAuth::new(&hash);
            let _ = auth.check_password("someone", "correct-horse-battery-staple");
        })
    });
}

fn bench_transfer_full_pipeline_4mb(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("transfer/full_pipeline_4mb", |b| {
        b.to_async(&rt).iter_batched(
            || {
                let chunks: Vec<Vec<u8>> = (0..64)
                    .map(|_| vec![0u8; 64 * 1024])
                    .collect();
                (chunks, 64u64 * 64 * 1024)
            },
            |(chunks, total_size)| async move {
                let (mut client, mut server) = tokio::io::duplex(512 * 1024);
                let total = total_size;
                let chunks_count = chunks.len();
                let write = tokio::spawn(async move {
                    irosh::transport::transfer::write_put_request(
                        &mut client,
                        &irosh::transport::transfer::PutRequest {
                            path: "/bench/file.dat".into(),
                            size: total,
                            mode: Some(0o644),
                            recursive: false,
                        },
                    )
                    .await?;
                    for chunk in &chunks {
                        irosh::transport::transfer::write_put_chunk(&mut client, chunk).await?;
                    }
                    irosh::transport::transfer::write_put_complete(
                        &mut client,
                        &irosh::transport::transfer::TransferComplete { size: total },
                    )
                    .await
                });
                let read = tokio::spawn(async move {
                    let _ = irosh::transport::transfer::read_put_request(&mut server).await?;
                    for _ in 0..chunks_count {
                        let _ = irosh::transport::transfer::read_put_chunk(&mut server).await?;
                    }
                    irosh::transport::transfer::read_put_complete(&mut server).await
                });
                write.await.unwrap().unwrap();
                read.await.unwrap().unwrap();
            },
            criterion::BatchSize::LargeInput,
        )
    });
}

criterion_group!(
    benches,
    bench_ticket_parse_junk,
    bench_ticket_parse_valid,
    bench_ipc_serialize,
    bench_ipc_deserialize,
    bench_ssh_handshake,
    bench_transfer_put_request_roundtrip,
    bench_transfer_get_request_roundtrip,
    bench_transfer_chunk_roundtrip,
    bench_transfer_full_pipeline_4mb,
    bench_password_hash,
    bench_password_verify,
);
criterion_main!(benches);

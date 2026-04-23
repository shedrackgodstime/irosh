use super::*;
use std::fs;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{SystemTime, UNIX_EPOCH};

use russh::client;
use russh::server;
use tokio::io::duplex;

use crate::client::handler::ClientHandler;
use crate::server::handler::ServerHandler;
use crate::storage::load_or_generate_identity;
use crate::{IroshError, SecurityConfig, StateConfig, config::HostKeyPolicy};

mod auth;
mod options;
mod session;

fn temp_state_dir(label: &str) -> StateConfig {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("irosh-test-{label}-{unique}"));
    fs::create_dir_all(&path).unwrap();
    StateConfig::new(path)
}

async fn connect_test_session(
    server_state: &StateConfig,
    client_state: &StateConfig,
) -> (
    Session,
    tokio::task::JoinHandle<
        std::result::Result<russh::server::RunningSession<ServerHandler>, IroshError>,
    >,
) {
    let server_identity = load_or_generate_identity(server_state).await.unwrap();
    let client_identity = load_or_generate_identity(client_state).await.unwrap();
    let (client_stream, server_stream) = duplex(1024 * 1024);

    let server_config = Arc::new(server::Config {
        auth_rejection_time: std::time::Duration::from_secs(1),
        keys: vec![server_identity.ssh_key],
        ..Default::default()
    });
    let server_handler = ServerHandler::new(
        Vec::new(),
        SecurityConfig {
            host_key_policy: HostKeyPolicy::Tofu,
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
            host_key_policy: HostKeyPolicy::Tofu,
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
    let channel = handle.channel_open_session().await.unwrap();

    (
        Session {
            handle: Arc::new(handle),
            channel: Some(channel),
            connection: None,
            endpoint: None,
            remote_metadata: None,
            state: SessionState::Authenticated,
        },
        server_task,
    )
}

use crate::config::HostKeyPolicy;
use crate::error::{Result, ServerError};
use crate::server::{Server, ServerOptions, ServerReady};
use crate::storage::{load_all_authorized_clients, load_or_generate_identity};
use crate::transport::iroh::bind_server_endpoint;
use russh::server;
use std::sync::Arc;

pub(crate) async fn inspect_server(options: &ServerOptions) -> Result<ServerReady> {
    let identity = load_or_generate_identity(options.state()).await?;
    let server_pub = identity.ssh_key.public_key();
    let node_id = identity.secret_key.public();
    let addr = iroh::EndpointAddr::new(node_id);

    Ok(ServerReady {
        endpoint_id: node_id.to_string(),
        ticket: crate::transport::ticket::Ticket::new(addr),
        relay_urls: vec![],
        direct_addresses: vec![],
        host_key_openssh: server_pub
            .to_openssh()
            .map_err(|source| ServerError::FormatHostKey { source })?,
    })
}

pub(crate) async fn bind_server(options: ServerOptions) -> Result<(ServerReady, Server)> {
    let identity = load_or_generate_identity(options.state()).await?;
    let server_key = identity.ssh_key;
    let server_pub = server_key.public_key().clone();

    let mut authorized_clients = options.authorized_key_list().to_vec();
    if authorized_clients.is_empty()
        && options.security_config().host_key_policy != HostKeyPolicy::AcceptAll
    {
        let mut saved_keys = load_all_authorized_clients(options.state())?;
        authorized_clients.append(&mut saved_keys);
    }

    let alpn = crate::transport::iroh::derive_alpn(options.secret_value());
    let transport = bind_server_endpoint(identity.secret_key, alpn).await?;

    let startup = ServerReady {
        endpoint_id: transport.endpoint_id,
        ticket: crate::transport::ticket::Ticket::new(transport.addr),
        relay_urls: transport.relay_urls,
        direct_addresses: transport.direct_addresses,
        host_key_openssh: server_pub
            .to_openssh()
            .map_err(|source| ServerError::FormatHostKey { source })?,
    };

    let config = Arc::new(server::Config {
        auth_rejection_time: std::time::Duration::from_secs(1),
        keys: vec![server_key],
        ..Default::default()
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);

    Ok((
        startup,
        Server {
            endpoint: transport.endpoint,
            config,
            authorized_clients,
            security: options.security_config(),
            state: options.state().clone(),
            secret: options.secret.clone(),
            shutdown_tx,
            shutdown_rx,
        },
    ))
}

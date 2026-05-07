use crate::auth::AuthMethod;
use crate::error::{Result, ServerError};
use crate::server::{Server, ServerOptions, ServerReady};
use crate::storage::{load_all_authorized_clients, load_or_generate_identity};
use crate::transport::iroh::bind_server_endpoint;
use russh::server;
use russh::{MethodKind, MethodSet};
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

/// Converts our [`AuthMethod`] flags into russh's [`MethodSet`].
fn build_method_set(methods: &[AuthMethod]) -> MethodSet {
    let mut set = MethodSet::empty();
    for method in methods {
        match method {
            AuthMethod::PublicKey => set.push(MethodKind::PublicKey),
            AuthMethod::Password => set.push(MethodKind::Password),
        }
    }
    set
}

pub(crate) async fn bind_server(options: ServerOptions) -> Result<(ServerReady, Server)> {
    let identity = load_or_generate_identity(options.state()).await?;
    let server_key = identity.ssh_key;
    let server_pub = server_key.public_key().clone();

    // Build the authenticator: custom if provided, else default UnifiedAuthenticator.
    let authenticator: Arc<dyn crate::auth::Authenticator> = if let Some(custom) =
        options.authenticator.clone()
    {
        custom
    } else {
        // Build the default unified auth from existing config.
        let vault = load_all_authorized_clients(options.state())?;
        let keys = vault.into_iter().map(|(_, k)| k).collect();
        let node_password = crate::storage::load_shadow_file(options.state()).unwrap_or_default();

        Arc::new(crate::auth::UnifiedAuthenticator::new(
            options.state().clone(),
            options.security_config().host_key_policy,
            keys,
            node_password,
            None, // No temp password for primary connections
        ))
    };

    // Configure russh to advertise only the methods our authenticator supports.
    let supported = authenticator.supported_methods();
    let method_set = build_method_set(&supported);

    let primary_alpn = crate::transport::iroh::derive_alpn(options.secret_value());
    let alpns = vec![
        primary_alpn,
        iroh_gossip::ALPN.to_vec(),
        crate::transport::wormhole::PAIRING_ALPN.to_vec(),
    ];
    let transport =
        bind_server_endpoint(identity.secret_key, alpns, options.relay_mode.clone()).await?;

    // Generate a stable ticket by using only NodeId and Relay information.
    // Transient direct addresses are omitted to ensure the ticket string
    // remains identical across server restarts and network changes.
    let mut stable_addr = iroh::EndpointAddr::new(transport.addr.id);
    if let Some(relay_url) = transport.addr.relay_urls().next() {
        stable_addr = stable_addr.with_relay_url(relay_url.clone());
    }

    let startup = ServerReady {
        endpoint_id: transport.endpoint_id,
        ticket: crate::transport::ticket::Ticket::new(stable_addr),
        relay_urls: transport.relay_urls,
        direct_addresses: transport.direct_addresses,
        host_key_openssh: server_pub
            .to_openssh()
            .map_err(|source| ServerError::FormatHostKey { source })?,
    };

    let config = Arc::new(server::Config {
        auth_rejection_time: std::time::Duration::from_secs(1),
        keys: vec![server_key],
        methods: method_set,
        ..Default::default()
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    let (control_tx, control_rx) = tokio::sync::mpsc::channel(32);

    let ticket = startup.ticket.clone();
    Ok((
        startup,
        Server {
            gossip: iroh_gossip::net::Gossip::builder().spawn(transport.endpoint.clone()),
            endpoint: transport.endpoint,
            ipc_enabled: options.ipc_enabled,
            config,
            authenticator,
            state: options.state().clone(),
            security: options.security_config(),
            secret: options.secret.clone(),
            shutdown_tx,
            shutdown_rx,
            control_tx,
            control_rx,
            ticket,
            wormhole_confirmation: options.wormhole_confirmation.clone(),
        },
    ))
}

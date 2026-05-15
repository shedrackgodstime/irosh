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
    let mut addr = iroh::EndpointAddr::new(node_id);

    // Ensure the stable ticket includes at least one relay URL for discovery.
    // Pure node-id tickets (discovery-only) often fail if Pkarr propagation is slow.
    let relay_url = options
        .relay_url
        .clone()
        .unwrap_or_else(|| "default".to_string());
    if relay_url != "disabled" {
        let url_to_parse = if relay_url == "default" {
            "https://relay.iroh.network"
        } else {
            &relay_url
        };

        if let Ok(url) = url_to_parse.parse::<iroh::RelayUrl>() {
            addr = addr.with_relay_url(url);
        }
    }

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

    // Build the authenticator: custom if provided, else respect auth_mode.
    let authenticator: Arc<dyn crate::auth::Authenticator> = if let Some(custom) =
        options.authenticator.clone()
    {
        custom
    } else {
        match options.auth_mode {
            crate::auth::AuthMode::Key => {
                let vault = load_all_authorized_clients(options.state())?;
                let keys = vault.into_iter().map(|(_, k)| k).collect();
                Arc::new(crate::auth::KeyOnlyAuth::new(
                    options.security_config(),
                    keys,
                    options.state().clone(),
                ))
            }
            crate::auth::AuthMode::Password => {
                let hash = crate::storage::load_shadow_file(options.state())?;
                if let Some(hash) = hash {
                    Arc::new(crate::auth::PasswordAuth::new(hash))
                } else {
                    return Err(ServerError::AuthConfiguration {
                            reason: "Password auth requested but no password set. Run 'irosh passwd set' first."
                                .to_string(),
                        }
                        .into());
                }
            }
            crate::auth::AuthMode::Combined => {
                let vault = load_all_authorized_clients(options.state())?;
                let keys = vault.into_iter().map(|(_, k)| k).collect();
                let key_auth = crate::auth::KeyOnlyAuth::new(
                    options.security_config(),
                    keys,
                    options.state().clone(),
                );
                let hash = crate::storage::load_shadow_file(options.state())?;
                if let Some(hash) = hash {
                    let pass_auth = crate::auth::PasswordAuth::new(hash);
                    Arc::new(crate::auth::CombinedAuth::new(key_auth, pass_auth))
                } else {
                    return Err(ServerError::AuthConfiguration {
                            reason: "Combined auth (key+password) requested but no password set. Run 'irosh passwd set' first."
                                .to_string(),
                        }
                        .into());
                }
            }
            crate::auth::AuthMode::Unified => {
                let vault = load_all_authorized_clients(options.state())?;
                let keys = vault.into_iter().map(|(_, k)| k).collect();

                Arc::new(crate::auth::UnifiedAuthenticator::new(
                    options.state().clone(),
                    options.security_config().host_key_policy,
                    keys,
                    None, // No temp password for primary connections
                ))
            }
        }
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

    // Generate a stable ticket by using only EndpointId and Relay information.
    // Transient direct addresses are omitted to ensure the ticket string
    // remains identical across server restarts and network changes.
    let mut stable_addr = iroh::EndpointAddr::new(transport.addr.id);
    for relay_url in transport.addr.relay_urls() {
        stable_addr = stable_addr.with_relay_url(relay_url.clone());
    }

    if matches!(options.relay_mode, iroh::RelayMode::Disabled) {
        // For local testing without relays, we MUST include direct addresses
        // otherwise discovery will fail.
        for addr in transport.addr.ip_addrs() {
            stable_addr = stable_addr.with_ip_addr(*addr);
        }
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
            shutdown_on_wormhole_success: options.shutdown_on_wormhole_success,
        },
    ))
}

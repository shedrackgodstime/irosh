//! Iroh endpoint initialization and connection routing.

use crate::error::{Result, TransportError};
use iroh::{Endpoint, EndpointAddr, RelayMode, SecretKey};

/// The base Application-Layer Protocol Negotiation (ALPN) string.
pub(crate) const BASE_ALPN: &[u8] = b"irosh/1";

/// Derives a unique ALPN for this session based on an optional shared secret.
/// If no secret is provided, the standard "irosh/1" protocol is used.
/// If a secret is provided, it is hashed to create a private stealth protocol.
pub fn derive_alpn(secret: Option<&str>) -> Vec<u8> {
    match secret {
        None => BASE_ALPN.to_vec(),
        Some(s) => {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(s.as_bytes());
            let hash = hex::encode(&hasher.finalize()[..8]); // Use first 8 bytes for brevity
            format!("{}/{}", String::from_utf8_lossy(BASE_ALPN), hash).into_bytes()
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerEndpoint {
    /// The actual Iroh endpoint bound on the networking interfaces.
    pub endpoint: Endpoint,
    /// The endpoint address, used for out-of-band P2P connection sharing.
    pub addr: EndpointAddr,
    /// The unique identifier of the node (Node ID).
    pub endpoint_id: String,
    /// The list of relay server URLs this node is connected to.
    pub relay_urls: Vec<String>,
    /// The direct IP addresses this node is bound to.
    pub direct_addresses: Vec<String>,
}

/// Binds a new Iroh endpoint for the server to listen on.
pub async fn bind_server_endpoint(secret_key: SecretKey, alpn: Vec<u8>) -> Result<ServerEndpoint> {
    let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(secret_key)
        .alpns(vec![alpn.clone()])
        .relay_mode(RelayMode::Default)
        .bind()
        .await
        .map_err(|source| TransportError::EndpointBind { source })?;

    // Wait until the node finishes initial networking setup and is online.
    endpoint.online().await;

    let endpoint_addr = endpoint.addr();
    let node_id = endpoint.id();

    // Use pure EndpointAddr for now, as NodeAddr path is elusive in this environment.
    // We will parse it back in the client.

    let direct_addresses = endpoint_addr
        .ip_addrs()
        .map(|addr| addr.to_string())
        .collect::<Vec<_>>();
    let relay_urls = endpoint_addr
        .relay_urls()
        .map(|url| url.to_string())
        .collect::<Vec<_>>();

    Ok(ServerEndpoint {
        endpoint_id: node_id.to_string(),
        endpoint,
        addr: endpoint_addr,
        relay_urls,
        direct_addresses,
    })
}

/// Binds a new Iroh endpoint for a client connection.
pub async fn bind_client_endpoint(secret_key: SecretKey, alpn: Vec<u8>) -> Result<Endpoint> {
    let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(secret_key)
        .alpns(vec![alpn])
        .relay_mode(RelayMode::Default)
        .bind()
        .await
        .map_err(|source| TransportError::EndpointBind { source })?;

    // Wait until the node finishes initial networking setup and is online.
    endpoint.online().await;
    Ok(endpoint)
}

//! Iroh endpoint initialization and connection routing.

use crate::config::EndpointId;
use crate::error::{Result, TransportError};
use iroh::{Endpoint, EndpointAddr, RelayConfig, RelayMode, SecretKey};

/// The base Application-Layer Protocol Negotiation (ALPN) string.
pub(crate) const BASE_ALPN: &[u8] = b"irosh/1";

/// Derives a unique ALPN for this session based on an optional shared secret.
/// If no secret is provided, the standard "irosh/1" protocol is used.
/// If a secret is provided, it is hashed to create a private stealth protocol.
///
/// An empty or whitespace-only secret is treated as "no secret" and yields
/// the base ALPN. Deriving a stealth ALPN from a degenerate secret would
/// give the appearance of stealth while providing none: the derived ALPN
/// travels in the clear in the QUIC ClientHello, and an observer who sees it
/// can reproduce it by hashing the same empty input.
#[must_use]
pub fn derive_alpn(secret: Option<&str>) -> Vec<u8> {
    match secret.map(str::trim).filter(|s| !s.is_empty()) {
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

/// A bound Iroh endpoint ready for peer-to-peer connections.
///
/// Holds the live [`Endpoint`], its address, endpoint ID, and any relay
/// URLs the endpoint is connected to.
#[derive(Debug, Clone)]
pub struct ServerEndpoint {
    /// The actual Iroh endpoint bound on the networking interfaces.
    pub endpoint: Endpoint,
    /// The endpoint address, used for out-of-band P2P connection sharing.
    pub addr: EndpointAddr,
    /// The unique identifier of the endpoint (Endpoint ID).
    pub endpoint_id: EndpointId,
    /// The list of relay server URLs this node is connected to.
    pub relay_urls: Vec<String>,
    /// The direct IP addresses this node is bound to.
    pub direct_addresses: Vec<String>,
}

/// Binds a new Iroh endpoint for the server to listen on.
///
/// # Errors
///
/// Returns an error if the endpoint fails to bind.
#[must_use]
pub async fn bind_server_endpoint(
    secret_key: SecretKey,
    alpns: Vec<Vec<u8>>,
    relay_mode: RelayMode,
) -> Result<ServerEndpoint> {
    let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(secret_key)
        .alpns(alpns)
        .relay_mode(relay_mode)
        .bind()
        .await
        .map_err(|source| TransportError::EndpointBind { source })?;

    // Wait until the endpoint finishes initial networking setup and is online.
    // Timeout prevents blocking forever if relays are unreachable.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), endpoint.online()).await;

    let endpoint_addr = endpoint.addr();
    let endpoint_id = endpoint.id();

    let direct_addresses = endpoint_addr
        .ip_addrs()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>();
    let relay_urls = endpoint_addr
        .relay_urls()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>();

    Ok(ServerEndpoint {
        endpoint_id: EndpointId::new(endpoint_id.to_string()),
        endpoint,
        addr: endpoint_addr,
        relay_urls,
        direct_addresses,
    })
}

/// Binds a new Iroh endpoint for a client connection.
///
/// # Errors
///
/// Returns an error if the endpoint fails to bind.
#[must_use]
pub async fn bind_client_endpoint(
    secret_key: SecretKey,
    alpns: Vec<Vec<u8>>,
    relay_mode: RelayMode,
) -> Result<Endpoint> {
    let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(secret_key)
        .alpns(alpns)
        .relay_mode(relay_mode)
        .bind()
        .await
        .map_err(|source| TransportError::EndpointBind { source })?;

    // Wait until the endpoint finishes initial networking setup and is online.
    // Timeout prevents blocking forever if relays are unreachable.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), endpoint.online()).await;
    Ok(endpoint)
}

/// Parses a relay string into an Iroh `RelayMode`.
///
/// Supported formats:
/// - "disabled": Disables relaying entirely.
/// - "default": Uses the default Iroh relay servers.
/// - Any valid URL: Uses a custom relay server at that address.
///
/// # Errors
///
/// Returns an error if the relay URL is invalid and cannot be parsed.
#[must_use]
pub fn parse_relay_mode(url: &str) -> Result<RelayMode> {
    match url {
        "disabled" => Ok(RelayMode::Disabled),
        "default" => Ok(RelayMode::Default),
        u if u.starts_with("http") => {
            let url: iroh::RelayUrl = u
                .parse()
                .map_err(|_| TransportError::InvalidRelayUrl { url: u.to_string() })?;
            Ok(RelayMode::Custom(RelayConfig::from(url).into()))
        }
        _ => Ok(RelayMode::Default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "irosh/1";

    #[test]
    fn derive_alpn_without_secret_uses_base_alpn() {
        assert_eq!(derive_alpn(None), BASE.as_bytes());
    }

    #[test]
    fn derive_alpn_with_secret_uses_a_distinct_alpn() {
        let alpn = derive_alpn(Some("correct horse battery staple"));
        assert_ne!(alpn, BASE.as_bytes());
        assert!(String::from_utf8_lossy(&alpn).starts_with(BASE));
    }

    #[test]
    fn derive_alpn_with_empty_secret_is_treated_as_no_secret() {
        // A blank secret must not enable a "stealth" ALPN: the derived ALPN
        // is observable in the clear, so hashing the empty string buys
        // nothing while making the peer look hidden.
        assert_eq!(derive_alpn(Some("")), BASE.as_bytes());
    }

    #[test]
    fn derive_alpn_with_whitespace_secret_is_treated_as_no_secret() {
        assert_eq!(derive_alpn(Some("   ")), BASE.as_bytes());
        assert_eq!(derive_alpn(Some("\t\n")), BASE.as_bytes());
    }

    #[test]
    fn derive_alpn_with_empty_secret_matches_derive_alpn_without_secret() {
        // The invariant that matters: an empty secret must be
        // indistinguishable from no secret at all, otherwise a client and a
        // server configured with "" would fail to find each other.
        assert_eq!(derive_alpn(Some("")), derive_alpn(None));
    }
}

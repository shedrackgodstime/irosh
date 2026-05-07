//! Network and environment diagnostics for irosh.
//!
//! This module provides a lightweight, non-interactive diagnostic probe that
//! can be used to report the local P2P network environment without requiring
//! a full server or client session to be established.

use crate::config::StateConfig;
use crate::error::{Result, TransportError};
use crate::storage::keys::load_or_generate_identity;
use crate::transport::iroh::derive_alpn;

/// The result of a P2P network probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkProbe {
    /// The Node ID this instance presents to the network.
    pub node_id: String,
    /// Relay URLs the endpoint is connected to.
    pub relay_urls: Vec<String>,
    /// Direct IP addresses the endpoint is reachable on.
    pub direct_addresses: Vec<String>,
}

impl NetworkProbe {
    /// Returns `true` if the endpoint has at least one direct address,
    /// meaning hole-punching is likely available (Open/Restricted NAT).
    pub fn has_direct_connectivity(&self) -> bool {
        !self.direct_addresses.is_empty()
    }

    /// Returns `true` if the endpoint is connected to at least one relay.
    pub fn has_relay_connectivity(&self) -> bool {
        !self.relay_urls.is_empty()
    }

    /// Returns a human-readable NAT description.
    pub fn nat_description(&self) -> &'static str {
        match (
            self.has_direct_connectivity(),
            self.has_relay_connectivity(),
        ) {
            (true, _) => "Open NAT (direct connection available)",
            (false, true) => "Restricted NAT (relay fallback required)",
            (false, false) => "No connectivity detected",
        }
    }
}

/// Binds a short-lived Iroh endpoint to probe the local network environment.
///
/// This endpoint is closed immediately after the probe completes.
/// It does NOT share state with any running server or client.
///
/// # Errors
///
/// Returns an error if the identity cannot be loaded or the endpoint fails to bind.
pub async fn probe_network(state: &StateConfig) -> Result<NetworkProbe> {
    let identity = load_or_generate_identity(state).await?;
    let alpn = derive_alpn(None);

    let endpoint = iroh::Endpoint::builder()
        .secret_key(identity.secret_key)
        .alpns(vec![alpn])
        .relay_mode(iroh::RelayMode::Default)
        .bind()
        .await
        .map_err(|source| TransportError::EndpointBind { source })?;

    // Wait for the endpoint to come online and gather its addresses.
    endpoint.online().await;

    let addr = endpoint.addr();
    let node_id = endpoint.id().to_string();

    let relay_urls = addr.relay_urls().map(|u| u.to_string()).collect::<Vec<_>>();

    let direct_addresses = addr.ip_addrs().map(|a| a.to_string()).collect::<Vec<_>>();

    // Cleanly shut down the transient endpoint before returning.
    endpoint.close().await;

    Ok(NetworkProbe {
        node_id,
        relay_urls,
        direct_addresses,
    })
}

/// The result of a security permissions check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityReport {
    pub root_path: std::path::PathBuf,
    pub root_exists: bool,
    pub root_mode: Option<u32>,
    pub root_loose: bool,
    pub key_path: std::path::PathBuf,
    pub key_exists: bool,
    pub key_mode: Option<u32>,
    pub key_unsafe: bool,
}

/// The result of system-level environment checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemReport {
    pub ssh_version: Option<String>,
    pub udp_available: bool,
}

/// Checks the permissions of the state directory and identity keys.
pub fn check_security(state: &StateConfig) -> SecurityReport {
    let root_path = state.root().to_path_buf();
    let key_path = state.root().join("keys").join("node.secret");

    let mut report = SecurityReport {
        root_path: root_path.clone(),
        root_exists: root_path.exists(),
        root_mode: None,
        root_loose: false,
        key_path: key_path.clone(),
        key_exists: key_path.exists(),
        key_mode: None,
        key_unsafe: false,
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::metadata(&root_path) {
            let mode = meta.mode() & 0o777;
            report.root_mode = Some(mode);
            report.root_loose = (mode & 0o077) != 0;
        }

        if let Ok(meta) = std::fs::metadata(&key_path) {
            let mode = meta.mode() & 0o777;
            report.key_mode = Some(mode);
            report.key_unsafe = (mode & 0o077) != 0;
        }
    }

    report
}

/// Checks the local system environment for required dependencies.
pub fn check_system() -> SystemReport {
    let ssh_version = std::process::Command::new("ssh")
        .arg("-V")
        .output()
        .ok()
        .and_then(|out| {
            String::from_utf8_lossy(&out.stderr)
                .split_whitespace()
                .next()
                .map(|s| s.to_string())
        });

    // Check if we can bind a UDP socket (basic capability check)
    let udp_available = std::net::UdpSocket::bind("0.0.0.0:0").is_ok();

    SystemReport {
        ssh_version,
        udp_available,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nat_description_logic() {
        let open_nat = NetworkProbe {
            node_id: "test".to_string(),
            relay_urls: vec!["relay".to_string()],
            direct_addresses: vec!["1.2.3.4".to_string()],
        };
        assert_eq!(
            open_nat.nat_description(),
            "Open NAT (direct connection available)"
        );

        let restricted_nat = NetworkProbe {
            node_id: "test".to_string(),
            relay_urls: vec!["relay".to_string()],
            direct_addresses: vec![],
        };
        assert_eq!(
            restricted_nat.nat_description(),
            "Restricted NAT (relay fallback required)"
        );

        let no_conn = NetworkProbe {
            node_id: "test".to_string(),
            relay_urls: vec![],
            direct_addresses: vec![],
        };
        assert_eq!(no_conn.nat_description(), "No connectivity detected");
    }
}

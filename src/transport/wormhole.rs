//! Rendezvous discovery logic using Pkarr for Wormhole pairing.
//!
//! This module implements the "Global Trust Seed" bridge, allowing peers
//! to exchange connection tickets across the internet using short-lived
//! 3-word codes without needing static IPs or open ports.

use crate::error::{Result, TransportError};
use crate::transport::ticket::Ticket;
use pkarr::{Client, Keypair, SignedPacket, dns::rdata::RData, dns::rdata::TXT};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tracing::{debug, info, warn};

/// The salt used to ensure irosh wormhole topics are distinct from other pkarr applications.
const WORMHOLE_PKARR_SALT: &[u8] = b"irosh-wormhole-v1";

/// The ALPN used for the one-time "Trust-Seed" pairing handshake.
pub const PAIRING_ALPN: &[u8] = b"irosh/pairing/v1";

/// Derives a Pkarr Keypair from a human-readable wormhole code.
pub fn derive_keypair(code: &str) -> Keypair {
    let mut hasher = Sha256::new();
    hasher.update(WORMHOLE_PKARR_SALT);
    hasher.update(code.as_bytes());
    let seed: [u8; 32] = hasher.finalize().into();
    Keypair::from_secret_key(&seed)
}

/// Resolves a connection ticket using Pkarr as a rendezvous point.
pub async fn listen_for_ticket(_endpoint: &iroh::Endpoint, code: &str) -> Result<Ticket> {
    let keypair = derive_keypair(code);
    let public_key = keypair.public_key();
    let client = Client::builder()
        .build()
        .map_err(|e| TransportError::ProtocolError {
            details: format!("Failed to build pkarr client: {}", e),
        })?;

    info!("🔮 Searching for wormhole rendezvous via Pkarr: {}", code);

    // Poll the relays until we find the record.
    for i in 0..60 {
        // Try for 5 minutes (5s intervals)
        if let Some(signed_packet) = client.resolve(&public_key).await {
            for record in signed_packet.all_resource_records() {
                if let RData::TXT(txt) = &record.rdata {
                    if let Ok(content) = String::try_from(txt.clone()) {
                        if let Some(ticket_str) = content.strip_prefix("irosh-ticket=") {
                            if let Ok(ticket) = ticket_str.parse::<Ticket>() {
                                info!("✨ Wormhole discovered via Pkarr rendezvous");
                                return Ok(ticket);
                            }
                        }
                    }
                }
            }
        }

        if i % 6 == 0 && i > 0 {
            info!("... still searching for wormhole: {} (attempt {})", code, i);
        }

        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    Err(TransportError::ProtocolError {
        details: "Timed out waiting for wormhole discovery".to_string(),
    }
    .into())
}

/// Publishes a connection ticket using Pkarr as a rendezvous point.
pub async fn broadcast_ticket_loop(
    _gossip: &iroh_gossip::net::Gossip, // Kept for API compatibility for now
    code: &str,
    ticket: Ticket,
) -> Result<()> {
    let keypair = derive_keypair(code);
    let client = Client::builder()
        .build()
        .map_err(|e| TransportError::ProtocolError {
            details: format!("Failed to build pkarr client: {}", e),
        })?;

    let msg = format!("irosh-ticket={}", ticket);
    let txt = TXT::try_from(msg.as_str()).map_err(|e| TransportError::ProtocolError {
        details: format!("Failed to create TXT record: {}", e),
    })?;

    let signed_packet = SignedPacket::builder()
        .txt(
            "_irosh"
                .try_into()
                .map_err(|_| TransportError::ProtocolError {
                    details: "Failed to convert _irosh string to Pkarr name".to_string(),
                })?,
            txt,
            300,
        )
        .sign(&keypair)
        .map_err(|e| TransportError::ProtocolError {
            details: format!("Failed to sign pkarr packet: {}", e),
        })?;

    info!("📡 Publishing wormhole to Pkarr rendezvous: {}", code);

    loop {
        match client.publish(&signed_packet, None).await {
            Ok(_) => debug!("Successfully published to Pkarr rendezvous"),
            Err(e) => warn!("Failed to publish to Pkarr rendezvous: {}", e),
        }

        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

/// Actively unpublishes a wormhole from Pkarr relays to minimize lingering discovery.
pub async fn unpublish_ticket(code: &str) -> Result<()> {
    let keypair = derive_keypair(code);
    let client = Client::builder()
        .build()
        .map_err(|e| TransportError::ProtocolError {
            details: format!("Failed to build pkarr client: {}", e),
        })?;

    // Create an empty signed packet (tombstone) to overwrite the old record
    let signed_packet =
        SignedPacket::builder()
            .sign(&keypair)
            .map_err(|e| TransportError::ProtocolError {
                details: format!("Failed to sign empty pkarr packet: {}", e),
            })?;

    debug!("📡 Unpublishing wormhole from Pkarr: {}", code);

    // We only try once. If it fails, the TTL will naturally expire it anyway.
    let _ = client.publish(&signed_packet, None).await;

    Ok(())
}

/// Generates a random, human-friendly wormhole pairing code.
///
/// The code consists of two random words and a single digit, joined by hyphens.
/// Example: `apple-banana-7`
pub fn generate_code() -> String {
    use rand::Rng;
    let mut rng = rand::rng();

    const WORDS: &[&str] = &[
        "apple", "banana", "cherry", "dog", "elephant", "fox", "grape", "honey", "iron", "jungle",
        "kite", "lemon", "mountain", "night", "ocean", "piano", "quartz", "river", "sky", "tiger",
        "umbrella", "valley", "whale", "xray", "yellow", "zebra", "amber", "bright", "crystal",
        "delta", "echo", "frost",
    ];

    let w1 = WORDS[rng.random_range(0..WORDS.len())];
    let w2 = WORDS[rng.random_range(0..WORDS.len())];
    let n = rng.random_range(1..10);

    format!("{}-{}-{}", w1, w2, n)
}

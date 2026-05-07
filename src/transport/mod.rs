//! Networking transports and protocol implementation.
//!
//! This module provides the glue between the [Iroh](https://iroh.computer) 
//! peer-to-peer stack and the SSH protocol. It handles:
//!
//! 1. **Node Discovery**: Finding peers via P2P tickets and Pkarr (Wormhole).
//! 2. **Connection Establishment**: Hole-punching and relaying via QUIC.
//! 3. **Protocol Multiplexing**: Handling both standard SSH sessions and 
//!    irosh-specific transfer protocols (Wormhole pairing, Metadata).
//!
//! ## Sub-modules
//!
//! - [`ticket`]: The core P2P connection string format.
//! - [`wormhole`]: Peer rendezvous using human-friendly codes.
//! - [`metadata`]: Exchange of terminal types, version info, and peer aliases.
//! - [`stream`]: Bi-directional stream abstractions over Iroh.
//! - [`transfer`]: Specialized protocols for binary data and state sync.

pub mod iroh;
pub mod metadata;
pub mod stream;
pub mod ticket;
pub mod transfer;
pub mod wormhole;

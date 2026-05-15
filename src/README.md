# Irosh: Peer-to-Peer Secure Shell Library

`irosh` is a high-assurance networking library that bridges the [Iroh](https://iroh.computer) P2P stack with the [SSH](https://en.wikipedia.org/wiki/Secure_Shell) protocol. It enables developers to build applications with secure, direct terminal sessions and file transfers that work seamlessly across NATs and firewalls without centralized infrastructure.

## Key Features

- **Identity-First Security**: Uses Ed25519 node keys for both network discovery and cryptographic SSH authentication.
- **Zero-Config Connectivity**: Full NAT traversal and relaying provided by the Iroh transport layer.
- **Human-Friendly Pairing**: Establish trust between nodes using 3-word "Wormhole" codes, powered by Pkarr.
- **Unified Authentication**: A flexible, policy-driven auth system supporting Public Keys, Passwords, and Trust-On-First-Use (TOFU).

## Quick Start

Add `irosh` to your `Cargo.toml`. To host a P2P-accessible SSH server:

```rust,no_run
use irosh::{Server, ServerOptions, StateConfig};

#[tokio::main]
async fn main() -> irosh::Result<()> {
    // 1. Configure the state directory for keys and trust records
    let options = ServerOptions::new(StateConfig::new("./state".into()));
    
    // 2. Bind the server and get a shareable Ticket
    let (ready, server) = Server::bind(options).await?;
    println!("Server Ticket: {}", ready.ticket());
    
    // 3. Run the server loop
    server.run().await
}
```

## Library Architecture

`irosh` follows a **"Fat Library"** design philosophy. All protocol state machines, cryptographic handshakes, and P2P orchestration are encapsulated within this crate. This ensures that the underlying transport remains stable regardless of the frontend implementation.

### Core Modules

- **`server`**: Asynchronous SSH server implementation with PTY allocation.
- **`client`**: P2P-aware SSH client with interactive session support.
- **`auth`**: Pluggable security policies and multi-factor authentication backends.
- **`transport`**: Low-level Iroh integration and Pkarr-based discovery.
- **`storage`**: Secure, atomic persistence for keys and peer profiles.

## Feature Flags

- `server`: Enables the P2P SSH server and PTY management.
- `client`: Enables the P2P SSH client and interactive handlers.
- `storage`: Enables persistent storage for identities and trust records.
- `transport`: Enables the underlying Iroh and Pkarr stacks.

## ⚠️ Important Disclaimer & Liability

Irosh is a high-assurance tool, but it is currently in active, early-stage development. By using this library, you agree to the following:

1.  **"As-Is" Basis**: This software is provided without warranty of any kind.
2.  **User Responsibility**: You are solely responsible for your actions and must only use this tool for authorized access.
3.  **Non-Liability**: The author(s) and contributors shall **not be held responsible** for any misuse, damage, data loss, or legal consequences.
4.  **No Formal Audit**: While the library utilizes industry-standard primitives (Ed25519, ChaCha20-Poly1305), it has not been formally audited.

---

License: MIT OR Apache-2.0

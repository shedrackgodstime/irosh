# Irosh

**Cryptographically-Identified P2P Secure Shell and Decentralized Data Transfer Protocol.**

[![Crates.io](https://img.shields.io/crates/v/irosh.svg)](https://crates.io/crates/irosh)
[![Documentation](https://docs.rs/irosh/badge.svg)](https://docs.rs/irosh)
[![License](https://img.shields.io/crates/l/irosh.svg)](#license)

Irosh is a high-assurance remote access toolset built on the Iroh networking stack. It provides a robust library and a streamlined CLI for establishing secure shell sessions and high-speed data transfers over encrypted, hole-punched QUIC streams. 

By leveraging Ed25519 identities, Irosh eliminates the need for public IP addresses, open ports, or complex VPN configurations, making it ideal for managing distributed infrastructure across restricted networks.

---

## Installation

### CLI Binary (Recommended)
For standard interactive usage, install the pre-compiled binary via the unified installer:

**Linux / macOS / Android (Termux)**:
```bash
curl -fsSL irosh.pages.dev/install | sh
```

**Windows (PowerShell)**:
```powershell
iwr irosh.pages.dev/ps | iex
```

### From Source
If you have the Rust toolchain installed:
```bash
cargo install irosh-cli
```

---

## Core Architecture

Irosh is designed for professional environments where security and resilience are paramount.

- **Identity-Based Routing**: Peer discovery and authentication are tied to a persistent Ed25519 secret key.
- **NAT Traversal**: Automatic hole-punching and relaying via the Iroh stack ensures connectivity in complex network topologies.
- **Ad-hoc Peer Discovery**: Secure, out-of-band trust establishment using short-lived pairing codes.
- **Service-Oriented**: Native support for background execution via system services (systemd, launchd).
- **Protocol Multiplexing**: Dedicated side-channels for metadata exchange and high-performance file synchronization.

---

## Quick Start

### 1. Host Initialization
Install the background service on the host machine to enable persistent access:
```bash
irosh system install
```

### 2. Peer Pairing
Generate a temporary pairing code for initial discovery:
```bash
irosh wormhole <custom-code>
```

### 3. Establishing a Session
Connect from a client machine using the pairing code or a previously saved ticket:
```bash
irosh <code-or-ticket>
```

---

## Interactive Escape Commands
During an active session, use the `~` prefix at the start of a line for local control:
- `~?` - View all available local commands.
- `~put [-r] <local> [remote]` - Upload a file or directory.
- `~get [-r] <remote> [local]` - Download a file or directory.
- `~C` - Open the irosh local command prompt.
- `~~` - Send a literal tilde character.

---

## Developer Integration (Library)

The `irosh` crate provides a low-level API for embedding P2P SSH capabilities into custom Rust applications.

### Add to project
```bash
cargo add irosh
```

### Implementation Example
```rust,no_run
use irosh::{Server, ServerOptions, StateConfig};

#[tokio::main]
async fn main() -> irosh::Result<()> {
    let state = StateConfig::new("./state".into());
    let options = ServerOptions::new(state);
    
    let (ready, server) = Server::bind(options).await?;
    println!("Node initialized. Ticket: {}", ready.ticket);
    
    server.run().await
}
```

---

## Protocol Comparison

| Feature | OpenSSH | Irosh |
| :--- | :--- | :--- |
| **Addressing** | IP / Hostname | Cryptographic Node ID |
| **Connectivity** | Static Ports (22) | NAT Hole-punching / QUIC |
| **Identity** | External Key Management | Built-in Ed25519 Secrets |
| **Trust Model** | Manual known_hosts | Trust On First Use (TOFU) |
| **Relay Support** | Manual ProxyJump | Native Global Relay Network |

---

## Documentation

- [**API Reference**](https://docs.rs/irosh) - Full technical documentation for library consumers.
- [**Architecture Roadmap**](docs/ROADMAP.md) - Internal design documents and future development plans.
- [**Security Overview**](docs/ROADMAP.md) - In-depth analysis of the Irosh trust and encryption model.

---

## License

Licensed under MIT or Apache-2.0.

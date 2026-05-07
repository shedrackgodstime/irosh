# Irosh

**Cryptographically-Identified P2P Secure Shell and Decentralized Data Transfer Protocol.**

[![Crates.io](https://img.shields.io/crates/v/irosh.svg)](https://crates.io/crates/irosh)
[![Documentation](https://docs.rs/irosh/badge.svg)](https://docs.rs/irosh)
[![License](https://img.shields.io/crates/l/irosh.svg)](#license)

Irosh is a high-assurance networking library built on the Iroh P2P stack. It enables developers to establish secure, encrypted, and hole-punched QUIC streams for remote shell access and binary data synchronization without requiring public IP addresses or open ports.

---

## Library Integration

The `irosh` crate is the core of the ecosystem, providing a high-level API for embedding P2P SSH capabilities into custom Rust applications.

### Add to project
```bash
cargo add irosh
```

### Server Implementation Example
```rust,no_run
use irosh::{Server, ServerOptions, StateConfig};

#[tokio::main]
async fn main() -> irosh::Result<()> {
    // 1. Initialize state and server options
    let state = StateConfig::new("./state".into());
    let options = ServerOptions::new(state);
    
    // 2. Bind the P2P server to the Iroh network
    let (ready, server) = Server::bind(options).await?;
    println!("Node initialized. Ticket: {}", ready.ticket);
    
    // 3. Execute the server loop
    server.run().await
}
```

---

## Core Architecture

Irosh is built for resilient environments where security and low-latency connectivity are paramount.

- **Identity-Based Routing**: Peer discovery and authentication are tied to a persistent Ed25519 secret key.
- **NAT Traversal**: Native hole-punching and relaying via the Iroh stack ensures connectivity across complex network topologies.
- **Protocol Multiplexing**: Dedicated side-channels for metadata exchange and high-performance file synchronization.
- **Service-Oriented**: Support for persistent background execution via native system services.

### Protocol Comparison

| Feature | OpenSSH | Irosh |
| :--- | :--- | :--- |
| **Addressing** | IP / Hostname | Cryptographic Node ID |
| **Connectivity** | Static Ports (22) | NAT Hole-punching / QUIC |
| **Identity** | External Key Management | Built-in Ed25519 Secrets |
| **Trust Model** | Manual known_hosts | Trust On First Use (TOFU) |
| **Relay Support** | Manual ProxyJump | Native Global Relay Network |

---

## Command Line Interface (CLI)

The `irosh-cli` package provides a reference implementation of the library for interactive usage.

### Installation

**Linux / macOS / Android (Termux)**:
```bash
curl -fsSL irosh.pages.dev/install | sh
```

**Windows (PowerShell)**:
```powershell
iwr irosh.pages.dev/ps | iex
```

### Quick Start
1. **Initialize Host**: `irosh system install`
2. **Enable Pairing**: `irosh wormhole <code-or-alias>`
3. **Connect**: `irosh <code-or-ticket>`

### Interactive Escape Commands
During an active session, use the `~` prefix for local control:
- `~put [-r] <local> [remote]` - Upload a file or directory.
- `~get [-r] <remote> [local]` - Download a file or directory.
- `~C` - Open the irosh local command prompt.

---

## Documentation

- [**API Reference**](https://docs.rs/irosh) - Full technical documentation for library consumers.
- [**Architecture Roadmap**](docs/ROADMAP.md) - Internal design and future development plans.
- [**Security Overview**](docs/ROADMAP.md) - Analysis of the trust and encryption model.

---

## License

Licensed under MIT or Apache-2.0.

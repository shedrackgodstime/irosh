# Irosh

**Secure SSH-like remote access without open ports, NAT issues, or public IPs.**

Powered by [Iroh](https://iroh.computer) peer-to-peer transport with built-in identity and TOFU (Trust On First Use) security.

---

## 🚀 Installation (CLI)

Install the `irosh`, `irosh-server`, and `irosh-client` binaries for standard interactive usage:

**Linux / macOS / Android (Termux)**:
```bash
curl -fsSL irosh.pages.dev/install | sh
```

**Windows (PowerShell)**:
```powershell
iwr irosh.pages.dev/ps | iex
```

---

## ⚡ Quick Start

1. **On the Remote Machine**: Run the server to generate a connection ticket:
   ```bash
   irosh-server --simple
   ```
2. **On Your Local Machine**: Connect using that ticket:
   ```bash
   irosh-client <TICKET>
   ```
3. **Inside the Shell**: Use the `:` prefix for local commands (like `:put`, `:get`, or `:help`).

---

## 💎 Why Irosh?

Traditional SSH is built for the "Server-Client" world of public IPs and open ports. **Irosh is built for the P2P world.**

- ✅ **No Open Ports**: Works entirely via P2P hole-punching. No firewall rules needed.
- ✅ **NAT Traversal**: Connect to machines behind home routers or strict corporate firewalls.
- ✅ **Flexible Authentication**: Supports zero-config Key/TOFU policies or Shared Password verification (with future aims for true OS System users).
- ✅ **Secure by Default**: Built-in QUIC encryption and Ed25519 peer identity.
- ✅ **Premium CLI UX**: Visual progress bars, persistent history, Tab completion, and `Ctrl+C` cancellation for transfers.
- ✅ **Fast & Stable**: Non-blocking I/O and lazy channel initialization for a snappy feel.

---

## 🛠 Developer Integration (Library)

The `irosh` crate is designed as a library first. Transport, protocol, and framing are strictly independent of CLI assumptions.

### 1. Add to your project
```bash
cargo add irosh
```

### 2. Implementation Example
```rust,no_run
use irosh::{Client, ClientOptions, SecurityConfig, Server, ServerOptions, StateConfig};

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Bind a P2P server
    let (ready, server) = Server::bind(
        ServerOptions::new(StateConfig::new("/tmp/irosh-server".into()))
    ).await?;

    tokio::spawn(server.run());

    // 2. Connect from a P2P client
    let mut session = Client::connect(
        &ClientOptions::new(StateConfig::new("/tmp/irosh-client".into())),
        ready.ticket().to_string().parse()?
    ).await?;

    // 3. Execute remote commands via Iroh transport
    session.exec("uname -a").await?;
    Ok(())
}
```

### 3. Feature Flags
- `server`: enables server-side API (includes PTY logic).
- `client`: enables client-side API.
- `storage`: enables trust and identity persistence.
- `transport`: enables Iroh transport and protocol types.

---

## ❓ How is this different from SSH?

| Feature | Standard SSH | Irosh |
| :--- | :--- | :--- |
| **Connectivity** | Requires Public IP/Port 22 | P2P Hole-punching (Anywhere) |
| **Identity** | Manual SSH Keys | Automatic Ed25519 Peer IDs |
| **Trust** | `known_hosts` | TOFU (Trust On First Use) |
| **Relays** | Requires Jump-host/VPN | True P2P (via Iroh) |

---

## 📚 Documentation & History

- [**Architecture**](docs/architecture.md): The separation of transport, session, and shell state.
- [**Security**](docs/security.md): Cryptographic TOFU access policy and host key pinning.
- [**Protocol**](docs/protocol.md): Custom side-stream framing for metadata and file transfers.
- [**Changelog**](CHANGELOG.md): Full history of project updates and releases.

---

## License

Licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).

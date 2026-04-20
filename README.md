# Irosh

SSH sessions over [Iroh](https://iroh.computer) peer-to-peer transport.

`irosh` is a Rust library crate that provides interactive remote shell access, one-off command execution, and secure file transfers over Iroh's hole-punching P2P transport layer. It comes with TOFU-style (Trust On First Use) identity and trust management built in.

## Usage as a Library

The `irosh` crate keeps transport, protocol, and framing strictly independent of CLI assumptions, making it ideal for embedding secure remote access directly into your applications.

```rust,no_run
use std::error::Error;
use tokio::task::JoinHandle;
use irosh::{Client, ClientOptions, SecurityConfig, Server, ServerOptions, StateConfig};

async fn run() -> Result<(), Box<dyn Error>> {
    let server_state = StateConfig::new("/tmp/irosh-server".into());
    let client_state = StateConfig::new("/tmp/irosh-client".into());

    let (ready, server) = Server::bind(
        ServerOptions::new(server_state).security(SecurityConfig::default()),
    )
    .await?;

    let _server_task: JoinHandle<irosh::Result<()>> = tokio::spawn(server.run());

    let target: irosh::Ticket = ready.ticket().to_string().parse()?;
    let mut session = Client::connect(
        &ClientOptions::new(client_state).security(SecurityConfig::default()),
        target,
    )
    .await?;

    // Execute a secure command over P2P without open ports or DNS
    session.exec("uname -a").await?;
    Ok(())
}
```

## Feature Flags

The crate uses feature flags so downstream consumers can compile only the
parts they need in order to keep binary sizes minimal.

- `server`: enables the server-side API (includes `portable-pty` and PTY logic)
- `client`: enables the client-side API
- `storage`: enables trust, peer, and identity persistence
- `transport`: enables Iroh transport and protocol types

The default feature set enables both the client and server sides.

---

## For End Users: Command-Line Tools

The `irosh` repository includes a companion `cli/` crate that provides the `irosh`, `irosh-server`, and `irosh-client` binaries for standard interactive usage.

### Linux / macOS / Android (Termux)
```bash
# Install everything
curl -fsSL irosh.pages.dev/install | sh

# Install SERVER only
curl -fsSL irosh.pages.dev/install | sh -s -- server

# Install CLIENT only
curl -fsSL irosh.pages.dev/install | sh -s -- client
```

### Windows (PowerShell)
```powershell
# Install everything
iwr irosh.pages.dev/ps | iex
```

### Quick Start
1. Run `irosh-server --simple` on the remote machine. Copy the `Ticket`.
2. Run `irosh-client <TICKET>` on the local machine.
3. Access secure remote shell (run `:help` while inside the shell to see file transfer commands).

---

## Technical Documentation

Detailed manuals for the `irosh` architecture and protocols are generated in the `docs/` directory:

- [`docs/architecture.md`](docs/architecture.md): The separation of transport, session, and shell state.
- [`docs/security.md`](docs/security.md): The cryptographic TOFU access policy and host key pinning.
- [`docs/protocol.md`](docs/protocol.md): Custom side-stream framing for peer metadata and file transfers.

---

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.

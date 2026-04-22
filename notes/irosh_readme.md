# Irosh

**Secure SSH-like remote access without open ports, NAT issues, or public IPs.**

Powered by Iroh peer-to-peer transport with built-in identity and TOFU (Trust On First Use) security.

---

## Why Irosh?

Traditional SSH requires:
- Open ports
- Public IP or port forwarding
- Firewall configuration

`irosh` removes all of that:

- ✅ No open ports  
- ✅ Works behind NAT  
- ✅ Peer-to-peer encrypted transport  
- ✅ Built-in identity and trust (TOFU)  

---

## Quick Demo

On remote machine:
```bash
irosh-server --simple
```

On your machine:
```bash
irosh-client <TICKET>
```

That’s it. No ports. No config. No SSH keys.

---

## Usage as a Library

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

    // No SSH server, no port forwarding, no public IP required
    session.exec("uname -a").await?;
    Ok(())
}
```

---

## Feature Flags

- `server`: enables server-side API  
- `client`: enables client-side API  
- `storage`: enables persistence  
- `transport`: enables Iroh transport  

Default includes both client and server.

---

## Use Cases

- Remote server access behind NAT  
- Secure home lab access  
- Temporary debugging sessions  
- File transfer without exposing services  

---

## Installation

### Linux / macOS / Android (Termux)
```bash
curl -fsSL irosh.pages.dev/install | sh
```

### Windows (PowerShell)
```powershell
iwr irosh.pages.dev/ps | iex
```

> ⚠️ Always review install scripts before running them.

---

## How is this different from SSH?

- No daemon required  
- No port exposure  
- No centralized relay dependency  
- Built on peer-to-peer transport  

---

## Documentation

- docs/architecture.md  
- docs/security.md  
- docs/protocol.md  

---

## License

MIT or Apache-2.0

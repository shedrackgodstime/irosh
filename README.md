# Irosh

**Secure P2P SSH. No IP addresses. No open ports. No VPNs.**

[![Crates.io](https://img.shields.io/crates/v/irosh.svg)](https://crates.io/crates/irosh)
[![Documentation](https://docs.rs/irosh/badge.svg)](https://docs.rs/irosh)
[![License](https://img.shields.io/crates/l/irosh.svg)](#license)

Irosh lets you connect to any machine using just its name, even if it's behind a firewall. It handles all the networking for you using the Iroh P2P stack, so you never have to worry about IP addresses or port forwarding again.

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

---

## Quick Start

### 1. On the Host (Server)
Start the background service and generate a 3-word pairing code:
```bash
irosh system install    # Install as background service
irosh wormhole          # Get your pairing code (e.g. apple-pie-sunset)
```

### 2. On the Client
Connect from anywhere using the code:
```bash
irosh apple-pie-sunset
```
*That's it. You're connected.*

---

## Why use Irosh?

- **Zero-IP Connectivity**: Connect to your devices without needing a public IP or DNS.
- **Native & Standalone**: Unlike other tools, Irosh is a full SSH server. It doesn't need OpenSSH installed.
- **Human-Friendly**: Pair devices with simple words. Irosh automatically saves them with friendly names like `my-laptop`.
- **Integrated File Transfer**: Move files instantly with built-in `put` and `get` commands.
- **Global Roaming**: Stay connected even when switching between Wi-Fi and mobile data.

---

## Interactive Toolkit

During an active session, type these commands at the start of a line (after pressing Enter):

| Command | Description |
| :--- | :--- |
| `~.` | Disconnect immediately |
| `~put <local> [remote]` | Upload a file or directory to the peer |
| `~get <remote> [local]` | Download a file or directory from the peer |
| `~C` | Open the **irosh command prompt** for session management |
| `~?` | View help and all available escape sequences |

---

## Build from Source (Cargo)

If you have the Rust toolchain installed:
```bash
cargo install irosh-cli
```

---

Irosh is a solo-driven effort with a big vision for the future of the P2P internet. If you are passionate about Rust, P2P networking, or high-assurance security, your contributions are more than welcome! Feel free to open an issue or reach out if you're interested in collaborating.

---

## Important Disclaimer & Liability

Irosh is a powerful remote access tool. By using this software, you agree to the following:

1.  **"As-Is" Basis**: This software is provided "as is" without warranty of any kind, either express or implied. 
2.  **User Responsibility**: You are solely responsible for your actions. This tool must only be used for authorized access to systems you own or have explicit permission to manage.
3.  **Non-Liability**: The author(s) and contributors shall **not be held responsible** for any misuse, damage, data loss, or legal consequences resulting from the use of this software. 
4.  **Experimental Nature**: This project is in early development and has not been formally audited. Security and stability are not guaranteed.

---

## Architecture

Irosh is built as a **"Fat Library"**. All the networking, security, and SSH logic lives in the `irosh` crate, while the CLI is a thin, high-performance UI layer.

- [**Technical Manual (Library)**](src/README.md) - For developers building on Irosh.
- [**Development Roadmap**](docs/ROADMAP.md) - Our path to v1.0 and beyond.
- [**Changelog**](CHANGELOG.md) - What's new in v0.3.0.

---

## License

Licensed under MIT or Apache-2.0.

# Architecture

The `irosh` library is a remote access toolkit that bridges the reliability of SSH with the zero-configuration connectivity of the Iroh peer-to-peer (P2P) network.

This document describes the high-level architecture of the `irosh` crate.

## Core Concepts

1. **SSH over Iroh**: `irosh` does not invent a new shell protocol. It routes standard SSH bytes (via the `russh` crate) over an Iroh P2P connection. This means you get standard SSH security, PTY semantics, and multiplexing, but without needing IP addresses, open ports, or DNS.
2. **Library-First**: `irosh` is fundamentally a library. The unified `irosh` CLI binary is a thin wrapper over the library's `Client` and `Server` structs, using subcommands to expose each capability.
3. **Bring Your Own I/O**: The library handles transport, encryption, and protocol framing. The caller (e.g., the CLI code) is responsible for wiring up local `stdin`/`stdout` and handling terminal raw modes. The library never writes to the console.

## Module Layout

| Module | Responsibility |
|:---|:---|
| `transport/` | Connects endpoints via Iroh hole-punching. Manages raw `AsyncRead`/`AsyncWrite` streams, parses connection Tickets, and defines protocol framing for side-channels. |
| `session/` | Manages PTY types, terminal resizing logic, and signal forwarding structs. |
| `storage/` | Manages key persistence and the "Trust On First Use" (TOFU) trust records. |
| `client/` | Exposes the `Client` builder for establishing a connected `Session`. Handles client-side SSH negotiation and transfer request initiation. |
| `server/` | Exposes the `Server` listening loop. Handles server-side SSH accept logic, spawning child shells, and executing transfer directives inside appropriate Linux namespaces. |
| `config/` | Pure data types for state paths and security policies. |
| `error/` | A unified `IroshError` facade that wraps specific, granular subsystem errors (e.g. `ClientError`, `ServerError`, `TransportError`). |

## The Multiplexed Strategy

Because P2P connections are valuable, `irosh` opening a single Iroh connection between two peers and then multiplexing multiple logical streams over it:

1. **The Primary SSH Stream**: Exactly one bidirectional stream carrying standard SSH traffic for the interactive shell.
2. **The Metadata Stream**: A dedicated control stream opened upon connection to exchange node metadata (hostname, OS, etc.) so clients can automatically suggest aliases.
3. **Transfer Streams**: Every file transfer (put/get) opens an ephemeral, authenticated side-stream. This keeps binary file data completely out of the PTY terminal flow, preventing screen corruption and ensuring high performance.

## Feature Flags

To ensure minimal binary size for consumers, `irosh` uses granular feature flags:

- `server`: Includes the `russh` server backend and portable-pty execution code. 
- `client`: Includes the `russh` client backend and session event machinery.
- `storage`: Enables local key generation, trust tracking, and peer profiles.
- `transport`: Enables the Iroh peer-to-peer networking stack.

Consumers only pay for the code they use. While the official unified CLI binary includes all features for convenience, a custom implementation can choose to compile only the client or server logic to minimize footprint.

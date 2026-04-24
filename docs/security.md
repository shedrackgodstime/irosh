# Trust and Security Model

`irosh` combines the cryptographic identity primitives of the Iroh P2P framework with the proven application-layer security of SSH.

## Ed25519 Cryptographic Identity

Every `irosh` node (server or client) is identified by a unique, persistent Ed25519 keypair. 

When you start `irosh` for the first time, an identity key is generated and stored locally in the node's state path. The public half of this key (the "Node ID") acts as the root of identity on the P2P network.

## The Connection Ticket

To connect to a server, clients require a `Ticket`. A Ticket is an opaque, serialized string containing:
1. The Server's cryptographic Node ID.
2. The Server's Relay URL (for hole-punching fallback).
3. Direct IPv4/IPv6 hints (if available).

Because the Ticket contains the Server's Node ID, the underlying transport automatically enforces that the client is talking to the correct cryptographic peer. Connection attempts to a node with a mismatched ID are dropped at the transport layer.

## Trust On First Use (TOFU)

`irosh` enforces a strict **Trust On First Use (TOFU)** policy for application payload access, preventing Man-In-The-Middle (MITM) attacks and unauthorized client connections.

### Host Key TOFU (Client-Side)
1. **First Connection**: The first time a client connects to a server's Node ID, it records the server's SSH Host Key fingerprint in its local Trust Store (acting similarly to `known_hosts`).
2. **Subsequent Connections**: The client compares the presented Host Key against the pinned record. If the key changes, the connection is instantly rejected with a `SessionState::TrustMismatch` error.
3. **Strict Policy**: By default, the `HostKeyPolicy::Strict` configuration refuses connections to untrusted nodes, requiring an explicit override mechanism (handled via CLI flags) to update the pinned key.

### Client Key TOFU (Server-Side)
Servers must protect against unauthorized remote execution, even if an attacker discovers the connection Ticket.
1. **First Connection**: The first time a new client connects, the server records the client's public identity key in its Authorized Clients list. Depending on the server configuration, this initial connection may be interactively prompted or implicitly accepted.
2. **Authentication**: All subsequent connections require the client to successfully complete standard `publickey` SSH authentication against the pinned record. Clients presenting unknown keys are rejected with `SessionState::AuthRejected`.

## Authentication Modes & Future Aims

Irosh uses a pluggable authentication architecture via the `Authenticator` trait. Currently, the built-in modes support **Key-Only (TOFU)**, **Shared Password**, and **Combined** logic.

**Future Aim: Per-User System Authentication (PAM)**
While not currently implemented in the CLI, the architecture is designed to eventually support true per-user logins (e.g., `irosh-client <ticket> --user alice`) backed by the host Operating System's native users (PAM on Linux/macOS). 
The goal of this future `SystemAuth` backend is to allow Enterprise and multi-user environments to authenticate exactly like traditional `ssh user@host`, where adding a system user immediately grants them access to Irosh without managing virtual passwords or separate `.htpasswd`-style files.

## Local State and Secrets

All key material is stored on the local filesystem.
- `identity.secret`: Contains the Ed25519 secret seed.
- `trust_store/`: Contains the pinned server and client profiles.

The library assumes the local filesystem is secure. It restricts filesystem permissions on creation (e.g. `chmod 600` on Unix), but local OS-level isolation is ultimately responsible for preventing local privilege escalation.

# Implementation Plan: Irosh Wormhole (Secure Ad-hoc Pairing)

This plan details the technical steps to implement the **Wormhole** feature described in [WORMHOLE_DESIGN.md](file:///home/kristency/Projects/irosh/WORMHOLE_DESIGN.md).

## 1. IPC Layer (Daemon Control)
To allow the CLI to communicate with a running background service, we need a local IPC mechanism.

- **Technology**: Unix Domain Sockets on Linux/macOS, Named Pipes on Windows.
- **Protocol**: Simple JSON-based RPC over the socket.
- **Commands**:
    - `EnableWormhole { code, password, persistent }`
    - `DisableWormhole`
    - `GetStatus` (Includes wormhole status)

### Steps:
1.  **Create `src/server/ipc.rs`**: Implement the listener and command handling logic.
2.  **Integrate with `Server::run`**: Start the IPC listener task alongside the SSH/Iroh listener.
3.  **Create `src/client/ipc.rs`**: Implement the client side for the CLI to send commands.

## 2. Rendezvous Logic (Iroh Gossip)
The wormhole uses Iroh Gossip to exchange tickets without manual copying.

- **Topic**: `HMAC-SHA256(code, "irosh-wormhole-v1")` for privacy.
- **Message**: The server's connection ticket.

### Steps:
1.  **Add `iroh-gossip` dependency**: If not already included in `iroh` bundle.
2.  **Server side**:
    - When wormhole is enabled, join the gossip topic.
    - Periodically (or on request) broadcast the `ServerReady` ticket.
3.  **Client side**:
    - When `irosh connect <3-word-code>` is run, `parse_target` detects the code pattern.
    - Client joins the gossip topic, waits for a ticket, then proceeds with the connection.

## 3. Pairing & Security (The "Booster Rocket")
Ensure the wormhole is a one-time pairing event that seeds a permanent trust relationship.

### Steps:
1.  **Server Authorization**:
    - Use dedicated ALPN: `irosh/pairing/v1`.
    - If a connection comes in via this ALPN, trigger the "Pairing" flow.
    - **Confirmation**: If interactive, prompt user to accept the peer ID.
    - Validate the session password if provided (for persistent wormholes).
2.  **Automatic Key Exchange**:
    - On successful connection and confirmation, the server automatically adds the client's public key to `authorized_keys`.
    - **Auto-Burn**: Disable the wormhole immediately after successful pairing or 3 failed attempts.

## 4. CLI Implementation
Update the `irosh` CLI to expose these features.

- **`irosh system wormhole`**:
    - Generates a random 3-word code (using `rand` and a wordlist).
    - Sends the `EnableWormhole` command to the daemon.
- **`irosh connect <code-pattern>`**:
    - Update `Client::parse_target` to handle word-based codes.

## 5. Timeline & Milestones
1.  **Milestone 1**: Functional IPC layer and `irosh system wormhole` CLI.
2.  **Milestone 2**: Gossip-based discovery (client finds server via code).
3.  **Milestone 3**: "Trust-Seed" pairing (automatic key addition).

---

> [!IMPORTANT]
> The IPC socket path should be secure (restricted to the user's home directory or protected by file permissions).

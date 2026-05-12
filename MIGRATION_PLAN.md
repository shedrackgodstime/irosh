# Migration Plan: Iroh 1.0.0-rc.0 & Netwatch Cleanup

## Objective
Upgrade the core networking stack to **Iroh 1.0.0-rc.0** (released May 11, 2026) to leverage improved Android/Termux support, stabilized APIs, and a more modular crate structure. Simultaneously, remove the vendored `netwatch` dependency and replace it with the upstream version.

## Rationale
*   **API Stability:** Iroh 1.0 marks a major milestone. Migrating now avoids multiple breaking changes after the first official release of `irosh`.
*   **Android Compatibility:** Iroh 1.0.0-rc.0 includes dedicated testing and fixes for Android (PR #4183), potentially eliminating the need for our custom `netwatch` patch.
*   **Modularity:** The new "Router" model allows for better separation of protocols (SSH, Gossip, etc.).

---

## 1. Dependency Updates
*   Update `iroh`, `iroh-gossip`, and `iroh-tickets` to `1.0.0-rc.0` in the workspace `Cargo.toml`.
*   Remove the `[patch.crates-io]` for `netwatch` and delete the `vendor/netwatch` directory.
*   Add `iroh-base` if needed for common types.

## 2. Core Terminology Refactor (Global Rename)
The word "Node" is removed from core networking in favor of "Endpoint".
*   `NodeId` -> `EndpointId`
*   `NodeAddr` -> `EndpointAddr`
*   `NodeTicket` -> `EndpointTicket` (Internal to `iroh-tickets`)
*   `endpoint.node_id()` -> `endpoint.id()`
*   `endpoint.node_addr()` -> `endpoint.addr()`

## 3. Core Refactoring (`src/transport/iroh.rs`)
*   **Endpoint Binding:** Update `bind_server_endpoint` and `bind_client_endpoint` to use the 1.0 `Endpoint::builder()`.
*   **Router Implementation:** Replace any legacy `Node` logic with `iroh::protocol::Router`. 
*   **Protocol Registration:** Explicitly register the "irosh" SSH protocol and "iroh-gossip" with the Router using ALPNs.
*   **Discovery:** Ensure `EndpointId` discovery (formerly `NodeId`) works via the new discovery primitives.

## 4. Connection Logic Refactor (`src/client/connect.rs`)
*   **Infallible Accessors:** Remove error handling for `connection.remote_id()` as it is now infallible.
*   **Stateful Connections:** Update `establish_session` to handle any changes in the `Connection` type parameters.

## 5. Metadata & Gossip Refactor
*   **Gossip Crate:** Update to the standalone `iroh-gossip` 1.0.
*   **Router Integration:** Verify how `iroh-gossip` plugs into the `Router` for ALPN dispatching.

---

## 6. Value-Add Integration (New Features)

Beyond the basic migration, Iroh 1.0 provides several primitives that can directly improve `irosh`:

### A. Real-time Connectivity UI (Path Observation)
*   **Feature:** Use the new `Connection::path_events()` API.
*   **Improvement:** In the CLI, show a live indicator when a connection transitions from **Relay** to **Direct (P2P)**. This gives users immediate feedback on connection quality.

### B. Optimized File Transfer (iroh-blobs)
*   **Feature:** Transition from custom file transfer logic to `iroh-blobs`.
*   **Improvement:** 
    -   **Verified Transfers:** Automatic BLAKE3 hashing of all transferred data.
    -   **Resumable Transfers:** Support for resuming interrupted uploads/downloads.
    -   - **Performance:** Optimized for large directory trees and high-latency mobile networks.

### C. Discovery Service Refactor
*   **Feature:** Leverage new discovery primitives.
*   **Improvement:** Potential to replace or augment the "Wormhole" rendezvous with DHT-based discovery for persistent servers, making them discoverable by name without a full ticket.

### D. Relay Authentication
*   **Feature:** Support for Relay Auth Tokens.
*   **Improvement:** Allow users to configure private relay infrastructure with authentication for their `irosh` fleet.

### E. Server Architecture Simplification (Router Model)
*   **Feature:** Use `iroh::protocol::Router`.
*   **Improvement:** 
    -   Replace the complex manual `select!` loop in `src/server/mod.rs` with the `Router`.
    -   Encapsulate SSH/PTY logic into a dedicated `SshProtocol` implementing `ProtocolHandler`.
    -   Cleaner separation of concerns and easier addition of future protocols.

---

## 7. Verification & Testing
*   **Local Tests:** Run all unit and integration tests (`cargo test`).
*   **Android/Termux Validation:** Verify that `irosh` still works on Android without the `netwatch` patch.
*   **Performance Check:** Ensure hole-punching and relay fallback remain performant.

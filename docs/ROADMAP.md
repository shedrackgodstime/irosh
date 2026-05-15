# Irosh Development Roadmap (v0.3.0 -> v1.0)

The primary goal of the v0.3.0 series is to achieve **Production Stability** and **OS-Native Integration** while maintaining the high-performance "Fat Library" architecture.

## Phase 1: Feature Parity (v0.2.x) - [COMPLETE]
Restored professional-grade CLI features and unified the library architecture.
*   *** Authentication Parity**: Unified authenticator with multi-mode support.
*   *** Terminal Fidelity**: RAII-based `TerminalGuard` and robust Windows ConPTY handling.
*   *** Interactive Escapes**: Full `~` command mode with history and completion.
*   *** Wormhole Hardening**: Pkarr-based rendezvous and rate-limiting.

## Phase 2: OS Integration (v0.3.0) - [COMPLETE]
Expanding the "Service-Oriented" nature of Irosh across all major platforms.
*   *** Native Service Managers**: Cross-platform installers for systemd, launchd, and Windows Task Scheduler.
*   *** Service Diagnostics**: Unified `system status` command with real-time daemon health.
*   *** Storage Hardening**: High-assurance Windows ACLs and atomic secure writes.
*   *** IPC Lifecycle**: Synchronized shutdown of control listeners and session tasks.

## Phase 3: Stabilization & Polish (v0.3.x) - [CURRENT]
Focusing on developer experience, documentation, and performance edge cases.
*   **Optimized Binary**: Reduce binary size and further prune unused transitive dependencies.
*   **Audit & Documentation**: Complete the technical manual and finalize public API documentation for library users.
*   **P2P-Native Updates**: Implement the secure binary update flow via Iroh's blob transport.
*   **Diagnostics Extension**: Add NAT traversal debugging and peer latency metrics to `system status`.

## Phase 4: Production Readiness (v1.0)
Finalizing the protocol and committing to API stability.
*   **Stability Freeze**: Finalize the IPC and pairing protocol versions.
*   **Security Audit**: Independent review of the authentication and namespace-joining logic.
*   **Cloud Relay Fleet**: Deployment of a global relay network for zero-config internet traversal.

---

> [!NOTE]
> For our long-term vision and advanced P2P concepts (Collaborative SSH, Mobile support, AI agents), please see the [Future Roadmap](future_roadmap.md).

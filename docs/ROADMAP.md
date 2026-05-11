imidiate... when connecting with wormhole, when the server ticket is found save it as temp peer just the connection didnt went through and the wormhole already died user can connect to it again with the saved ticket  instead of opening wormhole again... remember wormhole is just to easily get the ticket without manually copy pasting it......
# Irosh Development Roadmap (v0.2.x -> v1.0)

The primary goal of the v0.2.x series is to achieve **Production Stability** and **Feature Parity** with the legacy Irosh MVP while maintaining the new "Fat Library" architecture.

## Phase 1: Feature Parity & Stability (v0.2.5)
The immediate focus is restoring professional-grade CLI features that were temporarily simplified during the V2 refactor.

*   **Authentication Parity**: Restore `--auth-mode` (key, password, combined) and `--authorize <key>` flags to the `host` command.
*   **Terminal Fidelity**: Implement full VT100 support for Windows consoles and non-blocking `AsyncStdin` for Unix.
*   **Interactive Escapes**: Restore the `~.` (disconnect), `~c` (command mode), and `~get`/`~put` (file transfer) escape sequences.
*   **Wormhole Hardening**: Enforce 8-character safety minimums and improve rendezvous reliability.

## Phase 2: OS Integration (v0.3.0)
Expanding the "Service-Oriented" nature of Irosh across all major platforms.

*   **Native Service Managers**: Complete the implementation of the Windows Task Scheduler and macOS launchd installers.
*   **Service Diagnostics**: Add a unified `system status` command to report the health of background P2P daemons.
*   **Auto-Update**: Implement a secure P2P-native update mechanism for the CLI binary.

## Phase 3: Path to v1.0
Hardening and auditing the codebase for mission-critical usage.

*   **Security Audit**: Review of the authentication handshake and trust store persistence logic.
*   **Performance Optimization**: Tuning QUIC congestion control for high-latency P2P links.
*   **API Stabilization**: Finalizing the `irosh` library API for 1.0 stability.

---

> [!NOTE]
> For our long-term vision and advanced P2P concepts (Collaborative SSH, Mobile support, AI agents), please see the [Future Roadmap](future_roadmap.md).

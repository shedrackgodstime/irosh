# Irosh: The Definition

Irosh is a high-assurance, peer-to-peer secure shell (SSH) and data transfer protocol. It is designed to provide "Magic Connectivity"  -  the ability to establish secure, encrypted shell sessions between any two devices on the planet without requiring public IP addresses, open ports, or complex VPNs.

---

##  Core Identity: "Native P2P"

The fundamental difference between Irosh and other P2P-SSH attempts is its **Native Autonomy**. 

*   **Standalone Server**: Most P2P-SSH tools are "proxies" that tunnel traffic to a local OpenSSH server. **Irosh is the server.** It uses a native Rust SSH implementation (`russh`), allowing it to run on minimal systems (Android, light containers) where OpenSSH is not available.
*   **Identity-First Routing**: Irosh replaces IP addresses with **Ed25519 Node Identities**. You don't connect to an IP; you connect to a *Trusted Identity*.
*   **The "Fat Library" Philosophy**: Irosh is built as a robust library first. The CLI is simply a professional UI layer over a high-performance networking engine.

---

##  Key Differentiators

### 1. Wormhole Discovery (Human-Centric)
Instead of forcing users to copy-paste 64-character cryptographic hex strings, Irosh uses **Wormholes**. 
*   **Codes**: Simple 3-word pairing codes (e.g., `apple-pie-sunset`).
*   **Zero-Config Auto-Save**: Once connected, Irosh automatically resolves the peer's identity (e.g., `user-linux`) and saves it to your address book silently.

### 2. Sidecar Transfer Protocols
Inside the secure QUIC tunnel, Irosh runs a dedicated metadata and data-transfer multiplexer.
*   **`~put` / `~get`**: These are not external SCP/SFTP calls. They are native side-streams within the same P2P session, allowing for high-speed file synchronization without breaking the terminal state.

### 3. Service-First Architecture
Irosh is designed to be a permanent part of your OS.
*   **Background Services**: Native installers for `systemd`, `launchd`, and **Windows Task Scheduler**.
*   **Status IPC**: A dedicated control channel that allows the CLI to query the background daemon for real-time health and connection metrics.

---

##  Security Model

Irosh implements a "Unified Authenticator" that provides multiple layers of defense:
*   **TOFU (Trust On First Use)**: Standard SSH-style key pinning.
*   **Strict Identity**: Connections from unknown keys are rejected by default unless a temporary "Wormhole" or "Invite Password" is active.
*   **Stealth Mode**: Ability to run the P2P endpoint with a secret key, making it invisible to anyone who doesn't possess the pre-shared secret.

---

##  Platform Support Matrix

| Platform | Terminal Engine | Service Manager | PTY Support |
| :--- | :--- | :--- | :--- |
| **Linux** | Standard TTY | systemd | Native (`forkpty`) |
| **Windows** | ConPTY | Task Scheduler | Windows Pseudo Console |
| **Android** | Termux / Service | Background Intent | Native (Stubs provided) |
| **macOS** | Standard TTY | launchd | Native (`forkpty`) |

---

##  The Vision: Beyond the Shell

Irosh is not just a tool for today; it is the foundation for a **Parallel P2P Infrastructure**.
*   **V1.0 Goal**: 100% parity with OpenSSH features (Port Forwarding, Agent Forwarding).
*   **V2.0 Goal**: Distributed "Mesh" coordination where nodes can autonomously find and secure each other across a global gossip network.

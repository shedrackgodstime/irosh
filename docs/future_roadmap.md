# Irosh Future Roadmap: V3 & Beyond

This document outlines the strategic roadmap for Irosh after the V2 migration is complete. The goal is to achieve full feature parity with OpenSSH while leveraging the unique advantages of the Iroh P2P transport.

## 1. OpenSSH Escape Sequence Parity
To provide a familiar environment for sysadmins, Irosh will implement the standard SSH escape character logic (triggered by `~` after a newline).

### Supported Escape Sequences
*   `~.` - Terminate connection immediately.
*   `~B` - Send a BREAK to the remote system.
*   `~C` - Open the Irosh Command Line (see below).
*   `~R` - Request rekeying of the session.
*   `~V/v` - Decrease/increase verbosity (LogLevel) on the fly.
*   `~^Z` - Suspend the irosh process.
*   `~#` - List currently active forwarded connections.
*   `~&` - Background irosh (when waiting for connections to terminate).
*   `~?` - Show the escape sequence help message.
*   `~~` - Send a literal escape character by typing it twice.

### Interactive Command Line (`~C`)
Opening the command line will provide an `irosh> ` prompt for dynamic session management:
*   `-L[bind_address:]port:host:hostport` — Request local forward.
*   `-R[bind_address:]port:host:hostport` — Request remote forward.
*   `-D[bind_address:]port` — Request dynamic SOCKS forward.
*   `-KL[bind_address:]port` — Cancel local forward.
*   `-KR[bind_address:]port` — Cancel remote forward.
*   `-KD[bind_address:]port` — Cancel dynamic forward.

---

## 2. Professional Protocol Support

### SFTP & SCP Integration
*   **Subsystem Support**: Implement the SSH SFTP subsystem to allow standard file transfer clients to work over Irosh.
*   **Local SSH Proxy**: Launch a local listener that third-party apps (FileZilla, VS Code) can connect to, which then tunnels traffic over the P2P connection.
*   **SCP Emulation**: Support the legacy SCP protocol by piping to the remote `scp` binary for quick one-off transfers.

### SSH Agent Forwarding (`-A`)
*   Allow remote servers to use the client's local SSH agent for secure authentication to secondary servers (e.g., GitHub, internal git servers) without copying private keys.

### Jump-Host / ProxyJump (`-J`)
*   Automated multi-hop P2P negotiation. Chain multiple Irosh nodes together to reach isolated internal networks.
*   Example: `irosh connect -J gateway-node target-internal-node`

---

## 3. Advanced Networking
*   **Reverse Port Forwarding (`-R`)**: Expose local development servers to a remote peer via the secure P2P tunnel (similar to a P2P-native `ngrok`).
*   **Dynamic SOCKS Proxy (`-D`)**: Create a secure P2P "VPN" by routing all system or browser traffic through a remote Irosh peer.
*   **Multiplexing**: Support multiple terminal sessions and file transfers over a single persistent P2P connection to reduce handshake overhead.
## 4. Advanced Concepts (Irosh-Exclusive)
Features that leverage the unique peer-to-peer nature of the Iroh transport.

### Collaborative SSH (Shared Sessions)
*   Allow multiple users to join a single terminal session for pair programming or remote troubleshooting.
*   P2P implementation similar to `tmate` but without the need for a central relay server.

### P2P Name Discovery
*   Move beyond static tickets. Implement a private discovery layer where peers can be reached by human-readable names (e.g., `irosh connect dev-server`) using Iroh's gossip and DHT capabilities.
## 5. Next-Gen Networking (The "Irosh" Ecosystem)
Long-term vision for a complete P2P infrastructure management suite.

### Irosh TUI (Terminal Dashboard)
*   A comprehensive Terminal User Interface for managing peer lists, active tunnels, and real-time P2P metrics (latency, bandwidth, NAT type).

### QR-Code Physical Pairing
*   Use QR codes printed in the terminal for instant, secure trust establishment between mobile devices and servers without manual ticket exchange.

### P2P Dead-Drop & Secrets
*   Encrypted, asynchronous message and secret delivery. Leave configurations or sensitive data for a peer to retrieve the next time they join the gossip network.

### Git-over-Irosh
*   A custom git-remote helper (`irosh://`) to enable seamless, keyless git operations over P2P transport.

### Network Pulse
*   A health-check dashboard that monitors the availability and security status of an entire network of trusted Irosh nodes.
## 6. The "God-Mode" Horizon
High-concept engineering targets for the next generation of Irosh.

### AI-Powered P2P Diagnostics
*   Integrated local LLM agents to analyze QUIC congestion, NAT probes, and network logs to provide human-readable troubleshooting and automatic optimization.

### Multi-Path QUIC (Aggregation)
*   Simultaneous usage of multiple network interfaces (Wi-Fi + Cellular + Ethernet) for increased bandwidth and zero-latency failover.

### Irosh-to-Legacy Gateway
*   Acting as a P2P-to-SSH bridge, allowing users to reach legacy infrastructure that lacks Irosh by tunneling through a single Irosh gateway node.

### Ephemeral Remote Environments
*   One-click spawning of sandboxed, remote Docker/Podman environments via P2P commands, providing temporary workspaces that vanish on disconnect.

### Decentralized Web-of-Trust (DPKI)
*   A blockchain-free, DHT-based identity system where trust is propagated through peer-to-peer vouchers, eliminating the need for central Certificate Authorities.
## 7. The Autonomous Peer (AI-First Architecture)
Transforming Irosh into a "P2P Nervous System" where AI agents can operate the network autonomously.

### Natural Language Transport
*   An integrated intent parser that allows users to perform complex P2P operations via simple instructions.
*   Example: *"Move the latest logs from the staging server to the analytics cluster."*

### Machine-Readable CLI (Agent-First)
*   Strict, first-class JSON output for every command to ensure AI agents can parse state and execute logic with 100% reliability.

### Self-Healing Mesh Coordination
*   Node-resident agents that communicate via the Iroh Gossip network to detect failures, optimize routing, and coordinate software updates across the entire mesh without human intervention.

### Autonomous Security & Quarantine
*   Real-time AI monitoring of session behavior across the P2P network. Ability to automatically detect and isolate compromised nodes by revoking their P2P trust vouchers in milliseconds.

### Distributed Task Orchestration
*   The ability for an agent to manage complex, multi-node workflows (e.g., rolling updates, distributed database backups) as a single high-level intent.
## 8. Distributed P2P Compute (Irosh-Compute)
Moving from remote access to remote execution and resource sharing.

### P2P Task Offloading
*   Seamlessly offload heavy computational tasks (compilation, rendering, data processing) from local devices to powerful remote peers with a single command.
*   Automatic synchronization of source code and build artifacts over QUIC side-streams.

### Distributed "Irosh-Lambda"
*   Executing stateless functions across a mesh of peers in parallel. Enables network-wide operations like log analysis or security audits to be performed locally on each node and aggregated centrally.

### Idle Resource Virtualization
*   Allowing mobile or low-power devices to "borrow" the CPU/GPU resources of trusted peers, effectively creating a virtualized, P2P-powered hardware abstraction layer.

### Hardware-Isolated Sandboxing
*   Enforcing strict security boundaries for remote tasks using lightweight micro-VMs (Firecracker) or WebAssembly (Wasm) runtimes to ensure host integrity during compute offloading.

### Embedded Telemetry
*   Background monitoring frames that provide real-time CPU/RAM/IO stats of the remote host directly within the `irosh> ` command prompt or a dedicated status bar.

### Hardware Security (FIDO2/WebAuthn)
*   First-class support for YubiKeys, TouchID, and Windows Hello for multi-factor authentication on every P2P connection.

### Mobile & Roaming
*   Native mobile applications (Android/iOS) that leverage Iroh's superior roaming capabilities, ensuring SSH sessions stay alive when switching between Wi-Fi and Cellular data.

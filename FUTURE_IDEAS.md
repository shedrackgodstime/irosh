# Irosh: Future Roadmap & Ideas

This document tracks premium features and experimental ideas for the Irosh P2P SSH platform.

## 🚀 Interactive UI & UX

### 1. Interactive Peer Selector (Planned)
- **Status**: Implementing
- **Description**: Allow running `irosh` without arguments to trigger a searchable `dialoguer` menu of saved peers.
- **Goal**: Reduce friction for frequent connections.

### 2. Live TUI Dashboard (`irosh status --live`)
- **Description**: A `ratatui`-based terminal dashboard for the `host` mode.
- **Features**:
    - Real-time bandwidth monitoring.
    - Active session list with connection duration and metadata.
    - Visual indicators for NAT traversal type (Relay vs. Direct).
    - Scrolling system logs.

## 📁 Enhanced File Management

### 3. "Magic" One-Shot Transfer (`irosh send` / `irosh receive`)
- **Description**: Wormhole-style one-time file transfers.
- **Workflow**:
    - `irosh send <file>`: Creates a temporary host, displays a short ticket/code.
    - `irosh receive <code>`: Connects, downloads, and terminates.
- **Benefit**: No persistent server configuration required for ad-hoc sharing.

## 💬 Communication & Collaboration

### 4. Secure P2P Chat (`irosh chat <peer>`)
- **Description**: A minimal, encrypted chat session between Irosh nodes.
- **Use Case**: Quick coordination between administrators or users sharing a P2P node.
- **Implementation**: Custom SSH channel type (`irosh-chat`).

### 5. Multi-Hop Port Forwarding
- **Description**: The ability to tunnel through multiple Irosh nodes to reach an isolated backend.

### 6. Identity Groups & ACLs
- **Description**: Define groups of trusted keys and enforce access control lists based on peer labels.

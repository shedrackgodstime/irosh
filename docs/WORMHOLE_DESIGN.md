# Design Document: Irosh Wormhole (Secure Ad-hoc Pairing)

This document outlines the design, security considerations, and implementation strategy for the **Wormhole** feature in Irosh—a human-friendly way to bridge nodes without manual ticket exchange.

## 🎯 The Objective
Solve the "Haste Friction": Users often need to connect to a new machine (e.g., a work PC) where copying a 100-character Iroh ticket is either impossible, insecure (e.g., via Slack/Email), or too slow.

---

## 🛑 The Problem: Discovery vs. Stealth
Irosh's primary value is **Stealth**. By requiring a long, high-entropy ticket, we ensure that an attacker cannot "find" or "scan" your server.

1.  **The Brute-Force Risk**: Short, human-readable codes (e.g., `429-sky`) are guessable.
2.  **The TOFU Challenge**: Irosh uses **Trust On First Use (TOFU)**. If an attacker guesses a wormhole code, they can "be the first" to connect and inject their key into your trusted list.
3.  **Resource Usage**: Traditional peer discovery (MDNS/Gossip) can be resource-heavy if left running permanently.

---

## 🛠️ The Solution: The "Discovery Bridge"
Instead of a permanent discovery service, we implement an **on-demand rendezvous bridge**.

**Philosophy**: Discovery is not Auth. The wormhole's primary job is to share the long Iroh Ticket. Once the ticket is discovered, the client proceeds to the standard authentication flow.

### 1. Ephemeral Wormhole (Standard)
- **Command**: `irosh wormhole`
- **Behavior**: Generates a random 3-word code.
- **Security**: Valid for **one successful pairing**. After discovery, the standard auth logic (TOFU or Node Password) takes over.

### 2. The "Invite Pattern" (Temp Password)
- **Command**: `irosh wormhole --passwd <secret>`
- **Use Case**: Used when no permanent Node Password is set, but the user wants to securely "invite" a new device into a populated vault.
- **Behavior**: Provides a one-time secret that gates the first connection after discovery.

---

## 🛡️ Security Layers & Tradeoffs

| Layer | Component | Purpose |
| :--- | :--- | :--- |
| **Discovery** | Wormhole Code | Replaces the long Ticket for initial "finding" of the node. |
| **Authentication** | Node/Temp Password | Authorizes the connection once the ticket is discovered. |
| **Trust** | SSH Vault | Pins the key permanently once the first auth is successful. |

### The "Booster Rocket" Pattern
The Wormhole is not a permanent connection method. It is a **pairing mechanism**:
1. Client resolves Code -> Gets Ticket.
2. Client connects using Ticket -> Triggers Handshake.
3. Server Auth handles the key/password verification.
4. On success, the key is added to the Vault and the Wormhole closes.

---

## ⚖️ Tradeoffs
- **Complexity**: Adds an IPC layer between the CLI and the background Daemon.
- **Entropy**: We must enforce a minimum length/word-count for custom codes to prevent "easy-guess" vulnerabilities.
- **Reliance on Iroh Gossip**: The discovery relies on a temporary Iroh Gossip topic, which requires at least one working relay or local connectivity.

---

## 🔒 Security Mitigations (The "Shield")
To prevent the wormhole from becoming a vulnerability, we implement the following protections:

### 1. Keyed Topic Hashes
Instead of using the raw wormhole code as the Gossip topic name, we use a **Keyed HMAC**:
- `Topic = HMAC-SHA256(Key: code, Data: "irosh-wormhole-v1")`
- This ensures that an attacker monitoring gossip topics cannot "guess" the code by seeing the topic name, and vice versa.

### 2. Auto-Burn & Cleanup
- **Expiry**: Ephemeral wormholes expire after 5 minutes of inactivity.
- **Auto-Burn**: The wormhole is destroyed immediately after **one** successful pairing.

### 3. Rate Limiting & Auto-Burn
- **Max Attempts**: 3 failed authentication attempts per wormhole window.
- **Auto-Burn**: The wormhole is destroyed immediately after **one** successful pairing or after 3 failed attempts.
- **Entropy Floor**: Custom codes (Persistent) must be at least 8 characters long if no password is used.

### 4. Protocol Isolation (ALPN)
Wormhole connections use a dedicated ALPN: `irosh/pairing/v1`.
- This ensures that a "wormhole client" cannot accidentally access the full SSH server without first completing the pairing handshake.

---

## 🏁 Final Call & Recommendation
**Implement as an "On-Demand Trust Seed".**

The Wormhole should be the **standard way** new users interact with Irosh. By making it an "on-demand" service triggered via `irosh system wormhole`, we maintain the "Zero-Resources" philosophy of the daemon while solving the single biggest UX hurdle in P2P networking: the "Ticket Dance."

**Implementation Priority**:
1. **IPC Layer**: Allow CLI to talk to the running background service.
2. **Rendezvous Logic**: Use short-lived Gossip topics for ticket exchange.
3. **Session Auth**: Integrate one-time passwords into the `Authenticator` trait.

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

## 🛠️ The Solution: The "Trust-Seed" Bridge
Instead of a permanent discovery service, we implement an **on-demand rendezvous bridge**.

### 1. Ephemeral Wormhole (Standard)
- **Command**: `irosh system wormhole`
- **Behavior**: Generates a random 3-word code (e.g., `crystal-piano-7`).
- **Expiry**: Valid for **one successful connection** or 5 minutes.
- **Security**: The short time window and high-entropy wordlist make bot-guessing mathematically improbable.

### 2. Persistent Wormhole (Custom/Manual)
- **Command**: `irosh system wormhole [custom-code] --no-expire-reboot`
- **Behavior**: Uses a user-provided string. Survives reboots.
- **Security**: **MANDATORY Session Password.** Because the code is persistent and potentially low-entropy, a secondary shared secret is required for the first handshake.

---

## 🛡️ Security Layers & Tradeoffs

| Layer | Component | Purpose |
| :--- | :--- | :--- |
| **Discovery** | Wormhole Code | Replaces the long Ticket for initial "finding" of the node. |
| **Authorization** | Session Password | Prevents unauthorized "TOFU hijacking" if the code is guessed. |
| **Verification** | SSH TOFU | Pins the host key once the first connection is established. |

### The "Booster Rocket" Pattern
The Wormhole is not a permanent connection method. It is a **pairing mechanism**:
1. Client connects via Wormhole + Password.
2. Server verifies the client and **automatically adds the client's public key** to its permanent `authorized_keys`.
3. The Wormhole closes.
4. Future connections use standard SSH public-key auth—the code and password are never needed again.

---

## ⚖️ Tradeoffs
- **Complexity**: Adds an IPC layer between the CLI and the background Daemon.
- **Entropy**: We must enforce a minimum length/word-count for custom codes to prevent "easy-guess" vulnerabilities.
- **Reliance on Iroh Gossip**: The discovery relies on a temporary Iroh Gossip topic, which requires at least one working relay or local connectivity.

---

## 🏁 Final Call & Recommendation
**Implement as an "On-Demand Trust Seed".**

The Wormhole should be the **standard way** new users interact with Irosh. By making it an "on-demand" service triggered via `irosh system wormhole`, we maintain the "Zero-Resources" philosophy of the daemon while solving the single biggest UX hurdle in P2P networking: the "Ticket Dance."

**Implementation Priority**:
1. **IPC Layer**: Allow CLI to talk to the running background service.
2. **Rendezvous Logic**: Use short-lived Gossip topics for ticket exchange.
3. **Session Auth**: Integrate one-time passwords into the `Authenticator` trait.

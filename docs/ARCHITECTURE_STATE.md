# Irosh State & Architecture Model

This document defines how state is stored, modified, and synchronized between the
background daemon and the interactive CLI tools. 

The goal is to maintain **zero-overhead, lock-free, cross-platform state synchronization**.

---

## 1. The Core Architecture

We use a **Reactive Filesystem Architecture** (Option: Atomic Writes + OS File Watcher).

1. **No IPC/Sockets**: The CLI does not communicate with the Daemon over a network port or socket.
2. **No Polling**: The Daemon does not constantly check files for changes.
3. **OS-Level Events**: The Daemon registers with the OS Kernel (via the Rust `notify` crate) to be woken up instantly when a file in the config directory changes.
4. **Atomic Mutations**: All writes (by either the CLI or the Daemon) use a safe `.tmp` rename pattern to prevent corruption.

---

## 2. Directory Layout

All state is stored locally, default `~/.irosh/`:

```text
~/.irosh/
├── server/
│   ├── identity/             # Cryptographic private key (Ed25519)
│   ├── config.json           # Node password hash, relay URL, stealth-secret
│   ├── state.json            # Ephemeral state (Active wormholes, burn timers)
│   └── trust/                # The "Vault"
│       └── clients/          # Folder containing authorized client public keys
└── client/
    ├── identity/             # Client's own private key
    └── peers/                # Saved address book (aliases -> tickets)
```

---

## 3. The Synchronization Flow

### Example A: User runs `irosh trust revoke Laptop-Old`

1. **CLI reads**: The CLI reads `~/.irosh/server/trust/clients/` to find the key.
2. **CLI modifies**: The CLI deletes the specific key file.
3. **OS signals**: The Kernel detects the folder modification and fires a `notify` event.
4. **Daemon reacts**: The running Daemon catches the event, re-reads the folder, and removes the key from its active memory.
5. **Daemon executes**: The Daemon looks for any active SSH sessions using that key and terminates them instantly.

### Example B: Client connects via Wormhole (Daemon modifying state)

1. **Daemon authenticates**: A new client connects and provides the correct wormhole password.
2. **Daemon writes**: The Daemon writes the new client's key to `~/.irosh/server/trust/clients/`.
3. **Daemon updates state**: The Daemon updates `state.json` to mark the wormhole as "completed/burning".
4. (No CLI action is needed because the Daemon is the active server).

---

## 4. The Atomic Write Pattern (Race Condition Prevention)

Because both the CLI and the Daemon can edit `config.json` or `state.json` simultaneously, we NEVER write directly to the target file.

**The Rule for all file modifications:**
1. Read `target.json` into memory.
2. Modify data in memory.
3. Write data to `target.json.tmp`.
4. Run OS-level `rename("target.json.tmp", "target.json")`.

**Why?**
The OS `rename` system call is guaranteed to be atomic. If the CLI and the Daemon both try to save at the exact same millisecond, the OS forces one to win and the other to overwrite it instantly. The file is never left in a corrupted or half-written state.

---

## 5. Resolving the "Double Instance" Problem

What happens if `irosh system install` (daemon) is running, and the user opens a terminal and types `irosh host`?

**Solution**: The Identity Lock.

- When the Daemon starts, it binds to the Iroh network using the `identity` private key.
- If a user runs `irosh host`, it tries to bind to the same network using the same key. The Iroh networking stack will throw an `Address in Use` or `Conflict` error.
- The `irosh host` command must catch this specific error and cleanly print:
  `❌ Failed to start. The Irosh daemon is already running in the background. Use 'irosh system logs' to view activity.`

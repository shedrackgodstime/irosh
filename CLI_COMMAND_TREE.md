# Irosh CLI Command Tree

> **Reading this file**: Each command is listed with its purpose, flags, and any design notes.
> Commands marked `[DESIGN DECISION NEEDED]` are flagged for review before implementation.

---

```text
irosh
```
**Root command — Dashboard.**
Shows: server status (running/stopped), trusted device count, active sessions, active wormholes.
If no subcommand is given and no target is passed, shows this dashboard.

```text
irosh <target>
```
**Shortcut connect.**
`<target>` is resolved in order: saved alias → full ticket → wormhole code.
Equivalent to `irosh connect <target>`.

---

## Server Commands

```text
irosh host
```
Run the server **in the foreground**. Prints the Ticket and first-run guidance.
Identical to the background daemon in auth behavior. Stops when the terminal closes.

```text
irosh wormhole [code] [--passwd <pw>] [--persistent]
```
Start a **Discovery session** — publishes this server's Ticket under a short code.
- `[code]`: Optional. Custom code (e.g. `my-office`). Auto-generates a 4-word code if omitted.
- `--passwd <pw>`: Temp password required for this wormhole session only. Destroyed after first use.
- `--persistent`: Wormhole survives reboot. **Requires `--passwd`.**
- **If no server is running**: Wormhole auto-starts a temporary foreground server session.
- **If daemon is running**: Wormhole uses the daemon's current ticket.

```text
irosh wormhole status
```
List all currently active wormhole codes and their expiry state.

```text
irosh wormhole stop [code]
```
Stop a specific wormhole by code, or all active wormholes if no code is given.

---

## Connection Commands

```text
irosh connect <target> [--forward L:port:R:port]
```
Connect to a peer. `<target>` resolved as: alias → ticket → wormhole code.
- `--forward L:port:R:port`: Local/remote port forwarding tunnel.
- **Removed `--auth-password`**: Not needed. Client prompts automatically if server requires a password.
- **Removed `--insecure`**: Not aligned with the security model. TOFU handles first-trust.
- **`--secret <string>`**: Removed from `connect`. Moved to server config (see `irosh config set stealth-secret`). See note below.

> **Note on `stealth-secret`**: When set on the server via `irosh config set stealth-secret <value>`, the server uses a
> custom ALPN handshake identifier derived from the secret. Any client connecting must also have the same secret
> configured. Without it, the server is **silently invisible** to unknown clients — they cannot even begin a handshake.
> This is an advanced stealth layer for paranoid setups. Not required for normal use.
> The client uses it automatically if configured via `irosh config set stealth-secret <value>`.

```text
irosh put <peer> <local> [remote]
```
Upload a file or folder to a peer directly. `<peer>` is an alias or ticket.

```text
irosh get <peer> <remote> [local]
```
Download a file or folder from a peer directly. `<peer>` is an alias or ticket.

---

## System / Daemon Commands

```text
irosh system install | uninstall
```
Install or remove the OS-level background service (systemd / launchd / Windows Task Scheduler).
Prints the Ticket and first-run guidance on install.

```text
irosh system start | stop | restart
```
Control the background daemon without reinstalling.

```text
irosh system status
```
Show daemon health: PID, uptime, memory, connection count.

```text
irosh system logs [-f]
```
Print daemon logs. `-f` to follow (stream live). Critical for debugging headless setups.

---

## Peer Commands (Address Book)

```text
irosh peer list
```
Show all saved peers (alias, NodeID, last seen).

```text
irosh peer add <name> <ticket>
```
Manually save a peer by name. The ticket is validated before saving.

```text
irosh peer remove <name>
```
Delete a saved peer from the local address book.

```text
irosh peer info <name>
```
Show detailed metadata: NodeID, ticket, when added, connection history.

---

## Trust Commands (Who Can Connect TO You)

```text
irosh trust list
```
Show all trusted devices allowed to connect to YOUR server.
Columns: Fingerprint | Alias (if any) | Date Paired | Last Seen | Status (Active/Inactive).

```text
irosh trust revoke <fingerprint>
```
Remove a specific device's permanent access. Closes any active session for that device immediately.

```text
irosh trust reset
```
**Nuclear option.** Wipes the entire Vault AND clears the Node Password.
Closes ALL active sessions immediately. Requires typing `yes` to confirm.

---

## Password Commands

```text
irosh passwd set
```
Set or update the Node Password. Uses an interactive TTY prompt (never a CLI argument).
Once set, TOFU is disabled — all new unknown devices must provide this password to pair.

```text
irosh passwd remove
```
Clear the Node Password.
- If Vault is not empty: new unknown devices are REJECTED (no password, no TOFU).
- If Vault is empty: TOFU becomes active again for the first connection.

```text
irosh passwd status
```
Print whether a Node Password is currently set (yes/no). Never reveals the hash.

---

## Identity Commands

```text
irosh identity show
```
Display this machine's NodeID and public key fingerprint.

```text
irosh identity rotate
```
Generate a new cryptographic identity.
⚠️ **WARNING**: This changes your NodeID and Ticket permanently.
All trusted relationships (other servers that trusted you) are broken.
All peers who saved your old Ticket must reconnect.

---

```text
irosh config list
```
Show all global CLI settings and their current values.

```text
irosh config get <key>
```
Get the value of a specific config setting.

```text
irosh config set <key> <value>
```
Set a config value.

**Known config keys:**

| Key | Description | Default |
| :--- | :--- | :--- |
| `stealth-secret` | Shared ALPN secret for extra stealth (server + client must match) | none |
| `relay-url` | Custom relay server URL | Iroh default |
| `log-level` | Logging verbosity: debug / info / warn / error | info |
| `wormhole-timeout` | Default wormhole expiry duration | until-first-connect |
| `state-dir` | Override default state directory | `~/.irosh` |
| `default-user` | Default username for connections | system user |

---

## Config Migration (Import / Export)

```text
irosh config export [--output <file>]
```
Export a **passphrase-encrypted** bundle containing:
- Server identity (NodeID + private key)
- Vault (all trusted client keys)
- Config settings (password hash, relay config, etc.)

Use case: Migrating Irosh to a new machine. Import on the new machine and all trusted devices
can still connect to you — same NodeID, same Ticket, no re-pairing needed.

⚠️ The bundle is encrypted with a passphrase you set at export time. Keep the file and
passphrase safe. Anyone with both can fully impersonate your server.

```text
irosh config import <file>
```
Restore an exported bundle on a new machine. Prompts for the export passphrase.
Will refuse to overwrite an existing identity without explicit `--force`.


---

## Diagnostics

```text
irosh check
```
Run P2P network diagnostics: relay connectivity, NAT type, endpoint resolution.
Useful for debugging connection failures.

---

## Key Design Principles

- **Top-Level Velocity**: Common tasks (`connect`, `wormhole`) are at the top level for speed.
- **Secure by Design**: `passwd set` uses an interactive TTY prompt. Passwords never appear in CLI arguments or shell history.
- **Consistent Auth**: `irosh host` and `irosh system install` behave identically in auth. No foreground-vs-background special cases.
- **Self-Contained Wormhole**: `irosh wormhole` always works, even if no server is running. It auto-starts a temporary session if needed.
- **Vault = authorized_keys**: `irosh trust` is the UI over permanent key-based access. Changing or removing the Node Password does NOT affect already-trusted keys.
- **Stable Foundation First**: All future features (`put`, `get`, `chat`, `share`) are designed AFTER the core auth and connection foundation is proven stable and unlikely to change.

---

## 🚀 Future Features (Do Not Implement Until Foundation is Stable)

These are planned features that will be designed in separate conversations once the core
auth flow, trust system, and connection pipeline are complete and stable.

| Feature | Command Idea | Description |
| :--- | :--- | :--- |
| **File Sharing** | `irosh share <path> [--passwd]` | Host a file/folder. Anyone with your NodeID (and optional password) can download it. |
| **File Transfer** | `irosh put <peer> <file>` / `irosh get <peer> <file>` | Direct peer-to-peer file push/pull without an interactive shell. |
| **P2P Chat** | `irosh chat <peer>` | Minimal encrypted real-time chat between trusted peers. |

> These features share the same trust and auth foundation. They will be layered on top
> without breaking the existing connection model.

---

## ✅ Closed Design Questions

- [x] **`--secret` flag**: Removed from `connect`. Moved to `irosh config set stealth-secret`. Client reads it automatically if configured.
- [x] **`irosh identity rotate` UX**: Warn only. Require user to type their current NodeID to confirm. No forced `trust reset`.
- [x] **`irosh put` / `irosh get`**: Future feature — requires separate design. Not part of the foundation.
- [x] **`irosh config` keys**: Defined above (6 keys: stealth-secret, relay-url, log-level, wormhole-timeout, state-dir, default-user).
- [x] **Import / Export**: `irosh config export` / `irosh config import`. Passphrase-encrypted bundle. Use case: server migration without re-pairing all devices.


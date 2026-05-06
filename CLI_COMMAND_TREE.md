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
- **Removed**: `--auth-password` — not needed; the client prompts automatically if the server requires a password.
- **Removed**: `--insecure` — not aligned with the security model. TOFU handles first-trust.
- **`--secret <string>`**: `[DESIGN DECISION NEEDED]` — Current implementation uses this to derive a custom ALPN so only clients who know the secret can even "see" the server. Evaluate if this stealth layer is worth the added complexity or should be folded into the `irosh passwd` flow.

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

## Config Commands

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
Set a config value (e.g. default relay, log level, etc.).

---

## Diagnostics

```text
irosh check
```
Run P2P network diagnostics: relay connectivity, NAT type, endpoint resolution.
Useful for debugging connection failures.

---

## Key Design Principles

- **Top-Level Velocity**: Common tasks (`connect`, `put`, `get`, `wormhole`) are at the top level for speed.
- **Secure by Design**: `passwd set` uses an interactive TTY prompt. Passwords never appear in CLI arguments or shell history.
- **Consistent Auth**: `irosh host` and `irosh system install` behave identically in auth. No foreground-vs-background special cases.
- **Self-Contained Wormhole**: `irosh wormhole` always works, even if no server is running. It auto-starts a temporary session if needed.
- **Vault = authorized_keys**: `irosh trust` is the UI over permanent key-based access. Changing or removing the Node Password does NOT affect already-trusted keys.

---

## Open Questions (Resolve Before Implementation)

- [ ] **`--secret` flag**: Keep as a stealth ALPN mechanism, fold into config, or remove?
- [ ] **`irosh identity rotate` UX**: Should it require `irosh trust reset` first, or warn and let the user decide?
- [ ] **`irosh put` / `irosh get`**: Should these open a one-shot connection per transfer, or require an active session?
- [ ] **`irosh config` keys**: What are the full list of configurable settings?
- [ ] **Import / Export**: `irosh config export` / `irosh config import` for migrating server setup — needed or not?

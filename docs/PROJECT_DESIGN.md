# Irosh: Unified Project Design & Lifecycle Specification

This document defines the formal behavior, security model, and user experience standards
for the Irosh P2P SSH system. It is the single source of truth for implementation.

---

## 1. System Philosophy

1.  **Trust-Seed Onboarding**: New devices are added to the Vault (`trust/clients/`) through an explicit pairing event.
2.  **The First-Connection Rule**: The very first connection to a server with an empty vault is automatically trusted (TOFU). If a Node Password is set before the first connection, TOFU is disabled and the password is required instead.
3.  **Consistent Auth**: The server behaves identically whether running as `irosh host` (foreground) or via the background daemon. No `y/n` prompts. Auth is always Key or Password.
4.  **Stealth by ALPN**: Two protocol identifiers separate traffic:
    - `irosh/1`: For established trust (Strict mode, key-only).
    - `pairing/v1`: For the wormhole pairing handshake.
5.  **Discovery is not Auth**: `irosh wormhole` is only a bridge to the Ticket. After discovery, the client follows the same auth flow as a direct Ticket connection.
6.  **The Invite Pattern**: If no Node Password is set, `irosh wormhole --passwd` provides a one-time "Invite" secret to pair a new device into a populated vault.

---

## 2. Command Specification

| Command | Description |
| :--- | :--- |
| `irosh host` | Run the server in the foreground. Prints the Ticket. |
| `irosh system install` | Install and run as a background daemon that survives reboots. print just like host|
| `irosh wormhole` | Publish the Ticket under a short 3-word code for easy discovery. |
| `irosh wormhole --passwd <pw>` | Same, but requires this password during the wormhole's pairing session only. |
| `irosh connect <ticket-or-code>` | Connect to a server using a Ticket or a wormhole discovery code. |
| `irosh passwd` | Set or change the Node Password for this server. |
| `irosh trust list` | List all trusted client fingerprints. **"Who has permanent access to my server?"** |
| `irosh trust revoke <fingerprint>` | Remove a single trusted device from the Vault. **Only way to kick out a trusted device short of full reset.** |
| `irosh trust reset` | Wipe the entire Vault. Reverts to bootstrap state. Nuclear option. |

> **Why `trust` exists**: Once a device's key is in the Vault, it connects **forever without a password**. Changing the Node Password does NOT revoke existing trusted keys. `irosh trust revoke` is the ONLY way to remove permanent access from a specific device. Example: You sell your laptop — `irosh trust revoke <laptop-fingerprint>` immediately kills its access.
| `irosh config export` | TBD — see Q5 discussion. |
| `irosh config import` | TBD — see Q5 discussion. |

---

## 3. Auth State Machine (Unified)

### Direct Ticket Flow (`irosh connect <ticket>`)
1. Client presents its SSH Public Key.
2. Server checks the Vault:
   - **Key is present**: Accept immediately (Established trust always wins).
   - **Key is absent AND Node Password is set**: Challenge for Node Password.
   - **Key is absent AND active Wormhole session has `--passwd`**: Challenge for Temp Password (Invite Pattern).
   - **Key is absent AND Vault is empty AND no password set**: Accept and save key (TOFU).
   - **Key is absent AND Vault is not empty AND no password set**: Reject. Silent drop.

### Wormhole Discovery Flow (`irosh connect <3-word-code>`)
1. Client resolves the 3-word code via Pkarr to get the real Ticket.
2. Discovered Ticket is **saved to the local peer list** (marked "Discovered, Not Connected Yet").
3. Client proceeds to the **Direct Ticket Flow** above using the discovered Ticket.
4. Even if the wormhole closes after discovery, the saved Ticket can be used to reconnect later.

---

## 4. Edge Case Q&A Registry

### Q1: What if two devices try to connect simultaneously for the first time? (Race condition)
- **User note**: Funny edge case but acknowledged as real.
- **Solution**: Atomic file write. We write the key to a `.tmp` file first, then use an OS-level `rename()` call. The OS guarantees only one rename wins. The second device's write fails and it gets rejected cleanly.

---

### Q2: What if the Node Password is forgotten?
- **User answer**: Physical access to the machine is required. User must access the machine directly and run `irosh passwd` to set a new one.
- **Security note**: There is no remote password recovery. This is by design.

---

### Q3: What if the Node Password is set AFTER the first device is already trusted?
- **User answer**: The existing trusted device should still be allowed in without a password (it already earned trust via key). The password now only applies to NEW, unknown devices.
- **Command added**: `irosh trust revoke <fingerprint>` — lets the admin manually revoke a specific device's trust if needed.

---

### Q4: What if `irosh trust reset` is run while an active session is open?
- **User answer**: The session **must close immediately**. This is a critical safety feature. Scenario: sysadmin finds unauthorized access and runs reset — the attacker's session must be killed at that exact moment. No grace period.

---

### Q5: Should the `trust` command be expanded? What about config import/export?
- **User answer**: The `trust` command should be expanded (see `trust list`, `trust revoke`, `trust reset` above).
- **Config import/export**: Open question. Need to research what standard SSH does here.
- **Research needed**: Standard OpenSSH does NOT have a built-in import/export command. It manages keys via `authorized_keys` and `known_hosts` files directly. Given Irosh's design, a config export could be useful for migrating a server setup to a new machine. **Decision deferred to implementation conversation.**

---

### Q6: What if a trusted device gets reinstalled (new OS, new SSH key)?
- **Problem**: The old key is in the Vault but the new key is unknown. The device is a stranger to the server.
- **User answer**: No clear answer yet.
- **Proposed solution**: If a Node Password is set, the reinstalled device simply uses the password to re-pair. If no password is set, the user must either:
  - A) Run `irosh trust revoke <old-fingerprint>` + let the device connect (TOFU if vault becomes empty).
  - B) Run `irosh trust reset` if there are no other trusted devices.
- **Action**: The client should print a helpful message: *"Your key was not recognized by this server. If this server has a Node Password, retry with `--passwd`."*

---

### Q7: What if the user wants to revoke ONE device, not all?
- **Solution**: `irosh trust revoke <fingerprint>` command (see Command Specification).
- `irosh trust list` shows all devices with their fingerprints so the user can identify the right one.

---

### Q8: What if two wormhole codes collide (two Irosh servers use the same 3-word code)?
- **User answer**: No idea — needs a solution.
- **Analysis**: The Pkarr keypair is derived from the code via SHA-256. Two servers using the same code derive the **same keypair** and publish to the **same Pkarr address**. Last write wins. The client could discover the wrong server's ticket.
- **Proposed solution**: Auto-generated codes should be 4 words (not 3) to make random collision astronomically unlikely. For custom codes (`irosh wormhole my-code`), we warn the user: *"Custom codes are not guaranteed unique. If another server uses this code, discovery may fail."* Additionally, the server signs the Ticket with its own private identity key. The client validates this signature before trusting the discovered Ticket, so even if the wrong ticket is received, connection will fail at the SSH layer.
- **Action**: Add a note in the wormhole command to use auto-generated codes for security.

---

### Q9: What if the server goes offline between wormhole discovery and the connection attempt?
- **User answer**: Temporarily save the discovered Ticket in the local peer list (marked "Discovered, Not Connected Yet"). When the server comes back online, the user can connect using the saved Ticket without re-running wormhole discovery.
- **UX**: `irosh connect` (interactive selector) should show these "discovered" peers in a distinct state.

---

### Q10: What if Pkarr is unavailable?
- **User answer**: Show a clear error message and/or log it.
- **UX**: *"❌ Wormhole discovery failed: Cannot reach the Pkarr relay network. Check your internet connection."*

---

### Q11: What if two clients try to connect via the same wormhole code (and the server then burns it)?
- **User answer**: 30 seconds to 1 hour grace period is acceptable. But what if the system goes offline during that window?
- **Final Solution**:
  - **Key insight**: The key is written to the Vault **before** the burn timer starts. The burn is just cleanup (unpublishing from Pkarr). Security is already done.
  - On first successful auth via wormhole: Save key → Mark wormhole `auth_completed = true` in `state.json` → Start burn timer (30s–1hr configurable).
  - If a second attempt arrives within the timer: reject immediately (wormhole is already "burning").
  - **If server goes offline during the timer**: On restart, check `state.json` for any wormhole marked `auth_completed = true` but `burned = false`. Burn those **immediately on startup**.
  - **Client side**: The discovered Ticket is cached locally (Q9). Even if the wormhole is gone, the client reconnects via the saved Ticket directly. No re-discovery needed.

---

### Q12: Is the Wormhole a security bypass?
- **Final Decision**: No. Discovery is not Auth. The wormhole only shares the Ticket.
- **Rules for Initiation**:
  1. **If Vault is EMPTY**: `irosh wormhole` is **Allowed** (The standard bootstrap experience).
  2. **If Vault is NOT EMPTY + No Node Password**: `irosh wormhole` is **Blocked**. The user must set a Node Password before they can use easy-discovery for new devices.
  3. **If a Node Password is set**: `irosh wormhole` is **Allowed**. The connection is protected by the password challenge.
  4. **The `--passwd <secret>` Override**: Using `--passwd` always allows the wormhole, as it provides an explicit one-time secret for that pairing session.
- **Key rule**: The wormhole `--passwd` (if used) is consumed and destroyed after ONE successful pairing. It cannot be reused.


---

## 5. 🧊 Parking Lot (Future Conversations)

- **Q13: Double Instance** — What happens when `irosh host` runs while the daemon is already active? Should it fail, attach, or show live logs?
- **Q14: Daemon crash mid-pairing** — How do we ensure atomic, crash-safe key writes?
- **Q15: `irosh system install` while `irosh host` is running** — Identity conflict resolution.
- **Alias Sync** — Should `trust/clients/` folder names double as peer aliases on the client side?
- **Audit Logs** — `irosh trust log` or similar to view connection history.
- **Config import/export** — Research OpenSSH equivalents before deciding.

---

## 6. ✅ Closed Gaps

### Gap A: Q16 — How does `irosh connect <input>` know what it was given?
- **It's three possible inputs, not two**: Alias, Ticket, or Wormhole code (auto-generated OR custom).
- **Detection order** (in strict priority):
  1. **Alias**: Is the input in the local saved peer list? → Use the saved ticket. No network call needed.
  2. **Ticket**: Does the input match the Iroh ticket format (long, base32-encoded, known prefix)? → Connect directly.
  3. **Wormhole code** (fallback): Everything else → Try Pkarr lookup. Covers both auto-generated 3-word codes AND any custom string the user passed to `irosh wormhole`.
- **Rule**: If the Pkarr lookup times out or finds nothing, print: *"❌ Could not resolve '\<input\>' as an alias, ticket, or wormhole code."*

---

### Gap B: Can you remove the Node Password?
- **No `irosh passwd --clear` command.**
- If you forget the password: physical access to machine → `irosh trust reset` → wipes Vault AND clears password → re-pair all devices.
- One command. One nuclear option. Simple, no confusion.

---

### Gap C: What does `irosh trust list` show?
- Fingerprint (always)
- Date/time of pairing (when the key was first added to the Vault)
- Alias or label if one exists (e.g., device name from the client)
- Status: Active (seen recently) vs. Inactive (never or long ago)

---

### Gap D: The first-run UX message
- **Applies to both `irosh host` AND `irosh system install`** — the message must be consistent.
- **Content must cover**:
  - The Ticket (how to share it)
  - The Vault state (empty = first device gets TOFU)
  - How to lock it down immediately (`irosh passwd`)
  - How to use wormhole for easy discovery
- **Exact copy to be designed in the implementation conversation.**


---

## 7. User Flows (End-to-End UX)

These flows describe exactly what the user does and what the system does at each step.
Both `irosh host` and `irosh system install` behave **identically** in auth. The only difference
is foreground vs. background execution.

---

### Flow 1: Quick Test / Temp Connection (TOFU — No Password)

> *"I just want to try it out or connect once."*

**On the Server machine:**
```
irosh host
```
Output:
```
🚀 Irosh server running.
📋 Your Ticket:
   iroh1abc...xyz

🔒 Vault is empty. The first device to connect will be permanently trusted.
💡 Tip: Run 'irosh passwd' now to require a password instead of trusting automatically.
```

**On the Client machine:**
```
irosh connect iroh1abc...xyz
```
- Server sees a new, unknown key.
- Vault is empty → TOFU: key is saved to Vault.
- Connection established.
- Server prints: `✅ New device trusted and connected. (1 trusted device in Vault)`

**After this:**
- The trusted device always connects silently (key-only, no password).
- Any other unknown device is **rejected** (no password set, vault not empty).

---

### Flow 2: Secure Bootstrap (Password Before First Connection)

> *"I'm setting up a server and walking away. I want full control."*

**On the Server machine:**
```
irosh host         (or: irosh system install)
irosh passwd
```
- `irosh passwd` prompts: `Enter new Node Password: ___`
- Password is hashed and stored.
- Server prints: `🔐 Password set. TOFU disabled. All new devices must provide this password.`

**On the Client machine:**
```
irosh connect iroh1abc...xyz
```
- Server sees a new, unknown key.
- Node Password is set → prompts client: `Enter Node Password: ___`
- Client types password → key is saved to Vault.
- Connection established.

**After this:**
- The now-trusted device connects silently (key-only, no password prompt again).
- Any other new device must also provide the password to be trusted.

---

### Flow 3: Always-On Background Server

> *"I want it to survive reboots, just like SSH."*

```
irosh system install
```
Output (same as `irosh host` but then adds):
```
🚀 Irosh server running in the background.
📋 Your Ticket:
   iroh1abc...xyz

🔒 Vault is empty. The first device to connect will be permanently trusted.
💡 Tip: Run 'irosh passwd' to require a password for all new devices.
✅ Service installed. Will restart automatically on reboot.
```

- The rest of the auth flow is **identical to Flow 1 or Flow 2** depending on whether a password is set.
- The user manages it via: `irosh system start/stop/status/uninstall`.

---

### Flow 4: Wormhole Discovery (Can't Copy the Ticket)

> *"I'm at the office. I can't copy this long ticket to my phone. I need a quick way in."*

**On the Server machine:**
```
irosh wormhole
```
Output:
```
🌀 Wormhole active.
🔑 Discovery code: apple-tiger-blue
   (Anyone who runs 'irosh connect apple-tiger-blue' will discover your ticket)
⏳ Expires: after first successful connection (or run 'irosh wormhole --stop' to cancel)
```

**On the Client machine:**
```
irosh connect apple-tiger-blue
```
- Client resolves `apple-tiger-blue` via Pkarr → retrieves the real Ticket.
- Ticket is **saved to local peer list** as a "Discovered" peer.
- Client proceeds with the **normal auth flow** (Flow 1 or Flow 2).
  - Vault empty + no password → TOFU: connected and trusted.
  - Password set → client is prompted for the Node Password.

**After this:**
- Wormhole burns (unpublished from Pkarr).
- Future connections use the **saved local Ticket** directly. No wormhole needed again.

---

### Flow 5: Adding a Second Device

> *"My laptop is already trusted. I want to add my phone too."*

- **If Node Password is set**: Run `irosh connect <ticket>` on the phone → prompted for password → enters it → trusted.
- **If NO Node Password is set**: The phone is rejected (vault is not empty, no password). User must either:
  - Set a permanent password first: `irosh passwd` on the server, then retry on the phone.
  - OR use `irosh wormhole --passwd <temp>` to gate the one-time pairing session.

---

### Flow 6: Wormhole with Temp Password (Secure Wormhole)

> *"I want to use wormhole but my vault is empty and I have no Node Password set yet."*

**On the Server machine:**
```
irosh wormhole --passwd mysecret
```
Output:
```
🌀 Wormhole active.
🔑 Discovery code: apple-tiger-blue
🔐 Wormhole password required to pair (destroyed after first use).
```

**On the Client machine:**
```
irosh connect apple-tiger-blue
```
- Discovers the Ticket.
- Server sees unknown key → prompts: `Enter Wormhole Password: ___`
- Client enters `mysecret` → key is saved → wormhole password **destroyed immediately**.
- Connection established.

---

### Flow 7: Revoking a Specific Device

> *"I sold my old laptop. I need to remove its access immediately."*

```
irosh trust list
```
Output:
```
Trusted Devices (2):
  1. SHA256:abc...123  [Laptop-Old]  Paired: 2026-01-10  Last seen: 2026-04-30
  2. SHA256:xyz...789  [Phone]       Paired: 2026-03-01  Last seen: 2026-05-06
```

```
irosh trust revoke SHA256:abc...123
```
Output:
```
✅ Device 'Laptop-Old' removed. All active sessions for this device have been closed.
```

---

### Flow 8: Emergency Reset (Unauthorized Access Found)

> *"Someone got in who shouldn't have. Kill everything now."*

```
irosh trust reset
```
Output:
```
⚠️  This will remove ALL trusted devices and clear the Node Password.
    All active sessions will be closed immediately.
    Type 'yes' to confirm: ___
```
- On confirm: Vault wiped, password cleared, all active sessions killed instantly.
- Server returns to bootstrap state (Flow 1 or Flow 2).

---

### Flow 9: Reinstalled Device (New SSH Key, Old Access Lost)

> *"I wiped my laptop and reinstalled it. Now I can't connect."*

**Client side:**
- `irosh connect <ticket>` → `❌ Permission denied. Your key is not recognized.`
- Client hint: `If this server has a Node Password, retry with --passwd. Otherwise ask the server admin to revoke your old key.`

**Server side (admin):**
```
irosh trust list          → find the old laptop fingerprint
irosh trust revoke <fp>   → remove it
```
- If password is set: the reinstalled device re-pairs using the password.
- If no password and vault is now empty: TOFU is active again (Flow 1).
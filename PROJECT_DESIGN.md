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

---

## 2. Command Specification

| Command | Description |
| :--- | :--- |
| `irosh host` | Run the server in the foreground. Prints the Ticket. |
| `irosh system install` | Install and run as a background daemon that survives reboots. |
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
   - **Vault is empty AND no Node Password**: Trust and save key. (TOFU).
   - **Vault is empty AND Node Password is set**: Prompt for password. On success, save key.
   - **Vault is not empty AND key is present**: Accept immediately.
   - **Vault is not empty AND key is absent AND Node Password set**: Prompt for password. On success, save key.
   - **Vault is not empty AND key is absent AND no Node Password**: Reject. Silent drop.

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

### Q12: What if `irosh wormhole` is used to bypass security when the vault is not empty?
- **User answer**: Idea — `irosh wormhole --passwd <temp-password>`. The temp password is only valid during the wormhole session. After pairing, it is discarded. The fear: an attacker could use this temp password + wormhole code to bypass security.
- **Proposed solution (layered)**:
  1. If **Vault is not empty AND Node Password is already set**: `irosh wormhole` is **blocked by default**. Print: *"⚠️ Wormhole is disabled: Your server already has trusted devices and a Node Password. Use `irosh trust revoke` to manage access."*
  2. If **Vault is not empty AND no Node Password**: `irosh wormhole` is allowed but prints a strong warning: *"⚠️ Warning: No Node Password is set. Any device that discovers this wormhole code will be trusted automatically."*
  3. If **Vault is empty**: `irosh wormhole --passwd <temp>` is the recommended secure path. The temp password gates the first connection.
- **Key rule**: The wormhole `--passwd` is consumed and destroyed after ONE successful pairing. It cannot be reused.
- **Security note**: The wormhole is already blocked if the vault is populated + password is set. This prevents the "bypass" fear almost entirely.

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


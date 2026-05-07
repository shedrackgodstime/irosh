# Irosh CLI UX Components

This document defines all reusable interactive prompt components used across the CLI.
Each component maps to one or more commands. The goal is **zero friction** — the user
should never have to keep unnecessary information in their head.

> **Library already in use**: `dialoguer` (prompts, selects, confirmations) + `indicatif` (progress bars).
> All new components should use the same libraries for visual consistency.

---

## Component Inventory

### Already Built ✅

| Component | Location | Used By |
| :--- | :--- | :--- |
| Peer selector (interactive list) | `connect/mod.rs` | `irosh connect` (no target) |
| Password prompt (hidden input) | `connect/mod.rs` `CliPasswordPrompter` | `irosh connect` (server requires password) |
| Connection progress bar | `connect/mod.rs` | `irosh connect` (while connecting) |
| Ticket/NodeID shortener | `display.rs` | Peer list, connect output |

---

### To Be Built 🔧

---

## C1 — Danger Confirmation Prompt
**Type**: Text input confirmation (user must type a specific word to proceed).
**Library**: `dialoguer::Input`

**Used by:**
- `irosh trust reset` → user must type `yes`
- `irosh identity rotate` → user must type `ROTATE`
- `irosh passwd remove` → user must type `yes`

**UX:**
```
[WARN] This will wipe ALL trusted devices and clear the Node Password.
       All active sessions will be closed immediately.

       Type 'yes' to confirm, or press Ctrl+C to cancel: ___
```

**Behavior:**
- If input does not match expected string → print `Cancelled.` and exit cleanly.
- Never retry on mismatch (no loops). One shot only.
- Ctrl+C → `Cancelled.` and exit.

---

## C2 — Soft Confirmation Prompt (y/N)
**Type**: Single-key yes/no. Default is NO (safe default).
**Library**: `dialoguer::Confirm`

**Used by:**
- `irosh trust revoke <fingerprint>` → "Are you sure?"
- `irosh peer remove <name>` → "Are you sure?"
- `irosh wormhole` when vault is not empty and no password is set → security warning

**UX:**
```
[WARN] Remove 'Laptop-Old' from trusted devices? [y/N]: ___
```

**Behavior:**
- Default is `N` (pressing Enter without typing = No).
- `y` or `Y` → proceed.
- Anything else → `Cancelled.` and exit cleanly.

---

## C3 — Password Set Prompt (with confirmation)
**Type**: Hidden input + repeat confirmation.
**Library**: `dialoguer::Password::with_confirmation`

**Used by:**
- `irosh passwd set`
- `irosh wormhole --passwd` (if user runs `irosh wormhole` and is prompted interactively)
- `irosh config export` (export passphrase)

**UX:**
```
Enter new password: ____
Confirm password:   ____
```

**Behavior:**
- If passwords do not match → `Passwords do not match. Try again.` (retry once, then exit).
- Empty password → print `Password cannot be empty.` and retry once.
- Ctrl+C → `Cancelled.` and exit.

---

## C4 — Password Input Prompt (single, hidden)
**Type**: Hidden single input.
**Library**: `dialoguer::Password`

**Used by:**
- `irosh connect` when server requires Node Password (already built, needs polish)
- `irosh connect` when server requires Wormhole temp password
- `irosh config import` (import passphrase)

**UX (current — needs polish):**
```
[SEC] Server requires a password.
Password: ____
```

**Behavior:**
- Empty input → retry once with message `Password cannot be empty.`
- Ctrl+C → `Cancelled.` and abort connection cleanly.

---

## C5 — Interactive List Selector
**Type**: Arrow-key searchable list. Single selection.
**Library**: `dialoguer::Select` with `ColorfulTheme` (already built for peers)

**Used by:**
- `irosh connect` (no target) → pick from saved peers (already built)
- `irosh trust revoke` (no fingerprint given) → pick from trusted devices
- `irosh peer remove` (no name given) → pick from saved peers

**UX (peer selector — already built):**
```
? Select a peer to connect
❯ [work-pc]   abc123...xyz  (iroh1abc...xyz)
  [home-pi]   def456...uvw  (iroh1def...uvw)
  [phone]     ghi789...rst  (iroh1ghi...rst)
```

**UX (trust revoke selector — to build):**
```
? Select a device to revoke
❯ SHA256:abc...123  [Laptop-Old]  Paired: 2026-01-10  Last seen: 2026-04-30
  SHA256:xyz...789  [Phone]       Paired: 2026-03-01  Last seen: 2026-05-06
```

**Behavior:**
- Arrow keys to navigate.
- Enter to confirm.
- Ctrl+C → `Cancelled.` and exit.
- If list is empty → skip selector, print contextual message instead (e.g. "No trusted devices found.").

---

## C6 — Text Input Prompt (visible)
**Type**: Single-line visible text input.
**Library**: `dialoguer::Input`

**Used by:**
- `irosh peer add` (no name given) → "Enter a name for this peer:"
- `irosh wormhole` (interactive mode, ask for custom code or accept auto-generated)

**UX (peer naming):**
```
Enter a name for this peer (e.g. 'work-pc'): ____
```

**UX (wormhole code):**
```
[P2P] Generated code: apple-tiger-blue
      Press Enter to use this code, or type a custom one: ____
```

**Behavior:**
- Empty input → use the default/generated value.
- Ctrl+C → `Cancelled.` and exit.

---

## C7 — Warning Banner (non-blocking)
**Type**: Printed warning block. No input required.
**Library**: `eprintln!` with emoji prefix. No dialoguer needed.

**Used by:**
- `irosh wormhole` when vault is not empty and no password is set
- `irosh passwd remove` before the C2 prompt
- `irosh host` / `irosh system install` first-run when vault is empty

**UX (unprotected wormhole warning):**
```
[WARN] Security Notice:
      Your vault is empty and no password is set.
      Any device that discovers this code will be trusted automatically.
      Tip: Run 'irosh wormhole --passwd <secret>' to invite a device securely.
```

**UX (first-run empty vault):**
```
[SEC] Vault is empty. The first device to connect will be permanently trusted.
[INFO] Tip: Run 'irosh passwd set' now to require a password instead.
```

**Behavior:**
- Printed to stderr (so it doesn't pollute piped output).
- No user input needed. Execution continues after printing.

---

## C8 — Spinner / Progress Indicator
**Type**: Animated spinner for async operations.
**Library**: `indicatif::ProgressBar` with spinner style (already in use for connection).

**Used by:**
- `irosh connect` → "Connecting to work-pc..." (already built)
- `irosh wormhole` → "Waiting for discovery..." (while code is published and waiting)
- `irosh config import` → "Importing configuration..."

**UX (wormhole wait):**
```
[P2P] Wormhole active. Code: apple-tiger-blue
⠸ Waiting for a device to connect... (Ctrl+C to cancel)
```

**Behavior:**
- Spinner animates while waiting.
- On success → spinner stops, prints success message.
- On Ctrl+C → spinner stops, prints `Wormhole cancelled.`, cleans up (unpublish from Pkarr).

---

## C9 — Success / Failure Banner
**Type**: Final status output after an operation.
**Library**: `println!` or `eprintln!` with colored text. Consistent prefix convention for a professional Unix feel.

**Prefix convention:**
- `[OK]` (Green) — Success
- `[ERR]` (Red) — Failure / Error
- `[WARN]` (Yellow) — Warning (non-fatal)
- `[INFO]` (Blue) — Tip / Suggestion / State
- `[SEC]` (Magenta) — Security-related state
- `[P2P]` (Cyan) — Wormhole / Networking state

**Examples:**

After `irosh connect` first TOFU:
```
[OK] Connected to 'work-pc'.
[SEC] This device has been added to your trusted list. (2 trusted devices total)
[INFO] Tip: Run 'irosh trust list' to see all trusted devices.
```

After `irosh trust revoke`:
```
[OK] Device 'Laptop-Old' removed. Active sessions closed.
```

After `irosh wormhole` burns:
```
[OK] Device paired successfully. Wormhole closed.
```

After `irosh trust reset`:
```
[OK] Vault cleared. All trusted devices removed. Node Password cleared.
     Server is back in bootstrap state.
```

---

## Component-to-Command Map

| Command | Components Used |
| :--- | :--- |
| `irosh connect` (no target) | C5 → peer selector |
| `irosh connect` (password required) | C4 → password input |
| `irosh connect` (result) | C8 spinner → C9 success/fail |
| `irosh host` (first run, empty vault) | C7 warning banner → C9 server started |
| `irosh system install` (first run) | C7 warning banner → C9 server started |
| `irosh wormhole` (interactive) | C6 code input → C7 security warning (if needed) → C8 spinner |
| `irosh wormhole` (burns) | C9 success |
| `irosh passwd set` | C3 password set with confirmation |
| `irosh passwd remove` | C7 warning → C2 soft confirm |
| `irosh trust list` | C9 output (table, no prompt) |
| `irosh trust revoke` (no fingerprint) | C5 selector → C2 soft confirm → C9 result |
| `irosh trust revoke` (with fingerprint) | C2 soft confirm → C9 result |
| `irosh trust reset` | C7 warning → C1 danger confirm → C9 result |
| `irosh peer add` (no name) | C6 name input |
| `irosh peer remove` (no name) | C5 selector → C2 soft confirm |
| `irosh identity rotate` | C7 warning → C1 danger confirm (ROTATE) → C9 result |
| `irosh config export` | C3 passphrase → C8 spinner → C9 result |
| `irosh config import` | C4 passphrase → C8 spinner → C9 result |

---

## Notes for Implementation

1. **All prompts must check `std::io::IsTerminal`** before running interactive mode.
   If stdin is not a TTY (e.g. piped/scripted), skip the prompt and either use the
   provided flag value or exit with a clear error.

2. **Ctrl+C must always clean up**. Use `tokio::signal` or `ctrlc` crate to intercept
   SIGINT and run cleanup logic (e.g. unpublish wormhole, close connection) before exiting.

3. **All prompts go to stderr**. Ticket strings and machine-readable output go to stdout.
   This ensures `irosh host 2>/dev/null | grep iroh1` works correctly.

4. **All components live in `cli/src/ui/`** — a shared module imported by all command modules.
   No ad-hoc prompts scattered across command files.

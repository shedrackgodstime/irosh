# Irosh V2 Migration Audit: Gaps & Technical Debt

This document outlines the features and technical logic present in the `temp/cli_old` and `temp/src_old` directories that have not yet been fully migrated to the V2 architecture. These gaps represent the "final 20%" of the migration required to reach feature parity and production stability.

## 1. CLI Functional Regressions
The following flags and commands were present in V1 but are currently missing or simplified in the V2 CLI.

| Feature | Missing Logic | Importance |
| :--- | :--- | :--- |
| **`host --auth-mode`** | Choice between `key`, `password`, and `combined`. | **Critical.** Without this, the server cannot be secured by a password, only by keys. |
| **`host --authorize`** | Ability to pre-authorize OpenSSH keys. | **High.** Essential for "headless" setups where TOFU (Trust On First Use) is not possible. |
| **`host --simple`** | Minimalist output mode. | **Medium.** Required for integration with scripts or parent processes. |
| **`system status`** | OS-specific service manager diagnostics. | **Medium.** Users need to know *why* a service isn't starting (systemd vs launchd). |
| **`wormhole --foreground`** | Toggle between daemon-mode and interactive-mode. | **Medium.** Important for debugging wormhole connections without involving the daemon. |

---

## 2. Terminal & Signal Fidelity
The current PTY implementation in `src/session/pty.rs` is a placeholder compared to the robust Unix/Windows logic in the legacy codebase.

### Unix `AsyncStdin`
*   **What's Missing**: A custom non-blocking stdin reader using `AsyncFd`.
*   **Why it Matters**: Standard `tokio::io::stdin()` uses background threads that can block process exit. The legacy `AsyncStdin` ensured the CLI could exit immediately upon session termination.

### Windows `RawTerminal` & VT100
*   **What's Missing**: Manual invocation of `ENABLE_VIRTUAL_TERMINAL_PROCESSING` and `ENABLE_VIRTUAL_TERMINAL_INPUT` via `SetConsoleMode`.
*   **Why it Matters**: Without this, the Windows Command Prompt/PowerShell will not render ANSI colors or handle arrow keys correctly in a remote shell.

### Advanced Signal Handling
*   **What's Missing**: Handlers for `SIGQUIT` (Unix) and `ctrl_close`/`ctrl_shutdown` (Windows).
*   **Why it Matters**: Ensures that if a user closes their terminal window, the server gracefully shuts down P2P sessions rather than leaving "ghost" connections active.

---

## 3. Authentication & Security Policy
The library supports advanced auth, but the "glue" in the CLI and Host commands is missing.

*   **Shadow File Integration**: The `irosh host` command does not currently load the hashed password from the state directory. This breaks the `irosh passwd` workflow.
*   **Password Security Constraints**: The legacy code enforced an 8-character minimum for custom wormhole codes without passwords. This security check is missing in V2.
*   **Wormhole Expiry Logic**: V1 had logic to auto-disable wormholes after 24 hours or one successful connection. We need to verify this is robust in the V2 server loop.

---

## 4. Platform-Specific Service Managers
While the library has `src/sys`, the CLI lacks the "installer" logic for:
1.  **Windows Task Scheduler**: XML template generation and `schtasks` orchestration.
2.  **macOS launchd**: `.plist` generation and `launchctl bootstrap` logic.

## 5. In-Session Escape Commands
The V2 session driver (`drive_session`) currently lacks the escape sequence parser present in the legacy `InputLoop`. This is a regression from the V1 interactive experience.

| Command | Missing Logic | Importance |
| :--- | :--- | :--- |
| **`~.`** | Hard-disconnect logic in the input stream. | **High.** Essential for killing frozen sessions. |
| **`~c` / `~C`** | In-session command prompt (Command Mode). | **High.** Used for client-side control without disconnecting. |
| **`~get` / `~put`** | In-session file transfer (Side-Stream). | **High.** Unique Irosh feature for ad-hoc sharing. |
| **`~~`** | Literal tilde escape handling. | **Low.** Required for sending a real tilde. |

---

## 6. UI/UX Fidelity
The legacy CLI had several "quality of life" features that have not been ported.

*   **Command History**: The `~c` prompt had persistent history across sessions (`support/history.rs`).
*   **Tab Completion**: Path completion for `~get`/`~put` inside the session (`input/completion.rs`).
*   **Shortcut Target Parsing**: The ability to run `irosh <code-word>` directly without the `connect` subcommand (Shortcut dispatch in `main.rs`).

## Summary of Importance
While the **P2P Core** and **Connection Heuristics** of V2 are superior, the **Terminal UX**, **Access Control**, and **Interactive Escapes** are currently behind V1. Restoring these features is the next step toward a stable `v0.3.0` release.

# Windows Migration Handoff (v0.3.0-rc.1)

Welcome! If you are picking up this codebase on a Windows machine to finalize the Windows-specific functionality, this document is for you.

## 1. What Just Changed Since v0.2.0?

We have just completed a massive overhaul of the Linux/Unix side of the codebase and established a hardened, strictly separated architecture:

- **Terminal UX Engine**: The `cli/src/commands/connect/` module now contains a custom, extremely robust P2P terminal engine (handling ANSI parsing, inline editors, input history, and autocomplete).
- **Automation Pipeline**: We implemented a `--json` and `--yes` automation framework. The `cli/src/output.rs` module ensures that when `--json` is passed, zero UI components are printed to `stdout`, allowing for perfect bash/PowerShell scriptability.
- **Fat Library, Thin CLI**: The `irosh` library crate does absolutely zero UI printing or argument parsing. It strictly returns typed `Result`s and structured data.
- **Authentication & Security**: The Auth lifecycle (`src/auth.rs`) has been deeply tested to handle passwords, TOFU, and Vault checks via Argon2id.

## 2. Windows Implementation Handoff

## 🚨 Critical Fixes Applied (2026-05-09)
The following issues have been resolved to ensure the background daemon is reliable:

1.  **IPC Deadlock Resolved**: Fixed a hang where `IpcClient` didn't shut down the write-half of Named Pipes on Windows, causing the server to wait forever for EOF.
2.  **State Path Synchronization**: Fixed a mismatch where the background service used `~/.irosh/` while the CLI looked in `~/.irosh/server/`. Both are now synchronized to `~/.irosh/server/`.
3.  **Service Startup Patience**: Added a 3-second retry loop to the `wormhole` command. This handles cases where the Windows service is reported "Running" by SCM but the Iroh network stack is still initializing.

## 🛠️ Windows-Specific Architecture
- **Binary**: `irosh.exe` is a "fat binary" containing both CLI and Server logic.
- **Service Manager**: Integrated with Windows Service Control Manager (SCM) via the `windows-service` crate.
- **Service Name**: `irosh`
- **Default State**: `%USERPROFILE%\.irosh\server` (Server) and `%USERPROFILE%\.irosh\client` (CLI).
- **IPC Mechanism**: Named Pipe: `\\.\pipe\irosh-service-<hash_of_state_dir>`

## 📝 Action Items for Windows Development
### 1. Verification of Background Daemon
Run the following to ensure the IPC and Service sync is working on your machine:
```powershell
# 1. Install and Start
irosh system install

# 2. Check status via IPC (Wait ~2 seconds for Iroh to bind)
irosh system status

# 3. Enable a wormhole and verify it stays in background
irosh wormhole --code windows-test
irosh wormhole status
```

### 2. ConPTY Fine-Tuning
The `src/sys/windows/pty.rs` includes a manual translation for arrow keys and special characters. If you notice specific keys (like PageUp/PageDown) aren't working in an SSH session to a Windows host, they should be added to the `match` block in `AsyncStdin::new`.

### 3. Permission Validation
Ensure that after running a command that writes state (like `irosh passwd set`), the files in `~/.irosh/server` have restricted access. Check File Explorer -> Properties -> Security (it should only show your user account).

## ⚠️ No-Regression Rule
**Do not modify the generic logic in `irosh-cli/src/commands/` or `irosh/src/server/`.**
All Windows-specific fixes must stay within `#[cfg(windows)]` blocks or inside the `src/sys/windows/` module to ensure we don't break the Linux/macOS builds.

Good luck!

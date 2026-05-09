# Windows Migration Handoff (v0.3.0-rc.1)

Welcome! If you are picking up this codebase on a Windows machine to finalize the Windows-specific functionality, this document is for you.

## 1. What Just Changed Since v0.2.0?

We have just completed a massive overhaul of the Linux/Unix side of the codebase and established a hardened, strictly separated architecture:

- **Terminal UX Engine**: The `cli/src/commands/connect/` module now contains a custom, extremely robust P2P terminal engine (handling ANSI parsing, inline editors, input history, and autocomplete).
- **Automation Pipeline**: We implemented a `--json` and `--yes` automation framework. The `cli/src/output.rs` module ensures that when `--json` is passed, zero UI components are printed to `stdout`, allowing for perfect bash/PowerShell scriptability.
- **Fat Library, Thin CLI**: The `irosh` library crate does absolutely zero UI printing or argument parsing. It strictly returns typed `Result`s and structured data.
- **Authentication & Security**: The Auth lifecycle (`src/auth.rs`) has been deeply tested to handle passwords, TOFU, and Vault checks via Argon2id.

## 2. Windows Separation of Concerns

**You do NOT need to touch the Linux code.**
The core architecture has been successfully decoupled:
- `src/sys/unix/` and `src/sys/linux/` (Handles Linux PTY and Systemd Daemons).
- `src/sys/windows/` (Handles Windows ConPTY and Windows Services).

The CLI layer dynamically interfaces with these through `src/sys/mod.rs` and `src/sys/service.rs`.

## 3. Your Objectives on Windows

Your primary goal is to finalize the background daemon implementation for Windows so that `irosh system install` and `irosh system start` work exactly like they do on Linux.

1. **Windows Service Implementation**:
   - Check `src/sys/windows/service.rs`.
   - Implement the `ServiceControlManager` bindings (likely using `windows-service` or the `windows` crate).
   - Ensure the daemon starts the `irosh host` process in the background.

2. **Windows ConPTY**:
   - The PTY code in `src/sys/windows/pty.rs` needs a final check to ensure the Windows Pseudo-Console handles resizing and interactive inputs flawlessly.

## 4. Verification

After you implement the Windows service bindings, run the following to verify the cross-platform separation has not been broken:

```powershell
# 1. Ensure the code compiles flawlessly on MSVC
cargo check --workspace
cargo clippy --all-targets --all-features -- -D warnings

# 2. Test the automation pipeline locally
$env:IROSH_PASSWORD="MySecurePassword"
cargo run --bin irosh -- passwd set
cargo run --bin irosh -- system status --json
```

**Rule of Thumb**: Do not modify the generic `cli/src/` logic. Only modify code inside `src/sys/windows/`. If you find yourself needing to change the generic CLI prompt behavior to make Windows work, stop and rethink the design!

Good luck!

# Windows Cross-Platform Handover

## Objective
The goal is to achieve full cross-platform parity for the Irosh (P2P SSH over Iroh) project on Windows, following the strict quality guidelines in `CLAUDE.md`.

## Current Status
Most core features have been implemented and verified on Windows, but integration tests (specifically recursive file transfers) are still timing out in the test environment.

### Implemented Features (Windows)
1.  **PTY Support**:
    - `src/session/pty.rs`: Implemented `current_terminal_size()` using Win32 API.
    - `RawTerminal` now enables `ENABLE_VIRTUAL_TERMINAL_PROCESSING` for correct ANSI rendering on Windows.
    - Background polling task in `cli/src/commands/connect/mod.rs` detects window resizes and sends PTY resize requests to the server.
2.  **Signal Handling**:
    - `src/server/handler/pty.rs`: Implemented `forward_signal` using `GenerateConsoleCtrlEvent` for `SIGINT` (Ctrl+C).
3.  **File Transfers**:
    - `src/server/transfer/helpers.rs`: Refactored to use `UploadSink` and `DownloadSource` abstractions.
    - **Optimization**: On Windows, the server now uses direct `tokio::fs` operations instead of spawning expensive PowerShell processes for every file in a transfer.
    - **is_remote_dir**: Client-side directory detection now handles PowerShell vs Shell differences based on remote peer metadata.
4.  **Shell Context**:
    - `ShellContext` now uses native `tokio::fs` for path operations on Windows, bypassing namespace-specific logic used on Linux.

### Known Issues & Blockers
1.  **Integration Test Timeouts**:
    - `test_recursive_directory_transfer` in `tests/integration.rs` frequently times out even with a 120s (now 300s) limit.
    - **Hypothesis**: Iroh connectivity (relaying) combined with ConPTY startup overhead or PowerShell probing might be causing the delay.
    - **Recent Fix**: Refactored server helpers to avoid PowerShell for file I/O. This significantly improved local performance, but P2P latency remains a factor.
2.  **Windows File Locking**:
    - Linker errors (`LNK1104`) often occur during rapid test cycles if previous integration test processes aren't fully terminated. Use `taskkill /IM integration* /F` to clear them.

## Recommended Next Steps
1.  **Debug Recursive Transfer**:
    - Run `cargo test --test integration test_recursive_directory_transfer -- --nocapture` and observe the logs.
    - Verify if the `is_remote_dir` probe (using PowerShell) is the bottleneck. If so, consider a faster way to probe remote file types.
2.  **Verify Terminal Interactions**:
    - Test interactive shells (`irosh connect <ticket>`) on Windows to ensure Ctrl+C correctly interrupts remote processes without killing the session.
3.  **Code Quality**:
    - Run `cargo clippy --all-targets --all-features -- -D warnings`.
    - Run `cargo fmt`.

## Architecture Notes
- **Portable-PTY**: Uses ConPTY on Windows. ConPTY is sensitive to process termination; the server handler uses a shared master handle to ensure the session is torn down correctly when the child exits.
- **Transfer Helpers**: Always prioritize `tokio::fs` over spawning processes on Windows. Spawning `powershell.exe` is very slow (~500ms-1s per call) and should be avoided in loops.

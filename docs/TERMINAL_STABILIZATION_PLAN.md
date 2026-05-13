# Irosh Terminal & PTY Stabilization Plan

This document outlines the architectural hardening of terminal and PTY handling in Irosh, based on research into production-grade implementations (specifically WezTerm) and identification of specific platform failures in the current Irosh v0.3.0 codebase.

## 1. Identified Technical Failures

### A. Windows Service Signal Failure (Critical)
*   **Location**: `src/server/handler/pty.rs:680`
*   **Problem**: Use of `GenerateConsoleCtrlEvent` to propagate `Ctrl+C` / `Ctrl+Break`.
*   **The "Intel"**: Windows API documentation states this function only works if the calling process has an attached console. Because `irosh host` runs as a Windows Service (headless), it has no console, causing this call to fail silently.
*   **Impact**: Remote processes (like `ping`, `tail`, or long-running scripts) cannot be interrupted by the user.

### B. Windows Code-Page (Encoding) Mismatch
*   **Location**: `src/server/handler/pty.rs:140`
*   **Problem**: UTF-8 is only enforced for "Exec" commands (one-off tasks), not for interactive login shells (PowerShell/CMD).
*   **The "Intel"**: The remote shell defaults to the system code page (e.g., CP 437), but the Irosh client sends UTF-8. 
*   **Impact**: Corrupted input for escape sequences (`~`), arrow keys, and non-ASCII characters.

### C. Local Prompt Race Condition (`~c` Mode)
*   **Location**: `cli/src/commands/connect/session.rs` and `input.rs`
*   **Problem**: The `irosh> ` prompt is written to the stream concurrently with the `\x1b[?1049h` (Alternate Screen) switch.
*   **The "Intel"**: The prompt is often "caught" on the main screen before the switch completes, or is missing on the alternate screen.
*   **Impact**: Visual glitches, erratic cursor jumps, and "frozen" text during local command execution.

### D. Manual Input Translation (Legacy Debt)
*   **Location**: `src/sys/windows/pty.rs`
*   **Problem**: Manual translation of `VirtualKey` codes into ANSI sequences.
*   **The "Intel"**: This approach fails to handle complex modifiers (Ctrl+Shift+Arrow) and modern protocols like the "Kitty Keyboard Protocol."
*   **Impact**: Inconsistent behavior compared to high-performance terminal emulators.

## 2. Proposed Architectural Changes (WezTerm Patterns)

### I. Unified Terminal Abstraction
*   Replace manual ANSI printing (`\x08`, `\x1b[K`) with a robust rendering logic.
*   Use `CSI` sequences for column-aware cursor management (`\x1b[G`, `\x1b[K`).
*   Ensure the `InputEngine` is aware of the current terminal width to handle line-wrapping during local edits.

### II. Suspended-State UI
*   When entering `LocalEdit` mode (`~`), the `drive_session` loop will:
    1.  **Suspend** processing of remote PTY data (buffer it).
    2.  **Execute** the switch to the Alternate Screen buffer.
    3.  **Initiate** a dedicated LineEditor session.
    4.  **Restore** the main screen and resume PTY processing only after completion.

### III. Native VT Input (Windows)
*   Enable `ENABLE_VIRTUAL_TERMINAL_INPUT` on the Windows client.
*   Drop manual `VirtualKey` mapping.
*   Read raw ANSI bytes directly from the console driver, ensuring perfect parity with the OS.

### IV. Hardened Signal Propagation
*   Implement a robust signal delivery mechanism for Windows services that does not rely on an attached console.
*   Enforce UTF-8 (Code Page 65001) for **all** server-side PTY sessions during initialization.

## 3. Phase-by-Phase Roadmap

### Phase 1: Server Hardening (Windows & Unix)
- [ ] Implement `chcp 65001` enforcement for login shells.
- [ ] Fix signal delivery for Windows Services.
- [ ] Ensure `SIGWINCH` propagation is atomic.

### Phase 2: Client Input Overhaul
- [ ] Implement `ENABLE_VIRTUAL_TERMINAL_INPUT` for Windows.
- [ ] Standardize `TerminalEvent` to include `Resize` natively on all platforms.
- [ ] Refactor `AsyncStdin` to be a unified event stream.

### Phase 3: The Prompt & Mode Refactor
- [ ] Fix the Alternate Screen switch race condition.
- [ ] Implement "Suspended PTY" state during `~` commands.
- [ ] Add line-wrapping awareness to the `InputEngine`.

### Phase 4: Validation & Parity
- [ ] Verify `~get` and `~put` stability under heavy remote data load.
- [ ] Validate Ctrl+C behavior across all Service/Interactive combinations.
- [ ] Finalize cross-platform PTY resize math.

# 🚀 Windows Terminal Stabilization Handoff

## 🎯 Context for Windows Agent
We have just completed a major architectural hardening of the Irosh terminal and PTY subsystem. The focus was on eliminating UI race conditions, cursor desync, and "disappearing output" bugs that were previously plaguing the Windows experience.

## 🛠️ Key Changes Implemented
1.  **Native VT Input**: Switched the Windows client to `ENABLE_VIRTUAL_TERMINAL_INPUT`. This delegates key-mapping to the OS, resolving brittle escape sequence handling.
2.  **Global UTF-8 Enforcement**: Both Server and Client now strictly enforce **CP 65001 (UTF-8)** for Windows interactive shells.
3.  **Atomic Buffer Transitions**: Implemented explicit Alternate Screen Buffer management (`\x1b[?1049h/l`) to prevent the `irosh>` prompt from colliding with the main session buffer.
4.  **CSI-Based Rendering**: The local editor now uses absolute CSI positioning (`\r\x1b[K`) for redrawing. This is much more robust against line-wraps than the old backspace (`\x08`) method.
5.  **Signal Injection**: Added a fallback byte-injection (`\x03`) for `SIGINT` to support Ctrl+C in headless Windows Service environments.

## 🧪 Critical Windows Verification Tasks
The following items MUST be verified on the Windows machine:
- [ ] **Prompt Stability**: Run `~get` or `~put` and confirm the `irosh>` prompt reprints correctly *below* the progress output.
- [ ] **Resize Resilience**: Resize the Windows Terminal during an active shell session and verify the cursor doesn't jump or "ghost."
- [ ] **Ctrl+C in Service**: Test if Ctrl+C successfully interrupts a long-running command when Irosh is running as a Windows Service.
- [ ] **Alternate Screen Cleanup**: Connect, run a command that uses an alternate screen (e.g., `vim` or `htop` if available), then disconnect and ensure the terminal is restored to its original state.

## 📑 Strategic Documents
Refer to these for the full architectural vision:
- `docs/TERMINAL_STABILIZATION_PLAN.md`: The blueprint we followed.
- `docs/STABILIZATION_AND_POLISH_PLAN.md`: The roadmap for the next phase (Intelligence & UI Polish).

**Current Status**: All tests pass on Linux. Workspace is clippy-clean and formatted. Ready for Windows verification.

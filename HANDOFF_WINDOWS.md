# 🚀 Windows Terminal Stabilization Handoff

## 🎯 Context for Windows Agent
We have just completed a major architectural hardening of the Irosh terminal and PTY subsystem. The focus was on eliminating UI race conditions, input hangs, and making Ctrl+C highly responsive on Windows during network wait states.

## 🛠️ Key Changes Implemented
1.  **Unified Input Reader**: Removed the `crossterm::event::EventStream` dependency. We now strictly read raw bytes from `AsyncStdin` in a single event loop. This solves the "Input Hang" bug where a background thread was stealing keystrokes on Windows.
2.  **Ctrl+C Signal Traps**: The `tokio::signal::ctrl_c()` listener is now embedded in `tokio::select!` blocks around all long-running network tasks (`connect_wormhole`, `dial_p2p`, `establish_session`, and `Server::bind`). This guarantees Ctrl+C will kill the process on Windows even when VT Input mode normally swallows the signal.
3.  **Prompt Wake-Up Sequence**: The input engine now sends a `\r` (Carriage Return) when returning to the remote session. This forces the remote shell to reprint its prompt immediately, solving the "blank screen / must press Enter manually" issue.
4.  **UX Spacing**: Local commands (e.g., `ls`, `lpwd`) now automatically insert a clean blank line (`\r\n`) before re-printing the `irosh>` prompt, matching standard professional CLI behavior.
5.  **Strict VT Enforcement**: `TerminalGuard` explicitly sets `ENABLE_VIRTUAL_TERMINAL_PROCESSING` and `DISABLE_NEWLINE_AUTO_RETURN` on Windows, with a robust `nuclear_cleanup` on drop.

## 🧪 Critical Windows Verification Tasks
The following items MUST be verified on the Windows machine:
- [ ] **Connection Cancel**: Run `irosh connect` to a peer that is offline (or taking a long time), and press Ctrl+C. It should instantly cancel and exit without hanging.
- [ ] **Prompt Refresh**: Enter `~c` to get the `irosh>` prompt, then type `exit`. The remote shell prompt should instantly reappear without needing to press Enter.
- [ ] **Visual Spacing**: Type `~ls` and ensure there is a single blank line between the directory output and the next `irosh>` prompt.
- [ ] **Flicker-Free Input**: Type characters in the `irosh>` prompt. They should not flicker or ghost (they use `\r\x1b[K` for absolute clearing).
- [ ] **Clean Exit**: When disconnecting, the terminal should be fully restored (colors reset, cursor visible, raw mode disabled).

## 📑 Current Status
- All tests pass on Linux (`cargo test`).
- The workspace is formatted (`cargo fmt`) and clippy-clean with zero warnings (`cargo clippy`).
- We are ready for Windows verification.
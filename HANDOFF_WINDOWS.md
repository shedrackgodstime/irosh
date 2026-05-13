# 🚀 Windows Terminal Stabilization Handoff

## 🎯 Context for Windows Agent
We have just completed a major architectural hardening of the Irosh terminal and PTY subsystem. We completely **abandoned the Alternate Screen Buffer** approach in favor of a **"True Transparency" / Non-Destructive UI** architecture.

Previously, running local commands (`~c`, `~get`) would hijack the screen or overwrite the remote shell's prompt (like `└─$`). We fixed this by respecting the main scrollback buffer and surgically rendering local UI elements.

## 🛠️ Key Architectural Changes Implemented
1. **NO Alternate Screen Buffers**: We stripped out all `\x1b[?1049h/l` usage. All local commands and prompts (`irosh>`) now operate directly on the user's main terminal buffer, ensuring full scrollback history retention.
2. **Non-Destructive Local Rendering**: When the user enters escape mode (`~`), the CLI no longer issues line-clearing commands (`\r\x1b[K`). It echoes characters exactly where the cursor is, preserving multi-line prompts (like Kali Linux's `└─$`).
3. **Surgical Backspace & Tab Completion**: If the user backspaces or tabs in escape mode, we use relative ANSI movements (`\x1b[1D\x1b[K`) to clear only the typed characters, leaving the remote prompt untouched.
4. **Vertical Hygiene**: Purged redundant newlines (`\r\n`) in the `print_prompt!` and file transfer logic to prevent "double spacing" between the remote shell and local commands.
5. **Start-of-Line Sync**: The `observe_remote` logic now correctly invalidates the "start of line" state if the remote shell sends data (e.g., a prompt) after a newline, preventing the `~` command from clobbering the shell.

## 🧪 Critical Windows Verification Tasks
The following items MUST be verified on the Windows machine by the AI:
- [ ] **Prompt Preservation**: Run `~put` or `~get`. Ensure the `~` command appears *after* the Windows remote prompt without deleting or overwriting it.
- [ ] **Scrollback History**: Run `~c`, run a command like `ls`, and then type `exit`. Confirm that the `irosh>` prompt and its output *remain visible* in the history when you return to the remote shell.
- [ ] **Vertical Spacing**: Ensure there are no massive gaps or double-newlines between the remote shell and the `irosh>` prompt.
- [ ] **Ctrl+C / Resize Resilience**: Verify resizing the window during a transfer or pressing Ctrl+C inside `~c` behaves cleanly.

## 📑 Strategic Documents
- `docs/TERMINAL_STABILIZATION_PLAN.md`: The blueprint for this stabilization.
- `docs/STABILIZATION_AND_POLISH_PLAN.md`: The roadmap for the next phase (Intelligence & UI Polish).

**Current Status**: All tests pass on Linux. The workspace is Clippy-clean and formatted. The terminal logic is 100% transparent and non-destructive. Ready for Windows verification.

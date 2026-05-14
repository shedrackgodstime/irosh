# 🎨 Irosh UX & Terminal Guidelines

This document serves as the absolute source of truth for the **User Experience (UX)** and **Terminal Rendering Philosophy** of Irosh. 

Any AI agent, contributor, or maintainer working on the terminal interface MUST strictly adhere to these guidelines. **Do not introduce hacky workarounds (like dummy newlines) or secondary screen buffers to solve rendering issues.**

---

## 1. The Core Philosophy: "True Transparency"
Irosh is designed to feel like a native, seamless extension of the user's terminal. It is a "Thin CLI" that respects the user's history, screen real estate, and the state of the remote shell.

- **No Screen Hijacking:** Local interactions (like `~put`, `~get`, or the `irosh>` prompt) must coexist gracefully with the remote shell's output.
- **Scrollback Preservation:** Everything a user does must remain visible in their permanent terminal scrollback.

## 2. No Alternate Screen Buffers
We **DO NOT** use Alternate Screen Buffers (`\x1b[?1049h` / `\x1b[?1049l`) for standard local commands or prompts.
- **Why?** Switching buffers completely hides the user's remote session and wipes their local commands from the terminal history once finished.
- **Rule:** All `irosh>` prompts, transfer progress bars, and escape commands (`~`) must be rendered directly onto the main terminal buffer.

## 3. Non-Destructive Local Rendering
When a user begins an escape sequence (e.g., typing `~put`), Irosh must intercept and render this input **non-destructively**.
- **Preserve Remote Prompts:** The remote shell (especially complex ones like Kali's `zsh` or Windows PowerShell) may have printed a multi-line prompt (e.g., `└─$`). Irosh MUST NOT clear the line (`\r\x1b[K`) when echoing the `~` character.
- **Surgical Backspacing:** If a user backspaces or triggers tab completion during an escape command, use relative ANSI movements (e.g., `\x1b[1D\x1b[K`) to clear only the local characters, leaving the remote prompt untouched.

## 4. Returning to the Remote Shell (The "OpenSSH Pattern")
When a local command finishes (e.g., a file transfer completes) or the user exits the `irosh>` prompt, we return control to the remote shell.
- **DO NOT Force Redraws:** Never send a hidden "Enter" (`\r` or `\n`) to the remote shell to force it to reprint its prompt.
- **Why?** Stateful PTYs (like Windows ConPTY) or complex `zsh` prompts use absolute cursor positioning. If forced to redraw, they will jump back up the screen and instantly overwrite/erase the local command output we just printed.
- **The Correct UX:** Simply leave the cursor on a fresh, blank line below the local output. The user can type their next command blindly (it will echo normally) or press `Enter` to request a fresh prompt. This matches the established UX of OpenSSH's `~C` escape sequence.

## 5. Vertical Hygiene
Irosh UI elements must be tight and professional.
- **No Double Spacing:** Do not inject redundant `\r\n` characters before or after local prompts. 
- **Clean Transitions:** The transition from a remote shell output to a local `irosh>` prompt should take exactly one line break.

## 6. Windows-to-Linux & Cross-Platform Consistency
The UX must be identical regardless of whether the user is running the client on Linux, Windows, or macOS. 
- If a rendering bug appears on Windows, **do not assume ConPTY is broken** and attempt to hack the server. Investigate how local ANSI sequences (like `indicatif` progress bars) are interacting with the local terminal emulator.
- Always use `ENABLE_VIRTUAL_TERMINAL_INPUT` and `ENABLE_VIRTUAL_TERMINAL_PROCESSING` on Windows to rely on the OS's native VT parsing rather than writing custom Windows Console API shims.

---

**⚠️ Directives for AI Agents:** If you encounter a bug where text disappears or the screen flickers, **refer to this document before touching `input.rs` or `prompt.rs`**. Do not guess. Ensure your fix aligns with "True Transparency" and "Non-Destructive Local Rendering."

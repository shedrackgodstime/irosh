# 🚀 Permanent Terminal Solution for Irosh

This document defines the final, stable architecture for the Irosh terminal interface. It is designed to eliminate Windows ConPTY corruption, "Tab" glitches, and unpredictable escape behavior.

## 1. The Core Pillar: Separation of Concerns
We must stop treating the "Remote Shell" and "Local Prompt" as the same stream. They require different handling logic.

### A. Remote Mode (The Transparent Pipe)
- **Mechanism**: Pure raw byte passthrough (`stdin.read() -> channel`).
- **Processing**: Zero. No UTF-8 validation, no ANSI parsing.
- **Why**: This is the only way to support `vim`, `htop`, and multi-byte character sequences without corruption.

### B. Local Prompt Mode (The Event-Driven Editor)
- **Mechanism**: `crossterm::event::read()` for structured keyboard events.
- **Processing**: Convert `KeyEvent` into actions (Backspace, Home, End, Tab).
- **Redraw Strategy**: **Forward-Only Line Redraw**.
  - Instead of backspacing (`\x08`), use `\x1b[G\x1b[K` (Move to Col 0, Clear Line).
  - Immediately print `irosh> [buffer]`.
  - This is atomic and prevents Windows ConPTY from "guessing" the cursor position.

## 2. Solving the "Tab Corruption"
- **Problem**: Completion results currently "clobber" the prompt line.
- **Solution**:
  1. Detect Tab event.
  2. Clear current prompt line using `\x1b[G\x1b[K`.
  3. Print completions on the lines *below* (as a fresh record in scrollback).
  4. Redraw the prompt on the *new* bottom line.
- **Result**: No "teleportation" bugs. The completions remain in the terminal history.

## 3. The "Nuclear Cleanup" (Session Handoff)
Whenever Irosh hands control back to the remote shell (after a prompt or a `~put` transfer):
- Send the **Universal Cleanup Sequence**: `\x1b[0m\x1b[?25h\x1b[G\x1b[K`.
- This ensures the cursor is visible, styles are reset, and the line is clean.

## 4. Persistent History
- **Location**: `~/.irosh_history`
- **Implementation**: Write the `prompt_history` vector to disk on every successful command execution. Load it on startup.

## 5. Cross-Platform VT Enforcement
On Windows, we must explicitly ensure the following flags are set via `SetConsoleMode`:
- `ENABLE_VIRTUAL_TERMINAL_PROCESSING` (Output)
- `ENABLE_VIRTUAL_TERMINAL_INPUT` (Input)
- `DISABLE_NEWLINE_AUTO_RETURN` (Prevent "staircase effect")

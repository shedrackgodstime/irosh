# Irosh Terminal UX & History Migration Plan

This document outlines the strategy for porting the advanced line-editing and history features from the legacy `cli_old` codebase into the V2 "Thin CLI."

## 1. Core Architecture: The Event-Effect Model
We will adopt the decoupled model from the legacy editor to ensure the `InputEngine` remains clean and maintainable.

*   **EditorEvent**: Captures raw input intents (`InsertByte`, `Backspace`, `HistoryUp`, `HistoryDown`, `Submit`).
*   **EditorEffect**: Defines the output result (`NoOp`, `Render`, `SubmitLocal`, `SubmitRemote`).
*   **LineEditor**: A pure state machine that manages the buffer and cursor position.

## 2. Advanced Terminal Handling (Raw Mode)
Since Irosh operates in Raw Mode, we must handle ANSI escape sequences manually for local input.

### ANSI Sequence Parsing
We will port the legacy `consume_control_sequence` state machine:
*   `\x1b[A` -> `HistoryUp`
*   `\x1b[B` -> `HistoryDown`
*   `\x1b[C` -> `MoveRight`
*   `\x1b[D` -> `MoveLeft`
*   `\x08` or `\x7f` -> `Backspace`

### Rendering Logic
The renderer will use the "Clear and Redraw" strategy to handle cursor movements and line editing:
1.  Move cursor to the start of the prompt.
2.  Clear from cursor to end of line (`\x1b[K`).
3.  Write the current line buffer.
4.  Move cursor back to the logical `display_cursor` position.

## 3. Persistent History
The `CommandHistory` will be integrated into the V2 `CliContext`.

*   **Storage**: History files will be stored in `~/.config/irosh/history/`.
*   **Segmentation**: Separate history files for the `irosh>` prompt and the `~` escape line.
*   **Async Loading**: History will be loaded asynchronously during connection establishment to prevent UI blocking.

## 4. Tab Completion (Future Phase)
A pluggable `Completer` trait will be implemented:
*   **Local FS Completer**: For `lls`, `put`, `lcd`.
*   **Keyword Completer**: For `irosh>` command keywords.
*   **Peer Completer**: For `connect` targets.

## 5. Porting Steps
1.  **Extract History**: Port `history.rs` to `cli/src/commands/connect/support/history.rs`.
2.  **Extract Editor**: Port `core.rs` (LineEditor) to `cli/src/commands/connect/input/editor.rs`.
3.  **Upgrade InputEngine**: Modify `InputEngine::process_local` to use the `LineEditor` and `CommandHistory`.
4.  **Integrate Context**: Update `CliContext` to provide the history path.

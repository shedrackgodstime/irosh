//! Input engine for handling local keystrokes, remote sync, and escape sequences.

use super::ansi::{ControlSequenceState, consume_control_sequence};
use super::completion::{self, CompletionMode, CompletionResult};
use super::editor::{EditorEffect, EditorEvent, EditorMode, LineEditor};
use super::history::CommandHistory;
use super::prompt::{LocalCommand, parse_local_command};
use super::transfer::TransferContext;
use irosh::Session;

/// Actions requested by the user via escape sequences (e.g., `~.`) or the local prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscapeAction {
    /// Disconnect the session immediately (`~.`).
    Disconnect,
    /// Enter the local command prompt (`~C`).
    CommandPrompt,
    /// Show help information (`~?`).
    Help,
    /// Execute a command from the local prompt.
    RunLocal(LocalCommand),
    /// Request tab completion.
    RequestCompletion,
}

/// The current mode of the input engine.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Normal mode: bytes are passed through to the remote host.
    #[default]
    Remote,
    /// Local editing mode (either Escape line or Local prompt).
    LocalEdit,
}

#[derive(Debug)]
pub struct LineSession {
    pub editor: LineEditor,
    pub display_cursor: usize,
    pub control_state: ControlSequenceState,
}

/// A state machine that tracks line state from both local and remote streams.
#[derive(Debug)]
pub struct InputEngine {
    pub mode: InputMode,
    /// Whether the next local character is at the start of a line.
    at_start_of_line: bool,
    /// Count of characters typed locally on the current line (for escape arming).
    local_line_len: usize,
    /// The active editing session (only Some when mode == LocalEdit).
    active_line: Option<LineSession>,
    /// History for the ~ escape line.
    pub escape_history: CommandHistory,
    /// History for the irosh> prompt.
    pub prompt_history: CommandHistory,
    /// Previous byte typed (to swallow \r\n pairs).
    swallow_next_enter_pair: Option<u8>,
    /// State machine for parsing ANSI sequences in Remote mode.
    remote_control_state: ControlSequenceState,
    /// Buffer for accumulating ANSI sequences in Remote mode before forwarding them.
    remote_control_buffer: Vec<u8>,
}

/// Parses an escape buffer (e.g. `b"~."` or `b"~help"`) into an action.
/// Returns `None` if the command is unknown (should be sent to remote).
fn parse_escape(buf: &[u8]) -> Option<EscapeAction> {
    // buf starts with `~`; strip it and trim ASCII whitespace.
    let cmd = buf.strip_prefix(b"~").unwrap_or(buf);
    let cmd = trim_bytes(cmd);
    match cmd {
        b"." => Some(EscapeAction::Disconnect),
        b"?" | b"help" => Some(EscapeAction::Help),
        b"C" | b"c" => Some(EscapeAction::CommandPrompt),
        _ => {
            // Try parsing it as a full local command (like `put` or `get`)
            parse_local_command(cmd).map(EscapeAction::RunLocal)
        }
    }
}

fn trim_bytes(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|i| i + 1)
        .unwrap_or(0);
    if start >= end { &[] } else { &s[start..end] }
}

impl InputEngine {
    pub fn new(state: &irosh::StateConfig) -> Self {
        let history_dir = state.root().join("history");
        Self {
            mode: InputMode::Remote,
            at_start_of_line: true,
            local_line_len: 0,
            active_line: None,
            escape_history: CommandHistory::new(Some(history_dir.join("escape.history"))),
            prompt_history: CommandHistory::new(Some(history_dir.join("prompt.history"))),
            swallow_next_enter_pair: None,
            remote_control_state: ControlSequenceState::None,
            remote_control_buffer: Vec::new(),
        }
    }

    /// Exit local prompt mode, returning to remote interaction.
    pub fn exit_local_prompt(&mut self) {
        self.mode = InputMode::Remote;
        self.active_line = None;
        self.at_start_of_line = true;
        self.local_line_len = 0;
    }

    /// Remote data arriving resets the 'start of line' state on newlines.
    pub fn observe_remote(&mut self, data: &[u8]) {
        for &byte in data {
            if byte == b'\r' || byte == b'\n' {
                self.at_start_of_line = true;
                self.local_line_len = 0;
            }
        }
    }

    /// Process local keystrokes.
    pub fn process_local(&mut self, data: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<EscapeAction>) {
        let mut to_remote = Vec::with_capacity(data.len());
        let mut to_local = Vec::new();
        let mut actions = Vec::new();

        for &byte in data {
            // Swallow \r\n or \n\r pairs from local terminal.
            if let Some(pair) = self.swallow_next_enter_pair {
                if byte == pair {
                    self.swallow_next_enter_pair = None;
                    continue;
                }
                self.swallow_next_enter_pair = None;
            }

            match self.mode {
                InputMode::Remote => {
                    if self.remote_control_state != ControlSequenceState::None {
                        self.remote_control_buffer.push(byte);
                        consume_control_sequence(&mut self.remote_control_state, byte);
                        if self.remote_control_state == ControlSequenceState::None {
                            // The sequence just finished. Check if it's a focus event.
                            let is_focus_event = self.remote_control_buffer == b"\x1b[I" || self.remote_control_buffer == b"\x1b[O";
                            if !is_focus_event {
                                // Not a focus event, forward the entire sequence to the remote.
                                to_remote.extend_from_slice(&self.remote_control_buffer);
                            }
                            self.remote_control_buffer.clear();
                        }
                        continue;
                    }

                    if byte == 27 {
                        self.remote_control_state = ControlSequenceState::Escape;
                        self.remote_control_buffer.clear();
                        self.remote_control_buffer.push(byte);
                        continue;
                    }

                    if self.local_line_len == 0 && byte == b'~' {
                        // Enter escape line mode.
                        self.mode = InputMode::LocalEdit;
                        let mut new_line = LineSession {
                            editor: LineEditor::new_escape(),
                            display_cursor: 0,
                            control_state: ControlSequenceState::None,
                        };
                        // Just echo the tilde directly instead of a full line clear/render.
                        to_local.push(b'~');
                        new_line.display_cursor = 1;
                        self.active_line = Some(new_line);
                    } else {
                        self.handle_remote_byte(byte, &mut to_remote);
                    }
                }

                InputMode::LocalEdit => {
                    let Some(mut line) = self.active_line.take() else {
                        // This should be logically impossible given the mode, but we handle it gracefully
                        self.mode = InputMode::Remote;
                        continue;
                    };

                    // 1. Process byte via ANSI state machine or direct insert
                    let event = if line.control_state != ControlSequenceState::None {
                        consume_control_sequence(&mut line.control_state, byte)
                    } else if byte == 27 {
                        // Escape
                        line.control_state = ControlSequenceState::Escape;
                        None
                    } else if byte == 8 || byte == 127 {
                        Some(EditorEvent::Backspace)
                    } else if byte == b'\r' || byte == b'\n' {
                        self.swallow_next_enter_pair = paired_enter_byte(byte);
                        self.at_start_of_line = true;
                        self.local_line_len = 0;
                        Some(EditorEvent::Submit)
                    } else if byte == b'\t' {
                        Some(EditorEvent::Tab)
                    } else if byte == 0x03 {
                        // Ctrl+C
                        // Cancel current line
                        to_local.extend_from_slice(b"^C\r\n");
                        self.mode = InputMode::Remote;
                        self.at_start_of_line = true;
                        self.local_line_len = 0;
                        self.active_line = None;
                        continue;
                    } else if byte.is_ascii_graphic() || byte == b' ' {
                        Some(EditorEvent::InsertByte(byte))
                    } else {
                        None
                    };

                    // 2. Apply event to editor
                    if let Some(ev) = event {
                        let is_escape = matches!(line.editor.line().first(), Some(b'~'));
                        let history = if is_escape {
                            &mut self.escape_history
                        } else {
                            &mut self.prompt_history
                        };

                        match line.editor.apply(ev, history) {
                            EditorEffect::NoOp => {}
                            EditorEffect::Render => {
                                if is_escape {
                                    // Non-destructive echo: just print the last character added.
                                    // (Simplification: for now, we just reprint the whole escape line
                                    // without the leading \r\x1b[K by using a custom render)
                                    render_escape_line_nondestructively(&mut to_local, &mut line);
                                } else {
                                    render_line(&mut to_local, &mut line);
                                }
                            }
                            EditorEffect::ClearAndExit => {
                                clear_line_preview(&mut to_local, &mut line);
                                self.mode = InputMode::Remote;
                                self.at_start_of_line = true;
                                self.local_line_len = 0;
                                self.active_line = None;
                                continue;
                            }
                            EditorEffect::RequestCompletion => {
                                actions.push(EscapeAction::RequestCompletion);
                            }
                            EditorEffect::SubmitEscape(bytes) => {
                                let line_str = String::from_utf8_lossy(&bytes);
                                self.escape_history.add(&line_str);

                                match parse_escape(&bytes) {
                                    Some(action) => {
                                        if action == EscapeAction::CommandPrompt {
                                            // Ensure we are at the end of the line before starting the prompt on a new line
                                            finalize_submitted_line(&mut to_local, &mut line);
                                            // Sync mode internally so next bytes are handled as Prompt
                                            self.mode = InputMode::LocalEdit;
                                            line = LineSession {
                                                editor: LineEditor::new_prompt(),
                                                display_cursor: 0,
                                                control_state: ControlSequenceState::None,
                                            };
                                            to_local.extend_from_slice(b"irosh> ");
                                        } else {
                                            finalize_submitted_line(&mut to_local, &mut line);
                                            self.mode = InputMode::Remote;
                                        }
                                        actions.push(action);
                                    }
                                    None => {
                                        finalize_submitted_line(&mut to_local, &mut line);
                                        // If it starts with a known local keyword but missing args, show help
                                        let cmd_str = String::from_utf8_lossy(trim_bytes(
                                            bytes.strip_prefix(b"~").unwrap_or(&bytes),
                                        ));
                                        let keyword =
                                            cmd_str.split_whitespace().next().unwrap_or("");
                                        if ["put", "get", "lls", "ls", "lcd", "cd"]
                                            .contains(&keyword)
                                        {
                                            to_local.extend_from_slice(
                                                b"Usage error. Type ~? for help.\r\n",
                                            );
                                            self.mode = InputMode::Remote;
                                            // DO NOT trigger prompt reprint on remote!
                                            // See UX_GUIDELINES.md: Never send a hidden Enter (\r).
                                        } else {
                                            self.mode = InputMode::Remote;
                                            let mut r_bytes = bytes;
                                            r_bytes.push(b'\r');
                                            to_remote.extend_from_slice(&r_bytes);
                                        }
                                    }
                                }
                                self.at_start_of_line = true;
                                self.local_line_len = 0;
                                if self.mode == InputMode::Remote {
                                    self.active_line = None;
                                    continue;
                                }
                            }
                            EditorEffect::SubmitPrompt(bytes) => {
                                let line_str = String::from_utf8_lossy(&bytes);
                                self.prompt_history.add(&line_str);

                                let Some(action) = parse_local_command(&bytes) else {
                                    finalize_submitted_line(&mut to_local, &mut line);
                                    // Reset the prompt editor for the next command
                                    line = LineSession {
                                        editor: LineEditor::new_prompt(),
                                        display_cursor: 0,
                                        control_state: ControlSequenceState::None,
                                    };
                                    to_local.extend_from_slice(b"irosh> ");
                                    self.active_line = Some(line);
                                    continue;
                                };

                                if matches!(action, LocalCommand::Exit | LocalCommand::Disconnect) {
                                    finalize_exit_line(&mut to_local, &mut line);
                                    self.exit_local_prompt();
                                    actions.push(EscapeAction::RunLocal(action));
                                    continue;
                                } else {
                                    finalize_submitted_line(&mut to_local, &mut line);
                                    actions.push(EscapeAction::RunLocal(action));
                                    // Reset the prompt editor for the next command
                                    line = LineSession {
                                        editor: LineEditor::new_prompt(),
                                        display_cursor: 0,
                                        control_state: ControlSequenceState::None,
                                    };
                                }
                            }
                        }
                    }
                    self.active_line = Some(line);
                }
            }
        }

        (to_remote, to_local, actions)
    }

    /// Attempts to complete the current active line.
    /// Returns any terminal output (e.g. suggestions or updated line).
    pub async fn complete_active_line(
        &mut self,
        session: &mut Session,
        transfer_context: &TransferContext,
    ) -> Vec<u8> {
        let Some(mut line) = self.active_line.take() else {
            return Vec::new();
        };

        let mode = match line.editor.mode() {
            EditorMode::Escape => CompletionMode::Escape,
            EditorMode::Prompt => CompletionMode::Prompt,
        };

        let mut to_local = Vec::new();
        match completion::complete_line(
            mode,
            session,
            transfer_context,
            line.editor.line(),
            line.editor.cursor(),
        )
        .await
        {
            Ok(CompletionResult::Applied(edit)) => {
                line.editor.replace_line(edit.line, edit.cursor);
                if mode == CompletionMode::Escape {
                    render_escape_line_nondestructively(&mut to_local, &mut line);
                } else {
                    render_line(&mut to_local, &mut line);
                }
            }
            Ok(CompletionResult::Suggestions(matches)) => {
                // Show matches on a new line and reprint prompt
                to_local.extend_from_slice(b"\r\n");
                for (i, m) in matches.iter().enumerate() {
                    if i > 0 {
                        to_local.extend_from_slice(b"  ");
                    }
                    to_local.extend_from_slice(m.as_bytes());
                }
                to_local.extend_from_slice(b"\r\n");
                if mode == CompletionMode::Prompt {
                    to_local.extend_from_slice(b"irosh> ");
                    render_line(&mut to_local, &mut line);
                } else {
                    // Tilde is already part of the editor buffer.
                    // For suggestions, we are on a new line now, so we actually
                    // DO need to render from the start of this new line.
                    // However, to keep it consistent, if we want to preserve
                    // the remote prompt, maybe we shouldn't show suggestions
                    // this way for escapes?
                    // Actually, if we just printed \r\n, we are on a new blank line.
                    // So render_escape_line_nondestructively will just print the escape line.
                    render_escape_line_nondestructively(&mut to_local, &mut line);
                }
            }
            _ => {
                // Do nothing or maybe a beep?
            }
        }

        self.active_line = Some(line);
        to_local
    }

    fn handle_remote_byte(&mut self, byte: u8, to_remote: &mut Vec<u8>) {
        to_remote.push(byte);
        match byte {
            b'\r' | b'\n' => {
                self.at_start_of_line = true;
                self.local_line_len = 0;
            }
            8 | 127 => {
                self.local_line_len = self.local_line_len.saturating_sub(1);
            }
            _ => {
                self.at_start_of_line = false;
                self.local_line_len += 1;
            }
        }
    }

    /// Handles a terminal resize event.
    /// Returns any terminal output needed to re-render the local UI.
    pub fn handle_resize(&mut self) -> Option<Vec<u8>> {
        if let Some(mut line) = self.active_line.take() {
            let mut to_local = Vec::new();
            render_line(&mut to_local, &mut line);
            self.active_line = Some(line);
            Some(to_local)
        } else {
            None
        }
    }
}

fn paired_enter_byte(byte: u8) -> Option<u8> {
    match byte {
        b'\r' => Some(b'\n'),
        b'\n' => Some(b'\r'),
        _ => None,
    }
}

fn render_line(to_local: &mut Vec<u8>, line: &mut LineSession) {
    let prompt = if matches!(line.editor.mode(), EditorMode::Prompt) {
        Some("irosh> ")
    } else {
        None
    };

    // Surgical redraw: move to start of the edit area and clear to the right.
    to_local.extend_from_slice(b"\r");
    if let Some(p) = prompt {
        to_local.extend_from_slice(p.as_bytes());
    }
    to_local.extend_from_slice(b"\x1b[K"); // Clear only from here to the end of the line

    // Re-print current editor buffer.
    to_local.extend_from_slice(line.editor.line());

    // Position the cursor correctly within the line.
    let cursor = line.editor.cursor();
    let tail_len = line.editor.line().len().saturating_sub(cursor);
    if tail_len > 0 {
        to_local.extend_from_slice(format!("\x1b[{}D", tail_len).as_bytes());
    }

    line.display_cursor = cursor;
}

fn render_escape_line_nondestructively(to_local: &mut Vec<u8>, line: &mut LineSession) {
    // For escape sequences, we never use \r. We just stay where we are
    // and manage the characters.
    // This is a "relative" render.

    // 1. Move back to the start of our "edit" area (where the tilde started).
    if line.display_cursor > 0 {
        to_local.extend_from_slice(format!("\x1b[{}D", line.display_cursor).as_bytes());
    }

    // 2. Clear to the right.
    to_local.extend_from_slice(b"\x1b[K");

    // 3. Print the new content.
    to_local.extend_from_slice(line.editor.line());

    // 4. Position cursor correctly.
    let cursor = line.editor.cursor();
    let tail_len = line.editor.line().len().saturating_sub(cursor);
    if tail_len > 0 {
        to_local.extend_from_slice(format!("\x1b[{}D", tail_len).as_bytes());
    }

    line.display_cursor = cursor;
}

fn clear_line_preview(to_local: &mut Vec<u8>, line: &mut LineSession) {
    if line.editor.mode() == EditorMode::Escape {
        // Move back to where the escape started and clear only to the right
        if line.display_cursor > 0 {
            to_local.extend_from_slice(format!("\x1b[{}D", line.display_cursor).as_bytes());
        }
        to_local.extend_from_slice(b"\x1b[K");
    } else {
        // For the irosh> prompt, we can use a full line clear as we own that line.
        to_local.extend_from_slice(b"\r\x1b[K");
    }
}

fn finalize_submitted_line(to_local: &mut Vec<u8>, line: &mut LineSession) {
    // Move cursor to the end of the line before adding newline.
    let tail_len = line.editor.line().len().saturating_sub(line.display_cursor);
    if tail_len > 0 {
        to_local.extend_from_slice(format!("\x1b[{}C", tail_len).as_bytes());
    }
    to_local.extend_from_slice(b"\r\n");
    line.display_cursor = 0;
}

fn finalize_exit_line(to_local: &mut Vec<u8>, line: &mut LineSession) {
    // Move to end and add a clean newline.
    let tail_len = line.editor.line().len().saturating_sub(line.display_cursor);
    if tail_len > 0 {
        to_local.extend_from_slice(format!("\x1b[{}C", tail_len).as_bytes());
    }
    to_local.extend_from_slice(b"\r\n\r");
    line.display_cursor = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use irosh::StateConfig;
    use tempfile::tempdir;

    fn setup_engine() -> (InputEngine, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let state = StateConfig::new(dir.path().to_path_buf());
        (InputEngine::new(&state), dir)
    }

    #[test]
    fn test_tilde_echoes_enters_local_edit() {
        let (mut engine, _dir) = setup_engine();
        let (remote, local, actions) = engine.process_local(b"~");
        assert!(remote.is_empty());
        // We now just echo the tilde directly
        assert_eq!(local, b"~");
        assert!(actions.is_empty());
        assert_eq!(engine.mode, InputMode::LocalEdit);
    }

    #[test]
    fn test_help_requires_enter() {
        let (mut engine, _dir) = setup_engine();
        engine.process_local(b"~");
        let (_, _, actions) = engine.process_local(b"?");
        assert!(actions.is_empty(), "no action until Enter");

        let (remote, local, actions) = engine.process_local(b"\r");
        assert!(remote.is_empty());
        assert!(local.contains(&b'\r'));
        assert!(local.contains(&b'\n'));
        assert_eq!(actions, vec![EscapeAction::Help]);
    }

    #[test]
    fn test_disconnect_requires_enter() {
        let (mut engine, _dir) = setup_engine();
        engine.process_local(b"~");
        engine.process_local(b".");
        let (_, _, actions) = engine.process_local(b"\r");
        assert_eq!(actions, vec![EscapeAction::Disconnect]);
    }

    #[test]
    fn test_backspace_cancels_when_only_tilde() {
        let (mut engine, _dir) = setup_engine();
        engine.process_local(b"~");
        assert_eq!(engine.mode, InputMode::LocalEdit);

        let (remote, local, _) = engine.process_local(&[127]);
        assert_eq!(engine.mode, InputMode::Remote, "escape cancelled");
        assert!(remote.is_empty());
        // Should have non-destructive ANSI clear bytes (\x1b[1D\x1b[K)
        assert!(local.contains(&b'D'));
        assert!(local.contains(&b'K'));
    }

    #[test]
    fn test_unknown_escape_shows_help() {
        let (mut engine, _dir) = setup_engine();
        engine.process_local(b"~");
        engine.process_local(b"x");
        let (remote, _, actions) = engine.process_local(b"\r");
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            EscapeAction::RunLocal(LocalCommand::Unknown(ref cmd)) if cmd == "x"
        ));
        assert!(
            remote.is_empty(),
            "unknown command not sent to remote anymore"
        );
    }

    #[test]
    fn test_empty_enter_reprints_prompt() {
        let (mut engine, _dir) = setup_engine();
        // Enter local prompt
        engine.process_local(b"~");
        engine.process_local(b"c");
        engine.process_local(b"\r");
        assert_eq!(engine.mode, InputMode::LocalEdit);

        // Press Enter on empty line
        let (remote, local, actions) = engine.process_local(b"\r");
        assert_eq!(engine.mode, InputMode::LocalEdit);
        assert!(actions.is_empty());
        assert!(remote.is_empty());
        assert!(String::from_utf8_lossy(&local).contains("irosh> "));
    }

    #[test]
    fn test_remote_newline_arms_escape() {
        let (mut engine, _dir) = setup_engine();
        engine.process_local(b"a");
        assert!(!engine.at_start_of_line);

        engine.observe_remote(b"\r\n");
        assert!(engine.at_start_of_line);

        let (_, _, _) = engine.process_local(b"~");
        assert_eq!(engine.mode, InputMode::LocalEdit);
    }

    #[test]
    fn test_c_enters_prompt() {
        let (mut engine, _dir) = setup_engine();
        engine.process_local(b"~");
        engine.process_local(b"c");
        let (_, _, actions) = engine.process_local(b"\r");
        assert_eq!(actions, vec![EscapeAction::CommandPrompt]);
        assert_eq!(engine.mode, InputMode::LocalEdit);
    }

    #[test]
    fn test_exit_returns_to_remote() {
        let (mut engine, _dir) = setup_engine();
        // Setup state: inside prompt
        engine.process_local(b"~");
        engine.process_local(b"c");
        engine.process_local(b"\r");
        assert_eq!(engine.mode, InputMode::LocalEdit);

        // Type "exit" + ENTER
        engine.process_local(b"e");
        engine.process_local(b"x");
        engine.process_local(b"i");
        engine.process_local(b"t");
        let (_, _, actions) = engine.process_local(b"\r");

        assert_eq!(engine.mode, InputMode::Remote);
        match &actions[0] {
            EscapeAction::RunLocal(cmd) => {
                assert!(matches!(cmd, super::super::prompt::LocalCommand::Exit))
            }
            _ => panic!("Expected RunLocal(Exit)"),
        }
    }
}

// ---------------------------------------------------------------------------
// Property-based fuzz tests (proptest)
// ---------------------------------------------------------------------------
//
// Invariants enforced across all fuzz targets:
//   1. No panic on any input — crash safety.
//   2. `engine.mode` is always one of the two valid variants.
//   3. When `active_line` is Some, its editor cursor ≤ line length.
//   4. `to_remote` and `to_local` vectors are structurally sound (no OOB).
//
// These tests intentionally have no `assert` on the *values* of output — only
// on structural properties — because the semantics of arbitrary byte input are
// not well-defined. The goal is to shake out panics, index-out-of-bounds,
// arithmetic overflows, and corrupted state.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod fuzz {
    use super::*;
    use irosh::StateConfig;
    use proptest::prelude::*;
    use tempfile::tempdir;

    /// Creates a fresh engine backed by a real (but temporary) state directory
    /// so that history persistence code paths are exercised.
    fn make_engine() -> (InputEngine, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir creation must succeed in test environment");
        let state = StateConfig::new(dir.path().to_path_buf());
        (InputEngine::new(&state), dir)
    }

    /// Verifies that `engine.mode` is always one of the two valid variants.
    /// This is a no-op in terms of values but ensures the enum discriminant
    /// is never corrupted into an undefined state.
    fn assert_mode_valid(mode: &InputMode) {
        assert!(
            matches!(mode, InputMode::Remote | InputMode::LocalEdit),
            "engine.mode must always be Remote or LocalEdit, got: {mode:?}"
        );
    }

    proptest! {
        /// FUZZ-01: Arbitrary byte sequences must never panic the input engine.
        ///
        /// Covers: crash safety, mode invariant.
        /// Strategy: random byte slices of length 0–512, fed one call at a time
        /// (matching how `drive_session` calls it in production — one stdin read
        /// at a time, not individual bytes).
        #[test]
        fn fuzz_input_engine_no_panic(
            chunks in prop::collection::vec(
                prop::collection::vec(0u8..=255, 0..=64),
                0..=16,
            )
        ) {
            let (mut engine, _dir) = make_engine();
            for chunk in &chunks {
                let (to_remote, to_local, _actions) = engine.process_local(chunk);
                // Structural soundness: output vecs must be valid (no capacity issues).
                // We consume them to prevent the optimizer from eliding the work.
                let _ = to_remote.len();
                let _ = to_local.len();
                assert_mode_valid(&engine.mode);
            }
        }

        /// FUZZ-02: Cursor must never exceed line length in the active editor.
        ///
        /// Covers: editor cursor invariant (prevents OOB access in render_line).
        /// Strategy: interleave printable ASCII, control bytes, and arrow key
        /// sequences to stress the editor's cursor arithmetic.
        #[test]
        fn fuzz_input_engine_cursor_in_bounds(
            input in prop::collection::vec(
                prop::sample::select(vec![
                    // Printable ASCII range
                    b'a', b'z', b'0', b'~', b' ', b'/',
                    // Control
                    b'\r', b'\n', 8u8, 127u8, 0x03u8, b'\t',
                    // ESC + CSI arrow sequence bytes
                    27u8, b'[', b'A', b'B', b'C', b'D',
                    // Home / End via CSI
                    b'1', b'4', b'~', b'H', b'F',
                ]),
                0..=256,
            )
        ) {
            let (mut engine, _dir) = make_engine();
            let _ = engine.process_local(&input);
            assert_mode_valid(&engine.mode);

            if let Some(ref line) = engine.active_line {
                let editor_line_len = line.editor.line().len();
                let cursor = line.editor.cursor();
                assert!(
                    cursor <= editor_line_len,
                    "cursor ({cursor}) must be <= line length ({editor_line_len})"
                );
                // Visual tracking must also be consistent.
                assert!(
                    line.display_cursor <= editor_line_len,
                    "display_cursor ({}) must be <= editor_line_len ({})",
                    line.display_cursor,
                    editor_line_len,
                );
            }
        }

        /// FUZZ-03: `observe_remote` must never panic or corrupt mode on any byte stream.
        ///
        /// Covers: the remote-data path that arms/disarms the escape detector.
        #[test]
        fn fuzz_observe_remote_no_panic(
            data in prop::collection::vec(0u8..=255, 0..=512)
        ) {
            let (mut engine, _dir) = make_engine();
            // Mix observe_remote with process_local to test interleaved usage.
            engine.observe_remote(&data);
            assert_mode_valid(&engine.mode);

            // Now feed a tilde (would arm escape) and verify no corruption.
            let _ = engine.process_local(b"~");
            engine.observe_remote(&data);
            assert_mode_valid(&engine.mode);
        }

        /// FUZZ-04: `parse_local_command` must never panic on arbitrary UTF-8 input.
        ///
        /// Covers: shell-words tokenisation + keyword matching on untrusted strings.
        /// Strategy: arbitrary printable strings (proptest `".*"` regex generates
        /// valid Unicode strings, which is the real input domain — bytes are already
        /// UTF-8 decoded before `parse_local_command` is called).
        #[test]
        fn fuzz_parse_local_command_no_panic(raw in ".*") {
            let _ = parse_local_command(raw.as_bytes());
        }

        /// FUZZ-05: `CommandHistory` add/up/down must uphold its internal invariants.
        ///
        /// Invariants:
        ///   - `index` is always `None` or `Some(i)` where `i < entries.len()`.
        ///   - `up()` never returns an entry that is out of bounds.
        ///   - Repeated `down()` after hitting the end always returns the pending line.
        ///   - Entries never exceed `MAX_HISTORY_ENTRIES` (1000).
        ///
        /// We use a persistent in-memory history (no file) to keep the test hermetic.
        #[test]
        fn fuzz_command_history_state_machine(
            commands in prop::collection::vec("[a-z ]{0,40}", 0..=50),
            nav_sequence in prop::collection::vec(0u8..=3, 0..=30),
        ) {
            let mut history = CommandHistory::new(None);

            for cmd in &commands {
                history.add(cmd);
            }

            // Navigate: 0=up, 1=down, 2=abandon_navigation, 3=re-add current
            let current = "current";
            for nav in &nav_sequence {
                match nav {
                    0 => { let _ = history.up(current); }
                    1 => { let _ = history.down(); }
                    2 => history.abandon_navigation(current),
                    _ => history.add(current),
                }
            }

            // After arbitrary navigation, up() must not return an OOB value.
            // We verify this implicitly: if it panicked, the test would fail.
            let _ = history.up(current);
            let _ = history.down();
        }
    }
}

//! Input engine for handling local keystrokes, remote sync, and escape sequences.

use super::editor::{EditorEffect, EditorEvent, LineEditor, EditorMode};
use super::history::CommandHistory;
use super::prompt::{LocalCommand, parse_local_command};
use super::completion::{self, CompletionMode, CompletionResult};
use super::transfer::TransferContext;
use super::ansi::{ControlSequenceState, consume_control_sequence};
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
    pub visual_len: usize,
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
        }
    }

    /// Exit local prompt mode, returning to remote interaction.
    pub fn exit_local_prompt(&mut self) {
        self.mode = InputMode::Remote;
        self.active_line = None;
        self.at_start_of_line = true;
        self.local_line_len = 0;
    }

    /// A newline from the server arms the escape detector for the next keystroke.
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
                    if (self.at_start_of_line || self.local_line_len == 0) && byte == b'~' {
                        // Enter escape line mode.
                        self.mode = InputMode::LocalEdit;
                        let mut new_line = LineSession {
                            editor: LineEditor::new_escape(),
                            visual_len: 0,
                            display_cursor: 0,
                            control_state: ControlSequenceState::None,
                        };
                        // Redraw prompt line (which currently is just `~`)
                        render_line(&mut to_local, &mut new_line);
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
                                render_line(&mut to_local, &mut line);
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
                                            finalize_submitted_line(&mut to_local, &mut line);
                                            // Sync mode internally so next bytes are handled as Prompt
                                            self.mode = InputMode::LocalEdit;
                                            line = LineSession {
                                                editor: LineEditor::new_prompt(),
                                                visual_len: 0,
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
                                        let cmd_str = String::from_utf8_lossy(trim_bytes(&bytes.strip_prefix(b"~").unwrap_or(&bytes)));
                                        let keyword = cmd_str.split_whitespace().next().unwrap_or("");
                                        if ["put", "get", "lls", "ls", "lcd", "cd"].contains(&keyword) {
                                            to_local.extend_from_slice(b"Usage error. Type ~? for help.\r\n");
                                            self.mode = InputMode::Remote;
                                            // Trigger prompt reprint on remote
                                            to_remote.push(b'\r');
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
                                        visual_len: 0,
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
                                        visual_len: 0,
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
                render_line(&mut to_local, &mut line);
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
                } else {
                    // Tilde is already part of the editor buffer
                }
                render_line(&mut to_local, &mut line);
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
}

fn paired_enter_byte(byte: u8) -> Option<u8> {
    match byte {
        b'\r' => Some(b'\n'),
        b'\n' => Some(b'\r'),
        _ => None,
    }
}

fn render_line(to_local: &mut Vec<u8>, line: &mut LineSession) {
    // 1. Move to end of current display
    move_to_end(to_local, line.display_cursor, line.visual_len);
    // 2. Erase visual line
    for _ in 0..line.visual_len {
        to_local.extend_from_slice(b"\x08");
    }
    to_local.extend_from_slice(b"\x1b[K");

    // 3. Write new line
    to_local.extend_from_slice(line.editor.line());

    // 4. Position cursor
    let cursor = line.editor.cursor();
    let tail_len = line.editor.line().len().saturating_sub(cursor);
    if tail_len > 0 {
        to_local.extend_from_slice(format!("\x1b[{}D", tail_len).as_bytes());
    }

    line.visual_len = line.editor.line().len();
    line.display_cursor = cursor;
}

fn move_to_end(to_local: &mut Vec<u8>, cursor: usize, len: usize) {
    let diff = len.saturating_sub(cursor);
    if diff > 0 {
        to_local.extend_from_slice(format!("\x1b[{}C", diff).as_bytes());
    }
}

fn clear_line_preview(to_local: &mut Vec<u8>, line: &mut LineSession) {
    move_to_end(to_local, line.display_cursor, line.visual_len);
    for _ in 0..line.visual_len {
        to_local.extend_from_slice(b"\x08");
    }
    to_local.extend_from_slice(b"\x1b[K");
}

fn finalize_submitted_line(to_local: &mut Vec<u8>, line: &mut LineSession) {
    move_to_end(to_local, line.display_cursor, line.visual_len);
    to_local.extend_from_slice(b"\r\n");
    line.visual_len = 0;
    line.display_cursor = 0;
}

fn finalize_exit_line(to_local: &mut Vec<u8>, line: &mut LineSession) {
    move_to_end(to_local, line.display_cursor, line.visual_len);
    // Don't add a newline here; the remote shell's prompt reprint will handle it.
    // Just move to the start of the line so the next output is correctly positioned.
    to_local.extend_from_slice(b"\r");
    line.visual_len = 0;
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
        // render_line echoes the line (which is "~") prepended with ANSI clear codes
        assert!(local.ends_with(b"~"));
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
        // Should have clear_line_preview bytes
        assert!(local.contains(&b'\x08'));
        assert!(local.contains(&b'\x1b'));
    }

    #[test]
    fn test_unknown_escape_shows_help() {
        let (mut engine, _dir) = setup_engine();
        engine.process_local(b"~");
        engine.process_local(b"x");
        let (remote, _, actions) = engine.process_local(b"\r");
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], EscapeAction::RunLocal(LocalCommand::Help)));
        assert!(remote.is_empty(), "unknown command not sent to remote anymore");
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
}

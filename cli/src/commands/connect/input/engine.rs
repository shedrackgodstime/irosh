//! The input state machine: local keystrokes, remote line tracking, and
//! escape-sequence rendering.

use super::actions::{
    ControlSequenceState, EscapeAction, InputMode, RemoteAnsiState, paired_enter_byte, parse_escape,
};
use super::editor::{EditorEffect, EditorEvent, EditorMode, LineEditor};
use crate::commands::connect::completion::{self, CompletionMode, CompletionResult};
use crate::commands::connect::history::CommandHistory;
use crate::commands::connect::prompt::{LocalCommand, parse_local_command};
use crate::commands::connect::transfer::TransferContext;
use irosh::Session;

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
    /// State machine for tracking ANSI sequences arriving from the remote.
    /// Prevents escape codes in remote prompts from corrupting `local_line_len`.
    remote_ansi: RemoteAnsiState,
    /// True if the remote peer is running Windows.
    /// Used to conditionally apply ConPTY mitigations like `\x0C` screen clears.
    pub remote_is_windows: bool,
}

impl InputEngine {
    pub fn new(state: &irosh::StateConfig, remote_is_windows: bool) -> Self {
        let history_dir = state.root().join("history");
        Self {
            mode: InputMode::Remote,
            at_start_of_line: true,
            local_line_len: 0,
            active_line: None,
            escape_history: CommandHistory::new(Some(history_dir.join("escape.history"))),
            prompt_history: CommandHistory::new(Some(history_dir.join("prompt.history"))),
            swallow_next_enter_pair: None,
            remote_ansi: RemoteAnsiState::None,
            remote_is_windows,
        }
    }

    /// Remote data arriving resets the 'start of line' state on newlines.
    /// Only `\n` arms the escape detector: Windows shells redraw the prompt
    /// with bare `\r`s (PSReadLine) and progress bars reuse `\r` to repaint.
    /// Treating those as line starts would re-arm mid-command and hijack a
    /// literal `~` typed later on the same line. Real newlines always carry
    /// `\n` (`\r\n` on Windows, `\n` on Unix), so nothing is lost.
    pub fn observe_remote(&mut self, data: &[u8]) {
        for &byte in data {
            if byte == b'\n' {
                self.at_start_of_line = true;
                self.local_line_len = 0;
            }
        }
    }

    /// Process local input.
    pub fn process_local(&mut self, data: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<EscapeAction>) {
        let mut to_remote = Vec::with_capacity(data.len());
        let mut to_local = Vec::new();
        let mut actions = Vec::new();

        if self.mode == InputMode::Remote {
            for (i, &byte) in data.iter().enumerate() {
                // Swallow \r\n or \n\r pairs from local terminal.
                if let Some(pair) = self.swallow_next_enter_pair {
                    if byte == pair {
                        self.swallow_next_enter_pair = None;
                        continue;
                    }
                    self.swallow_next_enter_pair = None;
                }

                if self.local_line_len == 0 && byte == b'~' {
                    self.mode = InputMode::LocalEdit;
                    let new_line = LineSession {
                        editor: LineEditor::new_escape(),
                        display_cursor: 0,
                        control_state: ControlSequenceState::None,
                    };
                    to_local.push(b'~');
                    self.active_line = Some(new_line);
                    // Process any remaining bytes in this buffer in LocalEdit
                    // mode instead of leaking them to remote (arrow keys, etc.).
                    if i + 1 < data.len() {
                        let remaining = &data[i + 1..];
                        let (r, l, a) = self.process_local(remaining);
                        to_remote.extend(r);
                        to_local.extend(l);
                        actions.extend(a);
                    }
                    break;
                }
                self.handle_remote_byte(byte, &mut to_remote);
            }
        } else {
            // In LocalEdit mode, we parse bytes into EditorEvents using a stateless ANSI machine.
            for &byte in data {
                let Some(mut line) = self.active_line.take() else {
                    self.mode = InputMode::Remote;
                    break;
                };

                let event = if line.control_state != ControlSequenceState::None {
                    Self::consume_local_ansi(&mut line.control_state, byte)
                } else if byte == 27 {
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
                    to_local.extend_from_slice(b"^C\r\n");
                    self.exit_local_prompt(&mut to_remote);
                    break;
                } else if byte.is_ascii_graphic() || byte == b' ' {
                    Some(EditorEvent::InsertByte(byte))
                } else {
                    None
                };

                if let Some(ev) = event {
                    self.apply_editor_event(
                        ev,
                        &mut line,
                        &mut to_local,
                        &mut to_remote,
                        &mut actions,
                    );
                }

                if self.mode == InputMode::LocalEdit {
                    self.active_line = Some(line);
                }
            }
        }

        (to_remote, to_local, actions)
    }

    fn apply_editor_event(
        &mut self,
        event: EditorEvent,
        line: &mut LineSession,
        to_local: &mut Vec<u8>,
        to_remote: &mut Vec<u8>,
        actions: &mut Vec<EscapeAction>,
    ) {
        let is_escape = matches!(line.editor.mode(), EditorMode::Escape);
        let history = if is_escape {
            &mut self.escape_history
        } else {
            &mut self.prompt_history
        };

        match line.editor.apply(event, history) {
            EditorEffect::NoOp => {}
            EditorEffect::Render => {
                if is_escape {
                    echo_escape_line(to_local, line);
                } else {
                    render_line(to_local, line);
                }
            }
            EditorEffect::ClearAndExit => {
                let chars_on_screen = line.editor.line().len();
                clear_line_preview(to_local, line, chars_on_screen);
                self.exit_local_prompt(to_remote);
            }
            EditorEffect::RequestCompletion => {
                actions.push(EscapeAction::RequestCompletion);
            }
            EditorEffect::SubmitEscape(bytes) => {
                let line_str = String::from_utf8_lossy(&bytes);
                self.escape_history.add(&line_str);

                if let Some(action) = parse_escape(&bytes) {
                    match action {
                        EscapeAction::CommandPrompt => {
                            finalize_submitted_line(to_local, line);
                            self.mode = InputMode::LocalEdit;
                            line.editor = LineEditor::new_prompt();
                            line.display_cursor = 0;
                            line.control_state = ControlSequenceState::None;
                            self.at_start_of_line = true;
                            self.local_line_len = 0;
                            to_local.extend_from_slice(b"irosh> ");
                            actions.push(EscapeAction::CommandPrompt);
                        }
                        EscapeAction::SendLiteral(payload) => {
                            finalize_submitted_line(to_local, line);
                            // Back to remote input without waking the shell: the
                            // literal byte joins the pending remote line and no
                            // Enter is sent. One char is pending remotely, so a
                            // following `~` must not re-arm the escape detector.
                            self.mode = InputMode::Remote;
                            self.at_start_of_line = false;
                            self.local_line_len = 1;
                            self.active_line = None;
                            actions.push(EscapeAction::SendLiteral(payload));
                        }
                        action => {
                            finalize_submitted_line(to_local, line);
                            self.exit_local_prompt(to_remote);
                            actions.push(action);
                        }
                    }
                } else {
                    finalize_submitted_line(to_local, line);
                    self.exit_local_prompt(to_remote);
                    let mut r_bytes = Vec::with_capacity(bytes.len() + 2);
                    r_bytes.push(b'~');
                    r_bytes.extend_from_slice(&bytes);
                    r_bytes.push(b'\r');
                    to_remote.extend_from_slice(&r_bytes);
                }
            }
            EditorEffect::SubmitPrompt(bytes) => {
                let line_str = String::from_utf8_lossy(&bytes);
                self.prompt_history.add(&line_str);

                let Some(action) = parse_local_command(&bytes) else {
                    finalize_submitted_line(to_local, line);
                    line.editor = LineEditor::new_prompt();
                    line.display_cursor = 0;
                    line.control_state = ControlSequenceState::None;
                    to_local.extend_from_slice(b"\r\nirosh> ");
                    return;
                };

                if matches!(action, LocalCommand::Exit | LocalCommand::Disconnect) {
                    finalize_exit_line(to_local, line);
                    self.exit_local_prompt(to_remote);
                    actions.push(EscapeAction::RunLocal(action));
                } else {
                    finalize_submitted_line(to_local, line);
                    actions.push(EscapeAction::RunLocal(action));
                    line.editor = LineEditor::new_prompt();
                    line.display_cursor = 0;
                    line.control_state = ControlSequenceState::None;
                }
            }
        }
    }

    fn consume_local_ansi(state: &mut ControlSequenceState, byte: u8) -> Option<EditorEvent> {
        match state {
            ControlSequenceState::Escape => {
                if byte == b'[' {
                    *state = ControlSequenceState::Csi;
                    None
                } else {
                    *state = ControlSequenceState::None;
                    None
                }
            }
            ControlSequenceState::Csi => {
                // Parameter bytes (0x20-0x3F: digits, `;`, `<`, `=`, `>`, `?`)
                // stay in CSI state. Only final bytes (0x40-0x7E) produce events.
                if (0x20..=0x3F).contains(&byte) {
                    return None; // stay in CSI, consume parameter
                }
                *state = ControlSequenceState::None;
                match byte {
                    b'A' => Some(EditorEvent::HistoryUp),
                    b'B' => Some(EditorEvent::HistoryDown),
                    b'C' => Some(EditorEvent::MoveRight),
                    b'D' => Some(EditorEvent::MoveLeft),
                    b'H' => Some(EditorEvent::MoveHome),
                    b'F' => Some(EditorEvent::MoveEnd),
                    b'~' => Some(EditorEvent::Delete), // \x1b[3~ etc.
                    _ => None,
                }
            }
            ControlSequenceState::None => None,
        }
    }

    fn exit_local_prompt(&mut self, to_remote: &mut Vec<u8>) {
        self.mode = InputMode::Remote;
        self.at_start_of_line = true;
        self.local_line_len = 0;
        self.active_line = None;
        // "Wake Up" sequence: prompt the remote shell to reprint its prompt.
        to_remote.push(b'\r');
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
                    // For escape mode, tab completion replaces the line in place.
                    // Use backspace-over then re-echo to stay portable.
                    echo_escape_line(&mut to_local, &line);
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
                    // On a fresh new line, just echo the current escape buffer.
                    echo_escape_line(&mut to_local, &line);
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

        // Drive the remote ANSI state machine. Bytes that are part of an escape
        // sequence must not count toward `local_line_len` or reset `at_start_of_line`,
        // otherwise the `~` escape arm will misfire on remote shells with colored prompts.
        match self.remote_ansi {
            RemoteAnsiState::None => match byte {
                b'\r' | b'\n' => {
                    self.at_start_of_line = true;
                    self.local_line_len = 0;
                }
                0x1b => {
                    self.remote_ansi = RemoteAnsiState::AfterEsc;
                }
                8 | 127 => {
                    self.local_line_len = self.local_line_len.saturating_sub(1);
                }
                _ => {
                    self.at_start_of_line = false;
                    self.local_line_len += 1;
                }
            },
            RemoteAnsiState::AfterEsc => {
                self.remote_ansi = match byte {
                    b'[' => RemoteAnsiState::InCsi,
                    b']' => RemoteAnsiState::InOsc,
                    // Single-char escape (ESC M, ESC =, ESC >, ...): one introducer byte, done.
                    _ => RemoteAnsiState::None,
                };
            }
            RemoteAnsiState::InCsi => {
                // Parameter bytes: 0x30-0x3F  (digits, `;`, `:`, `<`, `=`, `>`, `?`)
                // Intermediate bytes: 0x20-0x2F (space, `!`, `"`, ...)
                // Together they occupy 0x20-0x3F. Any byte outside this range
                // (i.e. 0x40-0x7E final byte, or a stray control char) ends the sequence.
                if !(0x20..=0x3F).contains(&byte) {
                    self.remote_ansi = RemoteAnsiState::None;
                }
            }
            RemoteAnsiState::InOsc => match byte {
                0x07 => self.remote_ansi = RemoteAnsiState::None, // BEL terminates OSC
                0x1b => self.remote_ansi = RemoteAnsiState::InOscEsc, // possible ST start
                _ => {}                                           // keep consuming OSC payload
            },
            RemoteAnsiState::InOscEsc => {
                // Any byte after ESC inside an OSC closes the String Terminator.
                self.remote_ansi = RemoteAnsiState::None;
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

fn render_line(to_local: &mut Vec<u8>, line: &mut LineSession) {
    let prompt = if matches!(line.editor.mode(), EditorMode::Prompt) {
        Some("irosh> ")
    } else {
        None
    };

    // Surgical redraw: use \r (carriage return) to move to column 0.
    // We deliberately use \r instead of \x1b[G (CHA) here because Windows
    // ConPTY can misinterpret CHA when the viewport has scrolled, causing the
    // cursor to land on the wrong row in the scrollback buffer. \r is a raw
    // terminal primitive that is always handled correctly.
    to_local.push(b'\r');
    if let Some(p) = prompt {
        to_local.extend_from_slice(p.as_bytes());
    }
    to_local.extend_from_slice(b"\x1b[K"); // Clear from cursor to end of line

    // Re-print current editor buffer.
    to_local.extend_from_slice(line.editor.line());

    // Position the cursor correctly within the line.
    let cursor = line.editor.cursor();
    let tail_len = line.editor.line().len().saturating_sub(cursor);
    if tail_len > 0 {
        to_local.extend_from_slice(format!("\x1b[{tail_len}D").as_bytes());
    }

    line.display_cursor = cursor;
}

/// Forward-only echo for escape mode.
///
/// We never move the cursor backwards when in escape mode because we do not
/// know the absolute cursor column (the remote shell's prompt may be any
/// length). Instead we simply echo the last character that was added.
/// Backspacing is handled separately via `clear_line_preview`.
/// This is the OpenSSH `~C` pattern and works on every terminal, including
/// Windows ConPTY.
fn echo_escape_line(to_local: &mut Vec<u8>, line: &LineSession) {
    // The editor buffer always starts with '~'. Only echo the part after
    // whatever was already on-screen. Since we forward-echo every character
    // as it is typed, we just need to print the very last byte added.
    if let Some(&last_byte) = line.editor.line().last() {
        to_local.push(last_byte);
    }
}

fn clear_line_preview(to_local: &mut Vec<u8>, line: &mut LineSession, chars_on_screen: usize) {
    if line.editor.mode() == EditorMode::Escape {
        // Erase only the characters we typed in escape mode using
        // backspace-space-backspace sequences. This is purely additive and
        // never requires knowing the absolute cursor column.
        // Use chars_on_screen (captured before editor cleared its buffer) so
        // the loop count is always correct even when the buffer is already empty.
        let erase_count = chars_on_screen.max(1);
        for _ in 0..erase_count {
            to_local.extend_from_slice(b"\x08 \x08");
        }
    } else {
        // For the irosh> prompt we own the entire line, so \r is safe.
        to_local.extend_from_slice(b"\r\x1b[K");
    }
}

fn finalize_submitted_line(to_local: &mut Vec<u8>, line: &mut LineSession) {
    // Move cursor to the end of the line before adding newline.
    let tail_len = line.editor.line().len().saturating_sub(line.display_cursor);
    if tail_len > 0 {
        to_local.extend_from_slice(format!("\x1b[{tail_len}C").as_bytes());
    }
    to_local.extend_from_slice(b"\r\n");
    line.display_cursor = 0;
}

fn finalize_exit_line(to_local: &mut Vec<u8>, line: &mut LineSession) {
    // Move to end and add a clean newline. Do NOT send a bare \r after \r\n
    // because on Windows ConPTY that extra \r can be interpreted as an Enter
    // keypress being forwarded to the remote shell, causing spurious output.
    let tail_len = line.editor.line().len().saturating_sub(line.display_cursor);
    if tail_len > 0 {
        to_local.extend_from_slice(format!("\x1b[{tail_len}C").as_bytes());
    }
    to_local.extend_from_slice(b"\r\n");
    line.display_cursor = 0;
}

#[cfg(test)]
mod engine_tests;

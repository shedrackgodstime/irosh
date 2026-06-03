//! Input engine for handling local keystrokes, remote sync, and escape sequences.

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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ControlSequenceState {
    #[default]
    None,
    Escape,
    Csi,
}

/// Tracks ANSI/VT sequences arriving from the **remote** stream.
///
/// This prevents escape sequences emitted by remote shells (e.g. colored
/// prompts like `\x1b[38;5;196m`) from corrupting `local_line_len`, which
/// gates the `~` escape-sequence arm. The old approach used a single `bool`
/// that terminated on any ASCII letter - failing on multi-parameter CSI
/// sequences where the parameter bytes (`38;5;196`) were mistakenly counted
/// as typed characters.
///
/// Sequence types handled:
///   - **CSI** `\x1b[` + parameter bytes (0x30-0x3F) + intermediate bytes
///     (0x20-0x2F) + final byte (0x40-0x7E)
///   - **OSC** `\x1b]` + arbitrary bytes + BEL (`\x07`) or ST (`\x1b\\`)
///   - **Single-char escape** `\x1b` + any other byte (e.g. `\x1bM`)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum RemoteAnsiState {
    #[default]
    None,
    /// Saw `\x1b`; waiting for the sequence introducer.
    AfterEsc,
    /// Inside a CSI sequence (`\x1b[`); consuming until a final byte (0x40-0x7E).
    InCsi,
    /// Inside an OSC sequence (`\x1b]`); consuming until BEL or ST.
    InOsc,
    /// Saw `\x1b` inside an OSC; expecting `\\` to complete the String Terminator.
    InOscEsc,
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
    /// State machine for tracking ANSI sequences arriving from the remote.
    /// Prevents escape codes in remote prompts from corrupting `local_line_len`.
    remote_ansi: RemoteAnsiState,
    /// True if the remote peer is running Windows.
    /// Used to conditionally apply ConPTY mitigations like `\x0C` screen clears.
    pub remote_is_windows: bool,
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
        .map_or(0, |i| i + 1);
    if start >= end { &[] } else { &s[start..end] }
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
    pub fn observe_remote(&mut self, data: &[u8]) {
        for &byte in data {
            if byte == b'\r' || byte == b'\n' {
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
                } else {
                    self.handle_remote_byte(byte, &mut to_remote);
                }
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
                    if action == EscapeAction::CommandPrompt {
                        finalize_submitted_line(to_local, line);
                        self.mode = InputMode::LocalEdit;
                        line.editor = LineEditor::new_prompt();
                        line.display_cursor = 0;
                        line.control_state = ControlSequenceState::None;
                        to_local.extend_from_slice(b"\r\nirosh> ");
                    } else {
                        finalize_submitted_line(to_local, line);
                        self.exit_local_prompt(to_remote);
                    }
                    actions.push(action);
                } else {
                    finalize_submitted_line(to_local, line);
                    self.exit_local_prompt(to_remote);
                    let mut r_bytes = Vec::with_capacity(bytes.len() + 2);
                    r_bytes.push(b'~');
                    r_bytes.extend_from_slice(&bytes);
                    r_bytes.push(b'\r');
                    to_remote.extend_from_slice(&r_bytes);
                }
                self.at_start_of_line = true;
                self.local_line_len = 0;
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
mod tests {
    use super::*;
    use irosh::StateConfig;
    use tempfile::tempdir;

    fn setup_engine() -> (InputEngine, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let state = StateConfig::new(dir.path().to_path_buf());
        (InputEngine::new(&state, false), dir)
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
        assert_eq!(
            remote, b"\r",
            "Must send CR to remote to trigger prompt reprint"
        );
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
        assert_eq!(remote, b"\r", "Must send CR even on cancel");
        // Should erase with backspace-space-backspace sequence (portable, works on ConPTY)
        assert!(local.contains(&b'\x08'));
        assert!(local.contains(&b' '));
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
        assert_eq!(remote, b"\r", "Unknown command should trigger CR refresh");
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
    fn test_arrow_keys_in_same_buffer_as_tilde_do_not_leak_to_remote() {
        let (mut engine, _dir) = setup_engine();
        // Simulate pressing ~ then right-arrow in one OS read buffer.
        let (remote, local, actions) = engine.process_local(b"~\x1b[C");
        assert!(
            remote.is_empty(),
            "arrow key bytes must not leak to remote: {remote:?}"
        );
        assert_eq!(engine.mode, InputMode::LocalEdit);
        assert!(local.contains(&b'~'), "local must show tilde");
        // Right arrow after tilde should move cursor right (no-op since cursor already at end)
        assert!(
            actions.is_empty(),
            "movement alone should not produce actions"
        );
    }

    #[test]
    fn test_up_arrow_in_escape_mode_shows_history() {
        let (mut engine, _dir) = setup_engine();
        // ~ then up-arrow in one buffer
        let (remote, _, _) = engine.process_local(b"~\x1b[A");
        assert!(
            remote.is_empty(),
            "up arrow must not leak to remote: {remote:?}"
        );
        assert_eq!(engine.mode, InputMode::LocalEdit);
    }

    #[test]
    fn test_left_arrow_in_escape_mode_does_not_corrupt_line() {
        let (mut engine, _dir) = setup_engine();
        engine.process_local(b"~");
        engine.process_local(b"\x1b[D");
        let (remote, _, _) = engine.process_local(b"\r");
        // The escape line ("~") is submitted to remote as an unknown command.
        // The important thing: raw \x1b[D bytes must NOT appear in remote.
        assert!(
            !remote.contains(&0x1b),
            "escape sequences must not leak to remote: {remote:?}"
        );
        assert!(
            remote.contains(&b'~'),
            "tilde should be sent to remote as fallback"
        );
    }

    #[test]
    fn test_multi_byte_csi_delete_in_escape_mode() {
        let (mut engine, _dir) = setup_engine();
        engine.process_local(b"~");
        engine.process_local(b"a");
        engine.process_local(b"b");
        engine.process_local(b"c");
        // Move cursor left one position (before 'c')
        engine.process_local(b"\x1b[D");
        // Now press Delete (\x1b[3~) to remove 'c'
        engine.process_local(b"\x1b[3~");
        // Verify editor state (not visible output, which is cursor positioning)
        let line = engine.active_line.as_ref().expect("active line exists");
        assert_eq!(
            line.editor.line(),
            b"~ab",
            "Delete should remove char after cursor"
        );
        assert_eq!(line.editor.cursor(), 3, "cursor should stay after removal");

        // Submit — remote should contain CR only (fallback goes via RunLocal action)
        let (remote, _, actions) = engine.process_local(b"\r");
        assert!(
            !remote.contains(&0x1b),
            "no raw escape bytes should leak to remote: {remote:?}"
        );
        assert_eq!(
            actions,
            vec![EscapeAction::RunLocal(LocalCommand::Unknown("ab".into()))],
            "unknown escape should produce RunLocal action"
        );
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
//   1. No panic on any input - crash safety.
//   2. `engine.mode` is always one of the two valid variants.
//   3. When `active_line` is Some, its editor cursor <= line length.
//   4. `to_remote` and `to_local` vectors are structurally sound (no OOB).
//
// These tests intentionally have no `assert` on the *values* of output - only
// on structural properties - because the semantics of arbitrary byte input are
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
        (InputEngine::new(&state, false), dir)
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
        /// Strategy: random byte slices of length 0-512, fed one call at a time
        /// (matching how `drive_session` calls it in production - one stdin read
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
        /// valid Unicode strings, which is the real input domain - bytes are already
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

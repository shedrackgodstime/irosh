mod completion;
mod core;
pub(crate) mod display;
mod escape;
mod prompt;

use anyhow::Result;
use irosh::Session;
use tokio::io::AsyncWriteExt;

use self::completion::{CompletionResult, complete_escape_line, complete_prompt_line};
use self::core::{EditorEffect, EditorEvent, LineEditor};
use self::display::{print_completion_suggestions, print_escape_help, print_local_block};
use self::escape::EscapeCommand;
use self::prompt::{LOCAL_PROMPT, PromptOutcome, run_prompt_command};
use super::support::CommandHistory;
use super::transfer::{TransferContext, run_escape_transfer_command};

#[derive(Debug)]
pub(super) struct InputEngine {
    pending_remote_line: Vec<u8>,
    escape_armed: bool,
    local_input_len: usize,
    swallow_next_enter_pair: Option<u8>,
    escape_history: CommandHistory,
    prompt_history: CommandHistory,
    transfer_context: TransferContext,
    mode: InputMode,
}

#[derive(Debug, Default)]
enum InputMode {
    #[default]
    Remote,
    EscapeLine(LineSession),
    LocalPrompt(LineSession),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NextMode {
    Remote,
    EscapeLine,
    LocalPrompt,
}

#[derive(Debug)]
struct LineSession {
    editor: LineEditor,
    visual_len: usize,
    display_cursor: usize,
    control_state: ControlSequenceState,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum ControlSequenceState {
    #[default]
    None,
    Escape,
    Csi(Vec<u8>),
    Ss3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorKey {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
}

impl InputEngine {
    pub(super) fn new(transfer_context: TransferContext) -> Self {
        Self {
            pending_remote_line: Vec::new(),
            escape_armed: false,
            local_input_len: 0,
            swallow_next_enter_pair: None,
            escape_history: CommandHistory::new(None),
            prompt_history: CommandHistory::new(None),
            transfer_context,
            mode: InputMode::Remote,
        }
    }

    pub(super) async fn process_stdin_chunk<S>(
        &mut self,
        session: &mut Session,
        stdout: &mut tokio::io::Stdout,
        stdin: &mut S,
        chunk: &[u8],
    ) -> Result<()>
    where
        S: tokio::io::AsyncRead + Unpin,
    {
        for &byte in chunk {
            if should_swallow_paired_enter(&mut self.swallow_next_enter_pair, byte) {
                continue;
            }

            let mode = std::mem::take(&mut self.mode);
            self.mode = match mode {
                InputMode::Remote => {
                    if self.can_start_escape() && byte == b'~' {
                        let session = LineSession {
                            editor: LineEditor::new_escape(),
                            visual_len: 1,
                            display_cursor: 1,
                            control_state: ControlSequenceState::None,
                        };
                        stdout.write_all(b"~").await?;
                        stdout.flush().await?;
                        self.escape_armed = false;
                        InputMode::EscapeLine(session)
                    } else {
                        session.send(&[byte]).await?;
                        self.observe_local_bytes(&[byte]);
                        InputMode::Remote
                    }
                }
                InputMode::EscapeLine(mut line) => {
                    match self
                        .process_line_byte(session, stdout, stdin, &mut line, byte, false)
                        .await?
                    {
                        NextMode::Remote => InputMode::Remote,
                        NextMode::EscapeLine => InputMode::EscapeLine(line),
                        NextMode::LocalPrompt => InputMode::LocalPrompt(line),
                    }
                }
                InputMode::LocalPrompt(mut line) => {
                    match self
                        .process_line_byte(session, stdout, stdin, &mut line, byte, true)
                        .await?
                    {
                        NextMode::Remote => InputMode::Remote,
                        NextMode::EscapeLine => InputMode::EscapeLine(line),
                        NextMode::LocalPrompt => InputMode::LocalPrompt(line),
                    }
                }
            };
        }

        Ok(())
    }

    pub(super) fn observe_remote_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            match byte {
                b'\r' | b'\n' => {
                    self.pending_remote_line.clear();
                    self.escape_armed = true;
                    self.local_input_len = 0;
                }
                _ => {
                    self.pending_remote_line.push(byte);
                    if self.pending_remote_line.len() > 1024 {
                        self.pending_remote_line.remove(0);
                    }
                }
            }
        }
    }

    pub(super) async fn redraw_after_remote_output(
        &mut self,
        stdout: &mut tokio::io::Stdout,
    ) -> Result<()> {
        let InputMode::LocalPrompt(line) = &mut self.mode else {
            return Ok(());
        };

        stdout.write_all(b"\r\n").await?;
        stdout.write_all(LOCAL_PROMPT.as_bytes()).await?;
        stdout.flush().await?;
        render_line(stdout, line).await
    }

    fn can_start_escape(&self) -> bool {
        self.escape_armed || self.local_input_len == 0
    }

    fn observe_local_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            match byte {
                b'\r' | b'\n' => {
                    self.pending_remote_line.clear();
                    self.escape_armed = true;
                    self.local_input_len = 0;
                }
                3 | 4 | 21 => {
                    self.pending_remote_line.clear();
                    self.escape_armed = false;
                    self.local_input_len = 0;
                }
                8 | 127 => {
                    self.pending_remote_line.pop();
                    self.escape_armed = false;
                    self.local_input_len = self.local_input_len.saturating_sub(1);
                }
                _ => {
                    self.pending_remote_line.push(byte);
                    self.escape_armed = false;
                    self.local_input_len += 1;
                }
            }
        }
    }

    async fn process_line_byte(
        &mut self,
        session: &mut Session,
        stdout: &mut tokio::io::Stdout,
        stdin: &mut (impl tokio::io::AsyncRead + Unpin),
        line: &mut LineSession,
        byte: u8,
        prompt_mode: bool,
    ) -> Result<NextMode> {
        if matches!(
            line.control_state,
            ControlSequenceState::Escape | ControlSequenceState::Csi(_) | ControlSequenceState::Ss3
        ) {
            if let Some(event) = consume_control_sequence(&mut line.control_state, byte) {
                let effect = if prompt_mode {
                    line.editor.apply(event, &mut self.prompt_history)
                } else {
                    line.editor.apply(event, &mut self.escape_history)
                };
                return self
                    .apply_editor_effect(session, stdout, stdin, line, effect, prompt_mode)
                    .await;
            }
            return Ok(if prompt_mode {
                NextMode::LocalPrompt
            } else {
                NextMode::EscapeLine
            });
        }

        if prompt_mode && byte == 3 {
            clear_line_preview(stdout, line).await?;
            stdout.write_all(b"\r\n").await?;
            stdout.flush().await?;
            let _ = session.send(b"\r").await;
            self.pending_remote_line.clear();
            self.escape_armed = true;
            self.local_input_len = 0;
            return Ok(NextMode::Remote);
        }

        let effect = match byte {
            27 => {
                line.control_state = ControlSequenceState::Escape;
                EditorEffect::NoOp
            }
            8 | 127 => {
                if prompt_mode {
                    line.editor
                        .apply(EditorEvent::Backspace, &mut self.prompt_history)
                } else {
                    line.editor
                        .apply(EditorEvent::Backspace, &mut self.escape_history)
                }
            }
            9 => {
                self.complete_line_input(session, stdout, line, prompt_mode)
                    .await?;
                return Ok(if prompt_mode {
                    NextMode::LocalPrompt
                } else {
                    NextMode::EscapeLine
                });
            }
            b'\r' | b'\n' => {
                self.swallow_next_enter_pair = paired_enter_byte(byte);
                if prompt_mode {
                    line.editor
                        .apply(EditorEvent::Submit, &mut self.prompt_history)
                } else {
                    line.editor
                        .apply(EditorEvent::Submit, &mut self.escape_history)
                }
            }
            _ => {
                if prompt_mode {
                    line.editor
                        .apply(EditorEvent::InsertByte(byte), &mut self.prompt_history)
                } else {
                    line.editor
                        .apply(EditorEvent::InsertByte(byte), &mut self.escape_history)
                }
            }
        };

        self.apply_editor_effect(session, stdout, stdin, line, effect, prompt_mode)
            .await
    }

    async fn apply_editor_effect(
        &mut self,
        session: &mut Session,
        stdout: &mut tokio::io::Stdout,
        stdin: &mut (impl tokio::io::AsyncRead + Unpin),
        line: &mut LineSession,
        effect: EditorEffect,
        prompt_mode: bool,
    ) -> Result<NextMode> {
        match effect {
            EditorEffect::NoOp => Ok(if prompt_mode {
                NextMode::LocalPrompt
            } else {
                NextMode::EscapeLine
            }),
            EditorEffect::Render => {
                render_line(stdout, line).await?;
                Ok(if prompt_mode {
                    NextMode::LocalPrompt
                } else {
                    NextMode::EscapeLine
                })
            }
            EditorEffect::ClearAndExit => {
                clear_line_preview(stdout, line).await?;
                Ok(NextMode::Remote)
            }
            EditorEffect::SubmitLocal {
                command,
                line: raw_line,
            } => {
                match command {
                    EscapeCommand::Help => {
                        self.local_input_len = 0;
                        self.pending_remote_line.clear();
                        self.escape_armed = true;
                        print_escape_help(stdout).await?;
                        let _ = session.send(b"\r").await;
                    }
                    EscapeCommand::LiteralTilde => {
                        clear_line_preview(stdout, line).await?;
                        session.send(b"~\r").await?;
                        self.observe_local_bytes(b"~\r");
                    }
                    EscapeCommand::Disconnect => {
                        self.local_input_len = 0;
                        self.pending_remote_line.clear();
                        self.escape_armed = false;
                        print_local_block(stdout, "Disconnecting...\n").await?;
                        let _ = session.disconnect().await;
                    }
                    EscapeCommand::Prompt => {
                        clear_line_preview(stdout, line).await?;
                        stdout.write_all(b"\r\nirosh> ").await?;
                        stdout.flush().await?;
                        self.pending_remote_line.clear();
                        self.local_input_len = 0;
                        *line = LineSession {
                            editor: LineEditor::new_prompt(),
                            visual_len: 0,
                            display_cursor: 0,
                            control_state: ControlSequenceState::None,
                        };
                        return Ok(NextMode::LocalPrompt);
                    }
                    EscapeCommand::Put | EscapeCommand::Get => {
                        self.local_input_len = 0;
                        self.pending_remote_line.clear();
                        self.escape_armed = true;
                        let command_line = String::from_utf8_lossy(&raw_line).to_string();
                        run_escape_transfer_command(
                            session,
                            stdout,
                            stdin,
                            &self.transfer_context,
                            &command_line,
                        )
                        .await?;
                        let _ = session.send(b"\r").await;
                    }
                }
                Ok(NextMode::Remote)
            }
            EditorEffect::SubmitPrompt(command) => {
                finalize_submitted_line(stdout, line).await?;
                match run_prompt_command(
                    session,
                    stdout,
                    stdin,
                    &mut self.transfer_context,
                    &command,
                )
                .await?
                {
                    PromptOutcome::Continue => {
                        stdout.write_all(b"\r\nirosh> ").await?;
                        stdout.flush().await?;
                        *line = LineSession {
                            editor: LineEditor::new_prompt(),
                            visual_len: 0,
                            display_cursor: 0,
                            control_state: ControlSequenceState::None,
                        };
                        Ok(NextMode::LocalPrompt)
                    }
                    PromptOutcome::Exit => {
                        stdout.write_all(b"\r\n").await?;
                        stdout.flush().await?;
                        let _ = session.send(b"\r").await;
                        self.pending_remote_line.clear();
                        self.escape_armed = true;
                        self.local_input_len = 0;
                        Ok(NextMode::Remote)
                    }
                    PromptOutcome::Disconnect => Ok(NextMode::Remote),
                }
            }
            EditorEffect::SubmitRemote(bytes) => {
                if prompt_mode {
                    return Ok(NextMode::LocalPrompt);
                }

                clear_line_preview(stdout, line).await?;
                session.send(&bytes).await?;
                self.observe_local_bytes(&bytes);
                Ok(NextMode::Remote)
            }
        }
    }

    async fn complete_line_input(
        &mut self,
        session: &mut Session,
        stdout: &mut tokio::io::Stdout,
        line: &mut LineSession,
        prompt_mode: bool,
    ) -> Result<()> {
        let completion = if prompt_mode {
            complete_prompt_line(
                session,
                &self.transfer_context,
                line.editor.line(),
                line.editor.cursor(),
            )
            .await?
        } else {
            complete_escape_line(
                session,
                &self.transfer_context,
                line.editor.line(),
                line.editor.cursor(),
            )
            .await?
        };

        match completion {
            CompletionResult::None => Ok(()),
            CompletionResult::Applied(edit) => {
                line.editor.replace_line(edit.line, edit.cursor);
                if prompt_mode {
                    self.prompt_history
                        .abandon_navigation(&String::from_utf8_lossy(line.editor.line()));
                } else {
                    self.escape_history
                        .abandon_navigation(&String::from_utf8_lossy(line.editor.line()));
                }
                render_line(stdout, line).await
            }
            CompletionResult::Suggestions(suggestions) => {
                print_completion_suggestions(stdout, &suggestions).await?;
                if prompt_mode {
                    stdout.write_all(LOCAL_PROMPT.as_bytes()).await?;
                    stdout.flush().await?;
                }
                render_line(stdout, line).await
            }
        }
    }
}

async fn clear_line_preview(stdout: &mut tokio::io::Stdout, line: &mut LineSession) -> Result<()> {
    move_to_end(stdout, line.display_cursor, line.visual_len).await?;
    for _ in 0..line.visual_len {
        stdout.write_all(b"\x08").await?;
    }
    stdout.write_all(b"\x1b[K").await?;
    stdout.flush().await?;
    line.visual_len = 0;
    line.display_cursor = 0;
    line.control_state = ControlSequenceState::None;
    Ok(())
}

async fn finalize_submitted_line(
    stdout: &mut tokio::io::Stdout,
    line: &mut LineSession,
) -> Result<()> {
    move_to_end(stdout, line.display_cursor, line.visual_len).await?;
    stdout.write_all(b"\r\n").await?;
    stdout.flush().await?;
    line.visual_len = 0;
    line.display_cursor = 0;
    line.control_state = ControlSequenceState::None;
    Ok(())
}

async fn render_line(stdout: &mut tokio::io::Stdout, line: &mut LineSession) -> Result<()> {
    move_to_end(stdout, line.display_cursor, line.visual_len).await?;
    for _ in 0..line.visual_len {
        stdout.write_all(b"\x08").await?;
    }
    stdout.write_all(b"\x1b[K").await?;
    stdout.write_all(line.editor.line()).await?;

    let cursor = line.editor.cursor();
    let tail_len = line.editor.line().len().saturating_sub(cursor);
    if tail_len > 0 {
        let movement = format!("\x1b[{}D", tail_len);
        stdout.write_all(movement.as_bytes()).await?;
    }

    stdout.flush().await?;
    line.visual_len = line.editor.line().len();
    line.display_cursor = cursor;
    Ok(())
}

async fn move_to_end(
    stdout: &mut tokio::io::Stdout,
    display_cursor: usize,
    visual_len: usize,
) -> Result<()> {
    let to_end = visual_len.saturating_sub(display_cursor);
    if to_end > 0 {
        let movement = format!("\x1b[{}C", to_end);
        stdout.write_all(movement.as_bytes()).await?;
    }
    Ok(())
}

fn paired_enter_byte(byte: u8) -> Option<u8> {
    match byte {
        b'\r' => Some(b'\n'),
        b'\n' => Some(b'\r'),
        _ => None,
    }
}

fn should_swallow_paired_enter(swallow_next: &mut Option<u8>, byte: u8) -> bool {
    if *swallow_next == Some(byte) {
        *swallow_next = None;
        return true;
    }

    if byte != b'\r' && byte != b'\n' {
        *swallow_next = None;
    }

    false
}

fn consume_control_sequence(state: &mut ControlSequenceState, byte: u8) -> Option<EditorEvent> {
    let key = match state {
        ControlSequenceState::None => return None,
        ControlSequenceState::Escape => {
            if byte == b'[' {
                *state = ControlSequenceState::Csi(Vec::new());
            } else if byte == b'O' {
                *state = ControlSequenceState::Ss3;
            } else {
                *state = ControlSequenceState::None;
            }
            return None;
        }
        ControlSequenceState::Csi(params) => {
            if byte.is_ascii_digit() || byte == b';' {
                params.push(byte);
                return None;
            }

            let key = match (params.as_slice(), byte) {
                ([], b'A') => Some(EditorKey::Up),
                ([], b'B') => Some(EditorKey::Down),
                ([], b'C') => Some(EditorKey::Right),
                ([], b'D') => Some(EditorKey::Left),
                ([], b'H') => Some(EditorKey::Home),
                ([], b'F') => Some(EditorKey::End),
                ([b'1'], b'~') | ([b'7'], b'~') => Some(EditorKey::Home),
                ([b'4'], b'~') | ([b'8'], b'~') => Some(EditorKey::End),
                ([b'3'], b'~') => Some(EditorKey::Delete),
                _ => None,
            };
            *state = ControlSequenceState::None;
            key
        }
        ControlSequenceState::Ss3 => {
            let key = match byte {
                b'A' => Some(EditorKey::Up),
                b'B' => Some(EditorKey::Down),
                b'C' => Some(EditorKey::Right),
                b'D' => Some(EditorKey::Left),
                b'H' => Some(EditorKey::Home),
                b'F' => Some(EditorKey::End),
                _ => None,
            };
            *state = ControlSequenceState::None;
            key
        }
    }?;

    Some(match key {
        EditorKey::Up => EditorEvent::HistoryUp,
        EditorKey::Down => EditorEvent::HistoryDown,
        EditorKey::Left => EditorEvent::MoveLeft,
        EditorKey::Right => EditorEvent::MoveRight,
        EditorKey::Home => EditorEvent::MoveHome,
        EditorKey::End => EditorEvent::MoveEnd,
        EditorKey::Delete => EditorEvent::Delete,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ControlSequenceState, EditorEvent, InputEngine, consume_control_sequence,
        paired_enter_byte, should_swallow_paired_enter,
    };
    use crate::commands::connect::transfer::TransferContext;

    fn test_transfer_context() -> TransferContext {
        let cwd = std::env::temp_dir();
        TransferContext { local_root: cwd }
    }

    #[test]
    fn escape_can_start_when_line_is_empty_even_if_not_armed() {
        let mut engine = InputEngine::new(test_transfer_context());
        engine.escape_armed = false;
        engine.local_input_len = 0;
        assert!(engine.can_start_escape());

        engine.local_input_len = 1;
        assert!(!engine.can_start_escape());

        engine.escape_armed = true;
        assert!(engine.can_start_escape());
    }

    #[test]
    fn remote_newline_rearms_escape_and_clears_local_line_state() {
        let mut engine = InputEngine::new(test_transfer_context());
        engine.local_input_len = 4;
        engine.escape_armed = false;
        engine.observe_remote_bytes(b"\r\n");
        assert!(engine.escape_armed);
        assert_eq!(engine.local_input_len, 0);
    }

    #[test]
    fn csi_arrow_sequences_decode_to_editor_events() {
        let mut state = ControlSequenceState::Escape;
        assert_eq!(consume_control_sequence(&mut state, b'['), None);
        assert_eq!(
            consume_control_sequence(&mut state, b'A'),
            Some(EditorEvent::HistoryUp)
        );
        assert_eq!(state, ControlSequenceState::None);
    }

    #[test]
    fn csi_delete_sequence_decodes() {
        let mut state = ControlSequenceState::Escape;
        assert_eq!(consume_control_sequence(&mut state, b'['), None);
        assert_eq!(consume_control_sequence(&mut state, b'3'), None);
        assert_eq!(
            consume_control_sequence(&mut state, b'~'),
            Some(EditorEvent::Delete)
        );
    }

    #[test]
    fn ss3_arrow_sequences_decode_to_editor_events() {
        let mut state = ControlSequenceState::Escape;
        assert_eq!(consume_control_sequence(&mut state, b'O'), None);
        assert_eq!(
            consume_control_sequence(&mut state, b'D'),
            Some(EditorEvent::MoveLeft)
        );
        assert_eq!(state, ControlSequenceState::None);
    }

    #[test]
    fn paired_enter_byte_maps_cr_and_lf() {
        assert_eq!(paired_enter_byte(b'\r'), Some(b'\n'));
        assert_eq!(paired_enter_byte(b'\n'), Some(b'\r'));
        assert_eq!(paired_enter_byte(b'x'), None);
    }

    #[test]
    fn swallow_paired_enter_consumes_only_matching_second_byte() {
        let mut swallow = Some(b'\n');
        assert!(should_swallow_paired_enter(&mut swallow, b'\n'));
        assert_eq!(swallow, None);

        let mut swallow = Some(b'\n');
        assert!(!should_swallow_paired_enter(&mut swallow, b'\r'));
        assert_eq!(swallow, Some(b'\n'));
    }

    #[test]
    fn swallow_paired_enter_clears_on_non_enter_input() {
        let mut swallow = Some(b'\n');
        assert!(!should_swallow_paired_enter(&mut swallow, b'a'));
        assert_eq!(swallow, None);
    }
}

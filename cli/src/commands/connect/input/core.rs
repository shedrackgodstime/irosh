use super::escape::{EscapeCommand, parse_escape_command};
use crate::commands::connect::support::CommandHistory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditorEvent {
    InsertByte(u8),
    Backspace,
    Delete,
    MoveLeft,
    MoveRight,
    MoveHome,
    MoveEnd,
    HistoryUp,
    HistoryDown,
    Submit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EditorEffect {
    NoOp,
    Render,
    ClearAndExit,
    SubmitLocal {
        command: EscapeCommand,
        line: Vec<u8>,
    },
    SubmitPrompt(Vec<u8>),
    SubmitRemote(Vec<u8>),
}

#[derive(Debug)]
pub(super) struct LineEditor {
    line: Vec<u8>,
    cursor: usize,
    submit_mode: SubmitMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SubmitMode {
    Escape,
    Prompt,
}

impl LineEditor {
    pub(super) fn new_escape() -> Self {
        Self {
            line: vec![b'~'],
            cursor: 1,
            submit_mode: SubmitMode::Escape,
        }
    }

    pub(super) fn new_prompt() -> Self {
        Self {
            line: Vec::new(),
            cursor: 0,
            submit_mode: SubmitMode::Prompt,
        }
    }

    pub(super) fn line(&self) -> &[u8] {
        &self.line
    }

    pub(super) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(super) fn replace_line(&mut self, line: Vec<u8>, cursor: usize) {
        self.line = line;
        self.cursor = cursor.min(self.line.len());
    }

    pub(super) fn apply(
        &mut self,
        event: EditorEvent,
        history: &mut CommandHistory,
    ) -> EditorEffect {
        match event {
            EditorEvent::InsertByte(byte) => {
                history.abandon_navigation(&String::from_utf8_lossy(&self.line));
                self.line.insert(self.cursor, byte);
                self.cursor += 1;
                EditorEffect::Render
            }
            EditorEvent::Backspace => {
                if self.cursor == 0 {
                    return EditorEffect::NoOp;
                }

                history.abandon_navigation(&String::from_utf8_lossy(&self.line));
                self.line.remove(self.cursor - 1);
                self.cursor -= 1;

                if self.line.is_empty() {
                    EditorEffect::ClearAndExit
                } else {
                    EditorEffect::Render
                }
            }
            EditorEvent::Delete => {
                if self.cursor >= self.line.len() {
                    return EditorEffect::NoOp;
                }

                history.abandon_navigation(&String::from_utf8_lossy(&self.line));
                self.line.remove(self.cursor);

                if self.line.is_empty() {
                    EditorEffect::ClearAndExit
                } else {
                    EditorEffect::Render
                }
            }
            EditorEvent::MoveLeft => {
                if self.cursor == 0 {
                    EditorEffect::NoOp
                } else {
                    self.cursor -= 1;
                    EditorEffect::Render
                }
            }
            EditorEvent::MoveRight => {
                if self.cursor >= self.line.len() {
                    EditorEffect::NoOp
                } else {
                    self.cursor += 1;
                    EditorEffect::Render
                }
            }
            EditorEvent::MoveHome => {
                if self.cursor == 0 {
                    EditorEffect::NoOp
                } else {
                    self.cursor = 0;
                    EditorEffect::Render
                }
            }
            EditorEvent::MoveEnd => {
                if self.cursor == self.line.len() {
                    EditorEffect::NoOp
                } else {
                    self.cursor = self.line.len();
                    EditorEffect::Render
                }
            }
            EditorEvent::HistoryUp => {
                let current = String::from_utf8_lossy(&self.line).to_string();
                let Some(entry) = history.up(&current) else {
                    return EditorEffect::NoOp;
                };

                self.line = entry.into_bytes();
                self.cursor = self.line.len();
                EditorEffect::Render
            }
            EditorEvent::HistoryDown => {
                let Some(entry) = history.down() else {
                    return EditorEffect::NoOp;
                };

                self.line = entry.into_bytes();
                self.cursor = self.line.len();
                EditorEffect::Render
            }
            EditorEvent::Submit => {
                if self.line.is_empty() {
                    return EditorEffect::ClearAndExit;
                }

                let command = String::from_utf8_lossy(&self.line).to_string();
                match self.submit_mode {
                    SubmitMode::Escape => match parse_escape_command(&command) {
                        Some(local) => {
                            history.add(&command);
                            EditorEffect::SubmitLocal {
                                command: local,
                                line: self.line.clone(),
                            }
                        }
                        None => {
                            let mut passthrough = self.line.clone();
                            passthrough.push(b'\r');
                            EditorEffect::SubmitRemote(passthrough)
                        }
                    },
                    SubmitMode::Prompt => {
                        history.add(&command);
                        EditorEffect::SubmitPrompt(self.line.clone())
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EditorEffect, EditorEvent, LineEditor};
    use crate::commands::connect::input::escape::EscapeCommand;
    use crate::commands::connect::support::CommandHistory;

    #[test]
    fn backspace_at_cursor_zero_is_noop() {
        let mut editor = LineEditor::new_escape();
        let mut history = CommandHistory::new(None);
        editor.apply(EditorEvent::MoveHome, &mut history);
        assert_eq!(
            editor.apply(EditorEvent::Backspace, &mut history),
            EditorEffect::NoOp
        );
        assert_eq!(editor.line(), b"~");
        assert_eq!(editor.cursor(), 0);
    }

    #[test]
    fn deleting_last_character_exits_editor() {
        let mut editor = LineEditor::new_escape();
        let mut history = CommandHistory::new(None);
        assert_eq!(
            editor.apply(EditorEvent::Backspace, &mut history),
            EditorEffect::ClearAndExit
        );
        assert_eq!(editor.line(), b"");
        assert_eq!(editor.cursor(), 0);
    }

    #[test]
    fn unknown_submit_passes_full_line_to_remote() {
        let mut editor = LineEditor::new_escape();
        let mut history = CommandHistory::new(None);
        editor.apply(EditorEvent::InsertByte(b'l'), &mut history);
        editor.apply(EditorEvent::InsertByte(b's'), &mut history);
        assert_eq!(
            editor.apply(EditorEvent::Submit, &mut history),
            EditorEffect::SubmitRemote(b"~ls\r".to_vec())
        );
    }

    #[test]
    fn known_submit_resolves_locally() {
        let mut editor = LineEditor::new_escape();
        let mut history = CommandHistory::new(None);
        editor.apply(EditorEvent::InsertByte(b'?'), &mut history);
        assert_eq!(
            editor.apply(EditorEvent::Submit, &mut history),
            EditorEffect::SubmitLocal {
                command: EscapeCommand::Help,
                line: b"~?".to_vec(),
            }
        );
    }

    #[test]
    fn known_submit_with_trailing_spaces_resolves_locally() {
        let mut editor = LineEditor::new_escape();
        let mut history = CommandHistory::new(None);

        for byte in b"help   " {
            editor.apply(EditorEvent::InsertByte(*byte), &mut history);
        }

        assert_eq!(
            editor.apply(EditorEvent::Submit, &mut history),
            EditorEffect::SubmitLocal {
                command: EscapeCommand::Help,
                line: b"~help   ".to_vec(),
            }
        );
    }

    #[test]
    fn inserting_tilde_at_front_after_typing_body_resolves_locally() {
        let mut editor = LineEditor::new_escape();
        let mut history = CommandHistory::new(None);

        assert_eq!(
            editor.apply(EditorEvent::Backspace, &mut history),
            EditorEffect::ClearAndExit
        );

        let mut editor = LineEditor {
            line: b"help".to_vec(),
            cursor: 4,
            submit_mode: super::SubmitMode::Escape,
        };

        editor.apply(EditorEvent::MoveHome, &mut history);
        editor.apply(EditorEvent::InsertByte(b'~'), &mut history);

        assert_eq!(editor.line(), b"~help");
        assert_eq!(
            editor.apply(EditorEvent::Submit, &mut history),
            EditorEffect::SubmitLocal {
                command: EscapeCommand::Help,
                line: b"~help".to_vec(),
            }
        );
    }

    #[test]
    fn history_navigation_restores_pending_line() {
        let mut history = CommandHistory::new(None);
        let mut first = LineEditor::new_escape();
        first.apply(EditorEvent::InsertByte(b'?'), &mut history);
        assert_eq!(
            first.apply(EditorEvent::Submit, &mut history),
            EditorEffect::SubmitLocal {
                command: EscapeCommand::Help,
                line: b"~?".to_vec(),
            }
        );

        let mut editor = LineEditor::new_escape();
        editor.apply(EditorEvent::InsertByte(b'h'), &mut history);
        editor.apply(EditorEvent::InsertByte(b'e'), &mut history);
        editor.apply(EditorEvent::InsertByte(b'l'), &mut history);
        editor.apply(EditorEvent::InsertByte(b'p'), &mut history);
        assert_eq!(
            editor.apply(EditorEvent::HistoryUp, &mut history),
            EditorEffect::Render
        );
        assert_eq!(editor.line(), b"~?");
        assert_eq!(
            editor.apply(EditorEvent::HistoryDown, &mut history),
            EditorEffect::Render
        );
        assert_eq!(editor.line(), b"~help");
    }

    #[test]
    fn prompt_submit_stays_local() {
        let mut editor = LineEditor::new_prompt();
        let mut history = CommandHistory::new(None);
        editor.apply(EditorEvent::InsertByte(b'p'), &mut history);
        editor.apply(EditorEvent::InsertByte(b'w'), &mut history);
        editor.apply(EditorEvent::InsertByte(b'd'), &mut history);
        assert_eq!(
            editor.apply(EditorEvent::Submit, &mut history),
            EditorEffect::SubmitPrompt(b"pwd".to_vec())
        );
    }
}

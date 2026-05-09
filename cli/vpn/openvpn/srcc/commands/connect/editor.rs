use super::history::CommandHistory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorEvent {
    InsertByte(u8),
    Backspace,
    Delete,
    MoveLeft,
    MoveRight,
    MoveHome,
    MoveEnd,
    HistoryUp,
    HistoryDown,
    Tab,
    Submit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorEffect {
    NoOp,
    Render,
    ClearAndExit,
    SubmitPrompt(Vec<u8>),
    SubmitEscape(Vec<u8>),
    RequestCompletion,
}

#[derive(Debug)]
pub struct LineEditor {
    line: Vec<u8>,
    cursor: usize,
    mode: EditorMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Escape,
    Prompt,
}

impl LineEditor {
    pub fn new_escape() -> Self {
        Self {
            line: vec![b'~'],
            cursor: 1,
            mode: EditorMode::Escape,
        }
    }

    pub fn new_prompt() -> Self {
        Self {
            line: Vec::new(),
            cursor: 0,
            mode: EditorMode::Prompt,
        }
    }

    pub fn line(&self) -> &[u8] {
        &self.line
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn mode(&self) -> EditorMode {
        self.mode
    }

    pub fn replace_line(&mut self, line: Vec<u8>, cursor: usize) {
        self.line = line;
        self.cursor = cursor.min(self.line.len());
    }

    pub fn apply(&mut self, event: EditorEvent, history: &mut CommandHistory) -> EditorEffect {
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

                if self.line.is_empty() && self.mode == EditorMode::Escape {
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
                EditorEffect::Render
            }
            EditorEvent::MoveLeft => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    EditorEffect::Render
                } else {
                    EditorEffect::NoOp
                }
            }
            EditorEvent::MoveRight => {
                if self.cursor < self.line.len() {
                    self.cursor += 1;
                    EditorEffect::Render
                } else {
                    EditorEffect::NoOp
                }
            }
            EditorEvent::MoveHome => {
                self.cursor = 0;
                EditorEffect::Render
            }
            EditorEvent::MoveEnd => {
                self.cursor = self.line.len();
                EditorEffect::Render
            }
            EditorEvent::HistoryUp => {
                if let Some(prev) = history.up(&String::from_utf8_lossy(&self.line)) {
                    self.line = prev.into_bytes();
                    self.cursor = self.line.len();
                    EditorEffect::Render
                } else {
                    EditorEffect::NoOp
                }
            }
            EditorEvent::HistoryDown => {
                if let Some(next) = history.down() {
                    self.line = next.into_bytes();
                    self.cursor = self.line.len();
                    EditorEffect::Render
                } else {
                    EditorEffect::NoOp
                }
            }
            EditorEvent::Tab => {
                EditorEffect::RequestCompletion
            }
            EditorEvent::Submit => {
                let submitted = self.line.clone();
                match self.mode {
                    EditorMode::Prompt => EditorEffect::SubmitPrompt(submitted),
                    EditorMode::Escape => EditorEffect::SubmitEscape(submitted),
                }
            }
        }
    }
}

use std::fs;
use std::path::PathBuf;

/// Manages persistent and in-memory history for local commands.
#[derive(Debug)]
pub struct CommandHistory {
    path: Option<PathBuf>,
    entries: Vec<String>,
    index: Option<usize>,
    pending: String,
}

impl CommandHistory {
    pub fn new(path: Option<PathBuf>) -> Self {
        let mut entries = Vec::new();
        if let Some(ref p) = path {
            if let Ok(content) = fs::read_to_string(p) {
                entries = content.lines().map(|s| s.to_string()).collect();
            }
        }

        Self {
            path,
            entries,
            index: None,
            pending: String::new(),
        }
    }

    /// Adds a new command to the history and persists it.
    pub fn add(&mut self, command: &str) {
        let trimmed = command.trim();
        if trimmed.is_empty() || trimmed == ":" {
            return;
        }

        // Avoid duplicate consecutive entries.
        if self.entries.last().map(|s| s.as_str()) == Some(trimmed) {
            self.index = None;
            return;
        }

        self.entries.push(trimmed.to_string());
        self.index = None;
        self.append_to_file(trimmed);
    }

    /// Moves the history pointer up (older commands).
    pub fn up(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }

        let new_index = match self.index {
            None => {
                self.pending = current.to_string();
                self.entries.len().saturating_sub(1)
            }
            Some(i) => i.saturating_sub(1),
        };

        self.index = Some(new_index);
        Some(self.entries[new_index].clone())
    }

    /// Moves the history pointer down (newer commands).
    pub fn down(&mut self) -> Option<String> {
        let current_index = self.index?;

        if current_index + 1 >= self.entries.len() {
            self.index = None;
            return Some(self.pending.clone());
        }

        let new_index = current_index + 1;
        self.index = Some(new_index);
        Some(self.entries[new_index].clone())
    }

    /// Resets the history pointer.
    pub fn reset(&mut self) {
        self.index = None;
        self.pending.clear();
    }

    fn append_to_file(&self, entry: &str) {
        if let Some(ref p) = self.path {
            use std::fs::OpenOptions;
            use std::io::Write;

            if let Some(parent) = p.parent() {
                let _ = fs::create_dir_all(parent);
            }

            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(p) {
                let _ = writeln!(file, "{}", entry);
            }
        }
    }
}

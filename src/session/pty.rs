//! Pseudo-terminal (PTY) helper functions and terminal state manipulation.

pub use portable_pty::PtySize;

/// Declarative PTY request options for an SSH session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOptions {
    term: String,
    size: PtySize,
    modes: Vec<(russh::Pty, u32)>,
}

impl PtyOptions {
    /// Creates PTY options for the given terminal kind and size.
    pub fn new(term: impl Into<String>, size: PtySize) -> Self {
        Self {
            term: term.into(),
            size,
            modes: Vec::new(),
        }
    }

    /// Replaces the terminal modes sent in the request.
    pub fn modes(mut self, modes: impl Into<Vec<(russh::Pty, u32)>>) -> Self {
        self.modes = modes.into();
        self
    }

    /// Returns the terminal identifier that will be requested.
    pub fn term(&self) -> &str {
        &self.term
    }

    /// Returns the requested PTY size.
    pub fn size(&self) -> PtySize {
        self.size
    }

    /// Returns the encoded terminal modes.
    pub fn modes_slice(&self) -> &[(russh::Pty, u32)] {
        &self.modes
    }
}

/// Returns a fallback pseudo-terminal size if probing the active terminal fails.
pub fn default_pty_size() -> PtySize {
    let rows = std::env::var("LINES")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(24);
    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(80);
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Clamps the requested PTY dimensions to safe bounds and converts them to [`PtySize`].
pub fn pty_size(cols: u32, rows: u32, pixel_width: u32, pixel_height: u32) -> PtySize {
    PtySize {
        rows: rows.clamp(1, u16::MAX as u32) as u16,
        cols: cols.clamp(1, u16::MAX as u32) as u16,
        pixel_width: pixel_width.clamp(0, u16::MAX as u32) as u16,
        pixel_height: pixel_height.clamp(0, u16::MAX as u32) as u16,
    }
}

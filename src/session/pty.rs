//! Pseudo-terminal (PTY) helper functions and terminal state manipulation.

pub use portable_pty::PtySize;

/// Declarative PTY request options for an SSH session.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "builders do nothing unless consumed"]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_options_new_sets_term_and_size() {
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let opts = PtyOptions::new("xterm-256color", size);
        assert_eq!(opts.term(), "xterm-256color");
        assert_eq!(opts.size(), size);
        assert!(opts.modes_slice().is_empty());
    }

    #[test]
    fn pty_options_modes_replaces_modes() {
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let modes = vec![(russh::Pty::ECHO, 1u32)];
        let opts = PtyOptions::new("xterm", size).modes(modes.clone());
        assert_eq!(opts.modes_slice(), &modes);
    }

    #[test]
    fn pty_options_clone() {
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let a = PtyOptions::new("xterm", size);
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn pty_size_clamps_rows_to_minimum() {
        let result = pty_size(80, 0, 0, 0);
        assert_eq!(result.rows, 1);
    }

    #[test]
    fn pty_size_clamps_cols_to_minimum() {
        let result = pty_size(0, 24, 0, 0);
        assert_eq!(result.cols, 1);
    }

    #[test]
    fn pty_size_clamps_to_maximum() {
        let large = u16::MAX as u32 + 100;
        let result = pty_size(large, large, large, large);
        assert_eq!(result.cols, u16::MAX);
        assert_eq!(result.rows, u16::MAX);
        assert_eq!(result.pixel_width, u16::MAX);
        assert_eq!(result.pixel_height, u16::MAX);
    }

    #[test]
    fn pty_size_preserves_valid_values() {
        let result = pty_size(132, 43, 800, 600);
        assert_eq!(result.cols, 132);
        assert_eq!(result.rows, 43);
        assert_eq!(result.pixel_width, 800);
        assert_eq!(result.pixel_height, 600);
    }

    #[test]
    fn pty_size_pixels_cannot_be_negative() {
        let result = pty_size(80, 24, 0, 0);
        // u32 cannot be negative, but clamping from 0 is identity
        assert_eq!(result.pixel_width, 0);
        assert_eq!(result.pixel_height, 0);
    }

    #[test]
    fn pty_size_accepts_max_u32_pixels_safely() {
        let result = pty_size(80, 24, u32::MAX, u32::MAX);
        assert_eq!(result.pixel_width, u16::MAX);
        assert_eq!(result.pixel_height, u16::MAX);
    }

    #[test]
    fn default_pty_size_falls_back_when_no_env() {
        // Temporarily remove LINES and COLUMNS to test fallback
        let prev_lines = std::env::var("LINES").ok();
        let prev_cols = std::env::var("COLUMNS").ok();
        unsafe {
            std::env::remove_var("LINES");
            std::env::remove_var("COLUMNS");
        }

        let size = default_pty_size();
        assert!(size.rows > 0);
        assert!(size.cols > 0);

        // Restore
        if let Some(val) = prev_lines {
            unsafe { std::env::set_var("LINES", val) };
        }
        if let Some(val) = prev_cols {
            unsafe { std::env::set_var("COLUMNS", val) };
        }
    }
}

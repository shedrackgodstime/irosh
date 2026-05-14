//! Terminal state management and RAII guards for raw mode and VT processing.

use anyhow::Result;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::{Write, stdout};

/// An RAII guard that manages the terminal state.
/// When created, it enables raw mode and ensures VT processing is active on Windows.
/// When dropped, it restores the terminal to its original cooked state.
pub struct TerminalGuard {
    is_raw: bool,
}

impl TerminalGuard {
    /// Create a new guard and enter raw mode.
    pub fn new() -> Result<Self> {
        #[cfg(windows)]
        Self::ensure_windows_vt()?;

        enable_raw_mode()?;
        Ok(Self { is_raw: true })
    }

    /// Explicitly leave raw mode.
    #[allow(dead_code)]
    pub fn leave_raw_mode(&mut self) -> Result<()> {
        if self.is_raw {
            disable_raw_mode()?;
            self.is_raw = false;
        }
        Ok(())
    }

    /// Re-enter raw mode.
    #[allow(dead_code)]
    pub fn enter_raw_mode(&mut self) -> Result<()> {
        if !self.is_raw {
            enable_raw_mode()?;
            self.is_raw = true;
        }
        Ok(())
    }

    /// Reset the terminal to a clean state: reset colors, show cursor, and clear line.
    /// This is the "Nuclear Cleanup" defined in the permanent terminal solution.
    pub fn nuclear_cleanup(&self) -> Result<()> {
        let mut out = stdout();
        // \x1b[0m   - Reset styles
        // \x1b[?25h - Show cursor
        // \r        - Move to start of line
        // \x1b[K    - Clear line
        out.write_all(b"\x1b[0m\x1b[?25h\r\x1b[K")?;
        out.flush()?;
        Ok(())
    }

    #[cfg(windows)]
    fn ensure_windows_vt() -> Result<()> {
        use windows_sys::Win32::System::Console::*;
        unsafe {
            let stdout_handle = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut mode = 0;
            if GetConsoleMode(stdout_handle, &mut mode) != 0 {
                SetConsoleMode(
                    stdout_handle,
                    mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING | DISABLE_NEWLINE_AUTO_RETURN,
                );
            }

            let stdin_handle = GetStdHandle(STD_INPUT_HANDLE);
            if GetConsoleMode(stdin_handle, &mut mode) != 0 {
                SetConsoleMode(stdin_handle, mode | ENABLE_VIRTUAL_TERMINAL_INPUT);
            }
        }
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.is_raw {
            let _ = disable_raw_mode();
        }
        // Final safety cleanup
        let _ = self.nuclear_cleanup();
    }
}

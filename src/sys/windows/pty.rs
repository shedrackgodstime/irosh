//! Windows implementation of PTY and terminal handling.

use crate::error::{ClientError, IroshError, Result};
pub use portable_pty::PtySize;
use std::fmt;

/// Places the current physical Windows terminal into raw mode and restores it
/// automatically on `Drop`. This captures keystrokes without local processing.
pub struct RawTerminal {
    in_handle: windows_sys::Win32::Foundation::HANDLE,
    in_original_mode: u32,
    out_handle: windows_sys::Win32::Foundation::HANDLE,
    out_original_mode: u32,
}

impl fmt::Debug for RawTerminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawTerminal").finish()
    }
}

impl RawTerminal {
    /// Puts the standard input handle into raw mode and enables VT processing on stdout.
    pub fn new(_fd: i32) -> Result<Self> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Console::*;

        unsafe {
            let in_handle = GetStdHandle(STD_INPUT_HANDLE);
            let out_handle = GetStdHandle(STD_OUTPUT_HANDLE);
            if in_handle == INVALID_HANDLE_VALUE || out_handle == INVALID_HANDLE_VALUE {
                return Err(IroshError::Client(ClientError::TerminalIo {
                    source: std::io::Error::last_os_error(),
                }));
            }

            let mut in_mode = 0;
            if GetConsoleMode(in_handle, &mut in_mode) == 0 {
                return Err(IroshError::Client(ClientError::TerminalIo {
                    source: std::io::Error::last_os_error(),
                }));
            }
            let in_original_mode = in_mode;

            let mut out_mode = 0;
            let out_original_mode = if GetConsoleMode(out_handle, &mut out_mode) != 0 {
                let out_original = out_mode;
                let new_out_mode =
                    out_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING | DISABLE_NEWLINE_AUTO_RETURN;
                let _ = SetConsoleMode(out_handle, new_out_mode);
                out_original
            } else {
                0
            };

            let raw_in_mode = (in_mode
                & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
                | ENABLE_VIRTUAL_TERMINAL_INPUT;

            if SetConsoleMode(in_handle, raw_in_mode) == 0 {
                let raw_mode_basic =
                    in_mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT);
                if SetConsoleMode(in_handle, raw_mode_basic) == 0 {
                    return Err(IroshError::Client(ClientError::TerminalIo {
                        source: std::io::Error::last_os_error(),
                    }));
                }
            }

            Ok(Self {
                in_handle,
                in_original_mode,
                out_handle,
                out_original_mode,
            })
        }
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Console::SetConsoleMode;
        unsafe {
            let _ = SetConsoleMode(self.in_handle, self.in_original_mode);
            if self.out_original_mode != 0 {
                let _ = SetConsoleMode(self.out_handle, self.out_original_mode);
            }
        }
    }
}

/// Probes the physical terminal size.
pub fn current_terminal_size() -> PtySize {
    use windows_sys::Win32::System::Console::*;
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut info = std::mem::zeroed::<CONSOLE_SCREEN_BUFFER_INFO>();
        if GetConsoleScreenBufferInfo(handle, &mut info) != 0 {
            return PtySize {
                rows: (info.srWindow.Bottom - info.srWindow.Top + 1) as u16,
                cols: (info.srWindow.Right - info.srWindow.Left + 1) as u16,
                pixel_width: 0,
                pixel_height: 0,
            };
        }
    }
    crate::session::pty::default_pty_size()
}

/// A wrapper around standard stdin for Windows.
pub struct AsyncStdin {
    inner: tokio::io::Stdin,
}

impl AsyncStdin {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: tokio::io::stdin(),
        })
    }
}

impl tokio::io::AsyncRead for AsyncStdin {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

/// Dummy signal mapping for Windows compatibility.
pub fn map_sig(_signal: russh::Sig) -> Option<i32> {
    None
}

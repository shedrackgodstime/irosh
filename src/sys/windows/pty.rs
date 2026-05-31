//! Windows implementation of PTY and terminal handling.

use crate::error::{ClientError, IroshError, Result};
pub use portable_pty::PtySize;
use std::fmt;
use tokio::sync::mpsc;
use windows_sys::Win32::System::Console::*;

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

// SAFETY: `RawTerminal` wraps standard console handles (`GetStdHandle`) which are
// documented to be safe to use from any thread. The handles are only read/written
// through synchronized Win32 console API calls.
unsafe impl Send for RawTerminal {}
unsafe impl Sync for RawTerminal {}

impl RawTerminal {
    /// Puts the standard input handle into raw mode and enables VT processing on stdout.
    pub fn new(_fd: i32) -> Result<Self> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

        // SAFETY: Windows API calls for terminal mode manipulation.
        // We check for invalid handles and store original modes for restoration.
        unsafe {
            let in_handle = GetStdHandle(STD_INPUT_HANDLE);
            let out_handle = GetStdHandle(STD_OUTPUT_HANDLE);
            if in_handle == INVALID_HANDLE_VALUE || out_handle == INVALID_HANDLE_VALUE {
                return Err(IroshError::Client(ClientError::TerminalIo {
                    source: std::io::Error::last_os_error(),
                }));
            }

            // Enforce UTF-8 code page for the client terminal to match the server.
            SetConsoleCP(65001);
            SetConsoleOutputCP(65001);

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

            // For raw mode, we want to disable line input, echo, and processed input.
            // We also enable VT input to get sequences like arrows as VT codes natively from the driver.
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
        // SAFETY: Restoring original console modes using stored handles.
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
    // SAFETY: Querying standard output handle for console buffer information.
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

/// Events that can occur on a Windows terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// Raw data received from stdin.
    Data(Vec<u8>),
    /// The terminal window was resized.
    Resize(PtySize),
}

/// A robust asynchronous terminal input and event reader for Windows.
///
/// Uses a dedicated background thread to poll `ReadConsoleInputW`, allowing
/// concurrent capture of both raw keystrokes and console events (like resize).
pub struct AsyncStdin {
    rx: mpsc::UnboundedReceiver<TerminalEvent>,
}

impl AsyncStdin {
    /// Spawns the background input polling thread.
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let tx_resize = tx.clone();

        // We use ReadFile instead of ReadConsoleInputW because ReadFile
        // respects ENABLE_VIRTUAL_TERMINAL_INPUT and automatically translates
        // keys (like arrows, backspace) into standard VT sequences, providing
        // 100% parity with Linux terminals.
        std::thread::Builder::new()
            .name("irosh-win-input".to_string())
            .spawn(move || {
                // SAFETY: Polling standard input using ReadFile.
                let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
                let mut buf = [0u8; 1024];

                loop {
                    let mut read = 0;
                    if unsafe {
                        windows_sys::Win32::Storage::FileSystem::ReadFile(
                            handle,
                            buf.as_mut_ptr() as *mut _,
                            buf.len() as u32,
                            &mut read,
                            std::ptr::null_mut(),
                        )
                    } == 0
                    {
                        break;
                    }
                    if read == 0 {
                        break;
                    }

                    if tx
                        .send(TerminalEvent::Data(buf[..read as usize].to_vec()))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|e| IroshError::Client(ClientError::TerminalIo { source: e }))?;

        // Background task to poll for terminal size changes since ReadFile
        // does not yield WINDOW_BUFFER_SIZE_EVENT.
        tokio::spawn(async move {
            let mut last_size = current_terminal_size();
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                let size = current_terminal_size();
                if size.cols != last_size.cols || size.rows != last_size.rows {
                    last_size = size;
                    if tx_resize.send(TerminalEvent::Resize(size)).is_err() {
                        break;
                    }
                }
            }
        });

        Ok(Self { rx })
    }

    /// Reads the next terminal event (Data or Resize).
    pub async fn next_event(&mut self) -> Option<TerminalEvent> {
        self.rx.recv().await
    }

    /// Reads the next chunk of raw input data.
    /// Returns `None` when the channel is closed (process exiting).
    pub async fn read_data(&mut self) -> Option<Vec<u8>> {
        loop {
            match self.rx.recv().await? {
                TerminalEvent::Data(data) => return Some(data),
                TerminalEvent::Resize(_) => continue, // resize handled separately via next_event
            }
        }
    }

    /// Low-level poll function. Prefer `read_data()` for use in `tokio::select!`.
    pub fn poll_next(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<TerminalEvent>> {
        self.rx.poll_recv(cx)
    }
}

/// Maps SSH signals to Windows term signals.
///
/// Windows does not have a native POSIX signal model, so SSH signals
/// (e.g., SIGINT, SIGTERM) are ignored here. The remote peer will not
/// receive process-level signals through the SSH channel on Windows hosts.
pub fn map_sig(_signal: russh::Sig) -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_sig_always_returns_none() {
        assert_eq!(map_sig(russh::Sig::INT), None);
        assert_eq!(map_sig(russh::Sig::TERM), None);
        assert_eq!(map_sig(russh::Sig::KILL), None);
        assert_eq!(map_sig(russh::Sig::Custom("test".into())), None);
    }
}

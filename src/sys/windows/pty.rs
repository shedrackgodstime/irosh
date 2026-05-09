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

// Windows HANDLEs are pointers, but standard stream handles are safe to send across threads.
unsafe impl Send for RawTerminal {}
unsafe impl Sync for RawTerminal {}

impl RawTerminal {
    /// Puts the standard input handle into raw mode and enables VT processing on stdout.
    pub fn new(_fd: i32) -> Result<Self> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

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

            // For raw mode, we want to disable line input, echo, and processed input.
            // We also enable VT input to get sequences like arrows as VT codes.
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

        std::thread::Builder::new()
            .name("irosh-win-input".to_string())
            .spawn(move || {
                let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
                let mut buffer = [unsafe { std::mem::zeroed::<INPUT_RECORD>() }; 128];

                loop {
                    let mut read = 0;
                    if unsafe { ReadConsoleInputW(handle, buffer.as_mut_ptr(), 128, &mut read) }
                        == 0
                    {
                        break;
                    }

                    for i in 0..read {
                        let record = buffer[i as usize];
                        match record.EventType as u32 {
                            KEY_EVENT => {
                                let key = unsafe { record.Event.KeyEvent };
                                if key.bKeyDown != 0 {
                                    // Use the Unicode character if available
                                    let c = unsafe { key.uChar.UnicodeChar };
                                    if c != 0 {
                                        let mut utf8 = [0u8; 4];
                                        let s = char::from_u32(c as u32)
                                            .unwrap_or(' ')
                                            .encode_utf8(&mut utf8);
                                        if tx
                                            .send(TerminalEvent::Data(s.as_bytes().to_vec()))
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                            }
                            WINDOW_BUFFER_SIZE_EVENT => {
                                let _ = tx.send(TerminalEvent::Resize(current_terminal_size()));
                            }
                            _ => {}
                        }
                    }
                }
            })
            .map_err(|e| IroshError::Client(ClientError::TerminalIo { source: e }))?;

        Ok(Self { rx })
    }

    /// Reads the next chunk of raw input data.
    /// On Windows, this receives from the background input thread's channel.
    /// Returns `None` when the channel is closed (process exiting).
    pub async fn read_data(&mut self) -> Option<Vec<u8>> {
        loop {
            match self.rx.recv().await? {
                TerminalEvent::Data(data) => return Some(data),
                TerminalEvent::Resize(_) => continue, // resize handled separately
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

/// Dummy signal mapping for Windows compatibility.
pub fn map_sig(_signal: russh::Sig) -> Option<i32> {
    None
}

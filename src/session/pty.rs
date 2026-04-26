//! Pseudo-terminal (PTY) helper functions and terminal state manipulation.
//!
//! This module handles size negotiation, signals, and raw mode manipulation
//! for interactive SSH sessions.

use std::fmt;

pub use portable_pty::PtySize;

use crate::error::Result;

#[cfg(unix)]
use crate::error::{ClientError, IroshError};

/// Declarative PTY request options for an SSH session.
///
/// `PtyOptions` bundles terminal identity, size, and terminal-mode overrides
/// for use with [`crate::Session::request_pty`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOptions {
    term: String,
    size: PtySize,
    modes: Vec<(russh::Pty, u32)>,
}

impl PtyOptions {
    /// Creates PTY options for the given terminal kind and size.
    ///
    /// This is typically used together with [`current_terminal_size`] or
    /// [`default_pty_size`] when preparing an interactive shell session.
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

#[cfg(unix)]
/// Places the current physical Unix terminal into raw mode and restores it
/// automatically on `Drop`. This captures keystrokes without local processing.
pub struct RawTerminal {
    fd: i32,
    original: libc::termios,
}

#[cfg(unix)]
impl fmt::Debug for RawTerminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawTerminal").field("fd", &self.fd).finish()
    }
}

#[cfg(unix)]
impl RawTerminal {
    /// Puts the given file descriptor into raw mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal attributes cannot be read or if raw
    /// mode cannot be applied to the provided file descriptor.
    pub fn new(fd: i32) -> Result<Self> {
        // SAFETY: `fd` is supplied by the caller as a valid terminal file descriptor.
        // We allocate a zeroed `termios` only as temporary storage for `tcgetattr`
        // to populate, then pass that initialized structure back to libc APIs that
        // require a mutable `termios` pointer for this same file descriptor.
        unsafe {
            let mut termios = std::mem::zeroed::<libc::termios>();
            if libc::tcgetattr(fd, &mut termios) != 0 {
                return Err(IroshError::Client(ClientError::TerminalIo {
                    source: std::io::Error::last_os_error(),
                }));
            }
            let original = termios;
            libc::cfmakeraw(&mut termios);
            if libc::tcsetattr(fd, libc::TCSANOW, &termios) != 0 {
                // If it fails, attempt to restore the original before returning the error.
                let _ = libc::tcsetattr(fd, libc::TCSANOW, &original);
                return Err(IroshError::Client(ClientError::TerminalIo {
                    source: std::io::Error::last_os_error(),
                }));
            }
            Ok(Self { fd, original })
        }
    }
}

#[cfg(unix)]
impl Drop for RawTerminal {
    fn drop(&mut self) {
        // SAFETY: `self.fd` and `self.original` were captured from a successful
        // `RawTerminal::new` call for this process and are only used here to
        // best-effort restore the previous terminal settings on drop.
        unsafe {
            let _ = libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
        }
    }
}

#[cfg(unix)]
/// Probes the physical terminal size utilizing the `TIOCGWINSZ` syscall.
pub fn current_terminal_size() -> PtySize {
    // SAFETY: `winsize` is temporary output storage for `ioctl(TIOCGWINSZ)`,
    // and `STDOUT_FILENO` is the conventional terminal file descriptor for the
    // current process. On failure we ignore the contents and fall back.
    unsafe {
        let mut winsize = std::mem::zeroed::<libc::winsize>();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut winsize) == 0 {
            return PtySize {
                rows: winsize.ws_row.max(1),
                cols: winsize.ws_col.max(1),
                pixel_width: winsize.ws_xpixel,
                pixel_height: winsize.ws_ypixel,
            };
        }
    }
    default_pty_size()
}

#[cfg(unix)]
/// A truly non-blocking asynchronous stdin reader for Unix terminals.
///
/// This avoids the background threads used by `tokio::io::stdin()` and
/// ensures the process can exit immediately without waiting for a final newline.
pub struct AsyncStdin {
    inner: tokio::io::unix::AsyncFd<std::io::Stdin>,
    original_flags: libc::c_int,
}

#[cfg(unix)]
impl AsyncStdin {
    /// Creates a new `AsyncStdin` by setting the global stdin to non-blocking mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the stdin file descriptor cannot be manipulated.
    pub fn new() -> Result<Self> {
        use std::os::unix::io::AsRawFd;
        let fd = std::io::stdin().as_raw_fd();

        // SAFETY: We are performing standard fcntl operations to enable non-blocking I/O.
        // We capture the original flags to ensure they can be restored on drop.
        unsafe {
            let original_flags = libc::fcntl(fd, libc::F_GETFL);
            if original_flags == -1 {
                return Err(IroshError::Client(ClientError::TerminalIo {
                    source: std::io::Error::last_os_error(),
                }));
            }
            if libc::fcntl(fd, libc::F_SETFL, original_flags | libc::O_NONBLOCK) == -1 {
                return Err(IroshError::Client(ClientError::TerminalIo {
                    source: std::io::Error::last_os_error(),
                }));
            }
            Ok(Self {
                inner: tokio::io::unix::AsyncFd::new(std::io::stdin())?,
                original_flags,
            })
        }
    }

    /// Attempts to read data from stdin into the provided buffer.
    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::io::Read;
        loop {
            let mut guard = self.inner.readable_mut().await?;
            match guard.try_io(|inner| inner.get_mut().read(buf)) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }
}

#[cfg(unix)]
impl tokio::io::AsyncRead for AsyncStdin {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::io::Read;
        let self_mut = self.get_mut();
        loop {
            let mut guard = match self_mut.inner.poll_read_ready(cx) {
                std::task::Poll::Ready(Ok(g)) => g,
                std::task::Poll::Ready(Err(e)) => return std::task::Poll::Ready(Err(e)),
                std::task::Poll::Pending => return std::task::Poll::Pending,
            };

            match guard.try_io(|inner| {
                let mut b = vec![0u8; buf.remaining()];
                match inner.get_ref().read(&mut b) {
                    Ok(n) => {
                        buf.put_slice(&b[..n]);
                        Ok(n)
                    }
                    Err(e) => Err(e),
                }
            }) {
                Ok(Ok(_)) => return std::task::Poll::Ready(Ok(())),
                Ok(Err(e)) => return std::task::Poll::Ready(Err(e)),
                Err(_would_block) => continue,
            }
        }
    }
}

#[cfg(unix)]
impl Drop for AsyncStdin {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        let fd = std::io::stdin().as_raw_fd();
        // SAFETY: Best-effort restoration of the original stdin flags.
        unsafe {
            let _ = libc::fcntl(fd, libc::F_SETFL, self.original_flags);
        }
    }
}

#[cfg(not(unix))]
/// Probes the physical terminal size. On non-Unix, defaults are returned.
pub fn current_terminal_size() -> PtySize {
    default_pty_size()
}

#[cfg(unix)]
/// Maps a russh protocol signal representation into the local libc signal ID.
pub fn map_sig(signal: russh::Sig) -> Option<libc::c_int> {
    match signal {
        russh::Sig::ABRT => Some(libc::SIGABRT),
        russh::Sig::ALRM => Some(libc::SIGALRM),
        russh::Sig::FPE => Some(libc::SIGFPE),
        russh::Sig::HUP => Some(libc::SIGHUP),
        russh::Sig::ILL => Some(libc::SIGILL),
        russh::Sig::INT => Some(libc::SIGINT),
        russh::Sig::KILL => Some(libc::SIGKILL),
        russh::Sig::PIPE => Some(libc::SIGPIPE),
        russh::Sig::QUIT => Some(libc::SIGQUIT),
        russh::Sig::SEGV => Some(libc::SIGSEGV),
        russh::Sig::TERM => Some(libc::SIGTERM),
        russh::Sig::USR1 => Some(libc::SIGUSR1),
        russh::Sig::Custom(_) => None,
    }
}

#[cfg(not(unix))]
/// A wrapper around standard stdin for non-Unix platforms.
pub struct AsyncStdin {
    inner: tokio::io::Stdin,
}

#[cfg(not(unix))]
impl AsyncStdin {
    /// Creates a new `AsyncStdin` for non-Unix platforms.
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: tokio::io::stdin(),
        })
    }
}

#[cfg(not(unix))]
impl tokio::io::AsyncRead for AsyncStdin {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

#[cfg(not(unix))]
/// Places the current physical Windows terminal into raw mode and restores it
/// automatically on `Drop`. This captures keystrokes without local processing.
pub struct RawTerminal {
    handle: windows_sys::Win32::Foundation::HANDLE,
    original_mode: u32,
}

#[cfg(not(unix))]
impl fmt::Debug for RawTerminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawTerminal").finish()
    }
}

#[cfg(not(unix))]
impl RawTerminal {
    /// Puts the standard input handle into raw mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal mode cannot be read or if raw
    /// mode cannot be applied to the console handle.
    pub fn new(_fd: i32) -> Result<Self> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Console::*;

        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            if handle == INVALID_HANDLE_VALUE {
                return Err(crate::error::IroshError::Client(
                    crate::error::ClientError::TerminalIo {
                        source: std::io::Error::last_os_error(),
                    },
                ));
            }

            let mut mode = 0;
            if GetConsoleMode(handle, &mut mode) == 0 {
                return Err(crate::error::IroshError::Client(
                    crate::error::ClientError::TerminalIo {
                        source: std::io::Error::last_os_error(),
                    },
                ));
            }

            let original_mode = mode;
            // Disable line input, echo, and signals (processed input).
            // Enable virtual terminal input for VT100/Xterm sequences.
            let raw_mode = (mode
                & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
                | ENABLE_VIRTUAL_TERMINAL_INPUT;

            if SetConsoleMode(handle, raw_mode) == 0 {
                // If VT input fails, try at least without it.
                let raw_mode_basic =
                    mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT);
                if SetConsoleMode(handle, raw_mode_basic) == 0 {
                    return Err(crate::error::IroshError::Client(
                        crate::error::ClientError::TerminalIo {
                            source: std::io::Error::last_os_error(),
                        },
                    ));
                }
            }

            Ok(Self {
                handle,
                original_mode,
            })
        }
    }
}

#[cfg(not(unix))]
impl Drop for RawTerminal {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Console::SetConsoleMode;
        // SAFETY: Best-effort restoration of the original console mode.
        unsafe {
            let _ = SetConsoleMode(self.handle, self.original_mode);
        }
    }
}

#[cfg(not(unix))]
/// Dummy signal mapping for Windows compatibility.
pub fn map_sig(_signal: russh::Sig) -> Option<i32> {
    None
}

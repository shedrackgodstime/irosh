//! Pseudo-terminal (PTY) helper functions and terminal state manipulation.
//!
//! This module handles size negotiation, signals, and raw mode manipulation
//! for interactive SSH sessions.

use std::fmt;

pub use portable_pty::PtySize;

use crate::error::{ClientError, IroshError, Result};

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
/// Dummy implementation of RawTerminal for Windows compatibility.
pub struct RawTerminal;

#[cfg(not(unix))]
impl fmt::Debug for RawTerminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RawTerminal")
    }
}

#[cfg(not(unix))]
impl RawTerminal {
    /// Creates a no-op raw-terminal guard on non-Unix platforms.
    pub fn new(_fd: i32) -> Result<Self> {
        Ok(Self)
    }
}

#[cfg(not(unix))]
/// Dummy signal mapping for Windows compatibility.
pub fn map_sig(_signal: russh::Sig) -> Option<i32> {
    None
}

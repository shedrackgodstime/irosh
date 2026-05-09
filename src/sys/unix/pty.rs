//! Unix implementation of PTY and terminal handling.

use crate::error::{ClientError, IroshError, Result};
pub use portable_pty::PtySize;
use std::fmt;

/// Places the current physical Unix terminal into raw mode and restores it
/// automatically on `Drop`. This captures keystrokes without local processing.
pub struct RawTerminal {
    fd: i32,
    original: libc::termios,
}

impl fmt::Debug for RawTerminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawTerminal").field("fd", &self.fd).finish()
    }
}

impl RawTerminal {
    /// Puts the given file descriptor into raw mode.
    pub fn new(fd: i32) -> Result<Self> {
        // SAFETY: `fd` is supplied by the caller as a valid terminal file descriptor.
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
                let _ = libc::tcsetattr(fd, libc::TCSANOW, &original);
                return Err(IroshError::Client(ClientError::TerminalIo {
                    source: std::io::Error::last_os_error(),
                }));
            }
            Ok(Self { fd, original })
        }
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        unsafe {
            let _ = libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
        }
    }
}

/// Probes the physical terminal size utilizing the `TIOCGWINSZ` syscall.
pub fn current_terminal_size() -> PtySize {
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
    crate::session::pty::default_pty_size()
}

/// Events that can occur on a Unix terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// Raw data received from stdin.
    Data(Vec<u8>),
    /// The terminal window was resized.
    Resize(PtySize),
}

/// A truly non-blocking asynchronous terminal reader for Unix terminals.
pub struct AsyncStdin {
    inner: tokio::io::unix::AsyncFd<std::io::Stdin>,
    original_flags: libc::c_int,
    sigwinch: Option<tokio::signal::unix::Signal>,
}

impl AsyncStdin {
    pub fn new() -> Result<Self> {
        use std::io::IsTerminal;
        use std::os::unix::io::AsRawFd;
        let fd = std::io::stdin().as_raw_fd();

        let sigwinch = if std::io::stdin().is_terminal() {
            Some(tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::window_change(),
            )?)
        } else {
            None
        };

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
                sigwinch,
            })
        }
    }

    /// Polls for the next terminal event (data or resize).
    pub fn poll_next(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<TerminalEvent>> {
        // Priority 1: Check for resize events
        if let Some(sig) = self.sigwinch.as_mut() {
            if let std::task::Poll::Ready(Some(_)) = sig.poll_recv(cx) {
                return std::task::Poll::Ready(Some(
                    TerminalEvent::Resize(current_terminal_size()),
                ));
            }
        }

        // Priority 2: Check for input data
        match self.inner.poll_read_ready(cx) {
            std::task::Poll::Ready(Ok(mut guard)) => {
                let mut buf = [0u8; 4096];
                use std::io::Read;
                match guard.try_io(|inner| inner.get_ref().read(&mut buf)) {
                    Ok(Ok(0)) => std::task::Poll::Ready(None), // EOF
                    Ok(Ok(n)) => {
                        std::task::Poll::Ready(Some(TerminalEvent::Data(buf[..n].to_vec())))
                    }
                    Ok(Err(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::task::Poll::Pending
                    }
                    Ok(Err(_)) => std::task::Poll::Ready(None),
                    Err(_) => std::task::Poll::Pending,
                }
            }
            std::task::Poll::Ready(Err(_)) => std::task::Poll::Ready(None),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

impl Drop for AsyncStdin {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        let fd = std::io::stdin().as_raw_fd();
        unsafe {
            let _ = libc::fcntl(fd, libc::F_SETFL, self.original_flags);
        }
    }
}

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

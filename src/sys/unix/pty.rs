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

/// A truly non-blocking asynchronous stdin reader for Unix terminals.
pub struct AsyncStdin {
    inner: tokio::io::unix::AsyncFd<std::io::Stdin>,
    original_flags: libc::c_int,
}

impl AsyncStdin {
    pub fn new() -> Result<Self> {
        use std::os::unix::io::AsRawFd;
        let fd = std::io::stdin().as_raw_fd();
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
}

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

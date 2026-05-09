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
}

impl AsyncStdin {
    pub fn new() -> Result<Self> {
        use std::os::unix::io::AsRawFd;
        let fd = std::io::stdin().as_raw_fd();

        // SAFETY: Standard fcntl to enable non-blocking I/O on stdin.
        // Original flags are saved so Drop can restore them.
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

    /// Reads the next chunk of raw stdin bytes.
    ///
    /// This uses `readable_mut().await` which is the correct high-level Tokio API.
    /// Unlike the low-level `poll_read_ready`, this properly re-registers the waker
    /// when used inside `tokio::select!` across loop iterations, preventing the
    /// "terminal freeze" caused by dropped wakers on edge-triggered epoll.
    ///
    /// Returns `None` on EOF.
    /// Reads the next terminal event (Data or Resize).
    pub async fn next_event(&mut self) -> Option<TerminalEvent> {
        use std::io::Read;
        loop {
            let mut guard = self.inner.readable_mut().await.ok()?;
            let mut buf = [0u8; 4096];
            match guard.try_io(|inner| inner.get_ref().read(&mut buf)) {
                Ok(Ok(0)) => return None,
                Ok(Ok(n)) => return Some(TerminalEvent::Data(buf[..n].to_vec())),
                Ok(Err(_)) => return None,
                Err(_would_block) => continue,
            }
        }
    }

    /// Reads the next chunk of raw stdin bytes.
    pub async fn read_data(&mut self) -> Option<Vec<u8>> {
        loop {
            match self.next_event().await? {
                TerminalEvent::Data(data) => return Some(data),
                TerminalEvent::Resize(_) => continue,
            }
        }
    }

    /// Low-level poll function. Prefer `read_data()` for use in `tokio::select!`.
    pub fn poll_next(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<TerminalEvent>> {
        match self.inner.poll_read_ready(cx) {
            std::task::Poll::Ready(Ok(mut guard)) => {
                let mut buf = [0u8; 4096];
                use std::io::Read;
                match guard.try_io(|inner| inner.get_ref().read(&mut buf)) {
                    Ok(Ok(0)) => std::task::Poll::Ready(None),
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

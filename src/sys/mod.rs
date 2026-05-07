#[cfg(unix)]
pub mod unix;

#[cfg(windows)]
pub mod windows;

pub mod service;

// Re-export the active platform's implementation
#[cfg(unix)]
pub use unix::pty::{AsyncStdin, RawTerminal, current_terminal_size, map_sig};

#[cfg(windows)]
pub use windows::pty::{AsyncStdin, RawTerminal, current_terminal_size, map_sig};

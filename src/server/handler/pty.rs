//! SSH PTY session plumbing: channel state, resize, and echo.

use std::collections::HashMap;

use bytes::Bytes;
use portable_pty::PtySize;
use russh::{ChannelId, server};
use tracing::debug;

use crate::session::pty::{default_pty_size, pty_size};

use super::ServerHandler;
use super::exec::RunningPty;

#[derive(Default)]
pub(super) struct ChannelState {
    pub(super) pty: PtySpec,
    pub(super) env: HashMap<String, String>,
    pub(super) process: Option<RunningPty>,
}

#[derive(Clone)]
pub(super) struct PtySpec {
    pub(super) term: String,
    pub(super) size: PtySize,
    /// Whether the client negotiated terminal ECHO for this channel.
    /// Defaults to false (no pty requested means no echo).
    /// Only consumed on Windows: ConPTY never echoes input itself, so the
    /// server mirrors keystrokes; Unix kernels echo in the line discipline,
    /// so on non-Windows this field is deliberately never read.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(super) echo: bool,
}

impl Default for PtySpec {
    fn default() -> Self {
        Self {
            term: "xterm-256color".to_string(),
            size: default_pty_size(),
            echo: false,
        }
    }
}

impl ServerHandler {
    pub(super) fn set_channel_pty(
        &self,
        channel: ChannelId,
        term: &str,
        size: PtySize,
        echo: bool,
        session: &mut server::Session,
    ) -> std::result::Result<(), crate::error::IroshError> {
        let mut channels = self.lock_channels();
        let state_entry = channels.entry(channel).or_default();
        state_entry.pty = PtySpec {
            term: term.to_string(),
            size,
            echo,
        };
        session.channel_success(channel)?;
        Ok(())
    }

    pub(super) fn record_env(
        &mut self,
        channel: ChannelId,
        variable_name: &str,
        variable_value: &str,
        session: &mut server::Session,
    ) -> std::result::Result<(), crate::error::IroshError> {
        let mut channels = self.lock_channels();
        let state_entry = channels.entry(channel).or_default();
        state_entry
            .env
            .insert(variable_name.to_string(), variable_value.to_string());
        session.channel_success(channel)?;
        Ok(())
    }

    pub(super) async fn write_channel_data(&self, channel: ChannelId, data: &[u8]) {
        debug!(
            bytes = data.len(),
            ?channel,
            "writing SSH data bytes into PTY channel"
        );
        // Clone the sender out of the lock so the std::sync guard is not held
        // across the await (russh handler futures must be Send).
        let pty_tx = {
            let mut channels = self.lock_channels();
            channels
                .get_mut(&channel)
                .and_then(|state_entry| state_entry.process.as_mut())
                .and_then(|process| process.pty_tx.clone())
        };
        if let Some(pty_tx) = pty_tx {
            let _ = pty_tx.send(Bytes::copy_from_slice(data)).await;
        }
    }

    /// Whether this channel wants server-side input echo (terminal ECHO on a
    /// shell that does not render its own input). Lock is released before any
    /// await performed by the caller.
    pub(super) fn channel_server_echo(&self, channel: ChannelId) -> bool {
        let channels = self.lock_channels();
        channels
            .get(&channel)
            .and_then(|state_entry| state_entry.process.as_ref())
            .is_some_and(|process| process.server_echo)
    }

    pub(super) fn resize_channel(
        &self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        session: &mut server::Session,
    ) -> std::result::Result<(), crate::error::IroshError> {
        let size = pty_size(col_width, row_height, pix_width, pix_height);
        let mut channels = self.lock_channels();
        let state_entry = channels.entry(channel).or_default();
        state_entry.pty.size = size;
        if let Some(process) = state_entry.process.as_ref() {
            // The master may already have been dropped (e.g. on Windows after
            // the child exited). Silently ignore the resize in that case.
            if let Ok(guard) = process.master.lock() {
                if let Some(master) = guard.as_ref() {
                    let _ = master.resize(size);
                }
            }
        }
        session.channel_success(channel)?;
        Ok(())
    }

    pub(super) fn close_channel_writer(&self, channel: ChannelId) {
        let mut channels = self.lock_channels();
        if let Some(state_entry) = channels.get_mut(&channel)
            && let Some(process) = state_entry.process.as_mut()
        {
            process.pty_tx.take();
        }
    }
}

/// Builds the bytes the server echoes back for client input when
/// `server_echo` is on (cmd.exe sessions: ConPTY never echoes by itself).
///
/// Faithful to TTY ECHO semantics rather than a raw copy:
/// - printable ASCII, space, tab and UTF-8 bytes echo as-is;
/// - `\r` echoes as `\n` (the shell's own `\r\n` supplies the carriage
///   return, avoiding a doubled newline);
/// - escape sequences (arrow keys, Delete, `ESC M`, ...) are swallowed:
///   the shell consumes those keys silently and repaints the line itself,
///   so echoing them would print `^[[A` glyph garbage;
/// - other C0 controls and DEL are swallowed: the shell announces their
///   effect through its own output (erase redraws, `^C` on interrupt).
pub(super) fn filter_echo_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b == 0x1b {
            // Skip an escape sequence. A truncated tail is dropped; the
            // remainder of a split sequence is swallowed on the next call
            // the same way.
            i += 1;
            if i >= data.len() {
                break;
            }
            match data[i] {
                b'[' => {
                    // CSI: parameter/intermediate bytes (0x20-0x3F), then one
                    // final byte (0x40-0x7E). Anything else ends the sequence.
                    i += 1;
                    while i < data.len() {
                        let f = data[i];
                        i += 1;
                        if (0x40..=0x7e).contains(&f) || !(0x20..=0x3f).contains(&f) {
                            break;
                        }
                    }
                }
                b']' => {
                    // OSC: consume until BEL or ST.
                    i += 1;
                    while i < data.len() {
                        let f = data[i];
                        i += 1;
                        if f == 0x07 {
                            break;
                        }
                        if f == 0x1b {
                            if i < data.len() && data[i] == b'\\' {
                                i += 1;
                            }
                            break;
                        }
                    }
                }
                b'(' | b')' | b'#' => {
                    // Two-byte sequences: consume the designator too.
                    i += 1;
                    if i < data.len() {
                        i += 1;
                    }
                }
                // Single-char escape (`ESC M`, `ESC =`, ...): consume it.
                _ => {
                    i += 1;
                }
            }
        } else if b == b'\r' {
            out.push(b'\n');
            i += 1;
        } else if b == b'\n' || b == b'\t' || b == b' ' || b.is_ascii_graphic() || b >= 0x80 {
            out.push(b);
            i += 1;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod echo_tests {
    use super::filter_echo_bytes;

    #[test]
    fn printable_passthrough() {
        assert_eq!(filter_echo_bytes(b"echo A~B"), b"echo A~B");
    }

    #[test]
    fn carriage_return_echoes_as_linefeed() {
        assert_eq!(filter_echo_bytes(b"a\rb"), b"a\nb");
    }

    #[test]
    fn arrow_key_sequence_swallowed() {
        assert!(filter_echo_bytes(b"\x1b[A").is_empty());
    }

    #[test]
    fn delete_key_sequence_swallowed_inline() {
        assert_eq!(filter_echo_bytes(b"ab\x1b[3~cd"), b"abcd");
    }

    #[test]
    fn single_char_escape_swallowed() {
        assert!(filter_echo_bytes(b"\x1bM").is_empty());
    }

    #[test]
    fn control_bytes_swallowed() {
        assert!(filter_echo_bytes(b"\x03").is_empty());
        assert!(filter_echo_bytes(b"\x7f").is_empty());
    }

    #[test]
    fn utf8_passthrough() {
        let input = "héllo ~".as_bytes();
        assert_eq!(filter_echo_bytes(input), input);
    }

    #[test]
    fn trailing_lone_esc_swallowed() {
        assert_eq!(filter_echo_bytes(b"a\x1b"), b"a");
    }

    #[test]
    fn multiparam_csi_swallowed() {
        assert!(filter_echo_bytes(b"\x1b[38;5;196m").is_empty());
        assert_eq!(filter_echo_bytes(b"x\x1b[1;31my"), b"xy");
    }

    #[test]
    fn osc_swallowed() {
        assert!(filter_echo_bytes(b"\x1b]0;title\x07").is_empty());
        assert_eq!(filter_echo_bytes(b"a\x1b]0;t\x1b\\b"), b"ab");
    }
}

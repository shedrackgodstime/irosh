//! Escape actions and ANSI/VT state tracking for the input engine.

use crate::commands::connect::prompt::{LocalCommand, parse_local_command};

/// Actions requested by the user via escape sequences (e.g., `~.`) or the local prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EscapeAction {
    /// Disconnect the session immediately (`~.`).
    Disconnect,
    /// Enter the local command prompt (`~C`).
    CommandPrompt,
    /// Show help information (`~?`).
    Help,
    /// Send literal bytes to the remote host without a trailing newline
    /// (`~~` sends a single `~`, so the remote command line can be composed).
    SendLiteral(Vec<u8>),
    /// Execute a command from the local prompt.
    RunLocal(LocalCommand),
    /// Request tab completion.
    RequestCompletion,
}

/// The current mode of the input engine.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum InputMode {
    /// Normal mode: bytes are passed through to the remote host.
    #[default]
    Remote,
    /// Local editing mode (either Escape line or Local prompt).
    LocalEdit,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ControlSequenceState {
    #[default]
    None,
    Escape,
    Csi,
}

/// Tracks ANSI/VT sequences arriving from the **remote** stream.
///
/// This prevents escape sequences emitted by remote shells (e.g. colored
/// prompts like `\x1b[38;5;196m`) from corrupting `local_line_len`, which
/// gates the `~` escape-sequence arm. The old approach used a single `bool`
/// that terminated on any ASCII letter - failing on multi-parameter CSI
/// sequences where the parameter bytes (`38;5;196`) were mistakenly counted
/// as typed characters.
///
/// Sequence types handled:
///   - **CSI** `\x1b[` + parameter bytes (0x30-0x3F) + intermediate bytes
///     (0x20-0x2F) + final byte (0x40-0x7E)
///   - **OSC** `\x1b]` + arbitrary bytes + BEL (`\x07`) or ST (`\x1b\\`)
///   - **Single-char escape** `\x1b` + any other byte (e.g. `\x1bM`)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum RemoteAnsiState {
    #[default]
    None,
    /// Saw `\x1b`; waiting for the sequence introducer.
    AfterEsc,
    /// Inside a CSI sequence (`\x1b[`); consuming until a final byte (0x40-0x7E).
    InCsi,
    /// Inside an OSC sequence (`\x1b]`); consuming until BEL or ST.
    InOsc,
    /// Saw `\x1b` inside an OSC; expecting `\\` to complete the String Terminator.
    InOscEsc,
}

/// Parses an escape buffer (e.g. `b"~."` or `b"~help"`) into an action.
/// Returns `None` if the command is unknown (should be sent to remote).
pub(super) fn parse_escape(buf: &[u8]) -> Option<EscapeAction> {
    // buf starts with `~`; strip it and trim ASCII whitespace.
    let cmd = buf.strip_prefix(b"~").unwrap_or(buf);
    let cmd = trim_bytes(cmd);
    match cmd {
        b"." => Some(EscapeAction::Disconnect),
        b"?" | b"help" => Some(EscapeAction::Help),
        b"C" | b"c" => Some(EscapeAction::CommandPrompt),
        // `~~` forwards a single literal tilde (OpenSSH parity). The editor
        // buffer holds both tildes; only one is sent, with no newline, so
        // the user can keep composing the remote line (e.g. `~/.ssh`).
        b"~" => Some(EscapeAction::SendLiteral(b"~".to_vec())),
        _ => {
            // Try parsing it as a full local command (like `put` or `get`)
            parse_local_command(cmd).map(EscapeAction::RunLocal)
        }
    }
}

pub(super) fn paired_enter_byte(byte: u8) -> Option<u8> {
    match byte {
        b'\r' => Some(b'\n'),
        b'\n' => Some(b'\r'),
        _ => None,
    }
}

fn trim_bytes(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(0, |i| i + 1);
    if start >= end { &[] } else { &s[start..end] }
}

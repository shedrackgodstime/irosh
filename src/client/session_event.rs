//! [`SessionEvent`] — events surfaced from an active SSH session.

use russh::ChannelMsg;

/// Events that can occur during an active SSH session.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionEvent {
    /// Raw data received from remote stdout.
    Data(bytes::Bytes),
    /// Raw data received from remote stderr or other extended streams.
    ExtendedData(bytes::Bytes, u32),
    /// The remote process has exited with the given status code.
    ExitStatus(u32),
    /// The remote process was terminated by a signal.
    ExitSignal {
        /// Signal name (e.g. "TERM", "KILL").
        signal: String,
        /// Whether a core dump was generated.
        core_dumped: bool,
        /// Human-readable error message.
        error_message: String,
        /// Language tag for the error message.
        lang_tag: String,
    },
    /// The remote session has been closed.
    Closed,
    /// An internal SSH message that the library doesn't need to surface.
    Ignore,
}

impl From<ChannelMsg> for SessionEvent {
    fn from(msg: ChannelMsg) -> Self {
        match msg {
            ChannelMsg::Data { data } => Self::Data(data),
            ChannelMsg::ExtendedData { data, ext } => Self::ExtendedData(data, ext),
            ChannelMsg::ExitStatus { exit_status } => Self::ExitStatus(exit_status),
            ChannelMsg::ExitSignal {
                signal_name,
                core_dumped,
                error_message,
                lang_tag,
            } => Self::ExitSignal {
                signal: format!("{signal_name:?}"),
                core_dumped,
                error_message: error_message.clone(),
                lang_tag: lang_tag.clone(),
            },
            ChannelMsg::Close => Self::Closed,
            _ => Self::Ignore,
        }
    }
}

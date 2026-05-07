//! Shared display and formatting utilities for the irosh CLI.
//!
//! These functions are `pub(crate)` and may be used by any command module
//! that needs to render shortened identifiers or ticket strings.

/// Shortens a string to at most `max_len` characters, replacing the middle
/// section with `...` to preserve both the prefix and suffix.
///
/// If `value` is already within `max_len`, it is returned unchanged.
pub(crate) fn shorten_middle(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    let keep = (max_len.saturating_sub(3)) / 2;
    let tail_len = max_len.saturating_sub(keep + 3);
    let head = &value[..keep];
    let tail = &value[value.len() - tail_len..];
    format!("{head}...{tail}")
}

/// Formats a ticket's Node ID as a 16-character shortened display label.
pub(crate) fn ticket_node_label(ticket: &irosh::Ticket) -> String {
    let node_id = ticket.to_addr().id.to_string();
    shorten_middle(&node_id, 16)
}

/// Formats a full ticket string as a 40-character shortened display label.
pub(crate) fn shorten_ticket(ticket: &irosh::Ticket) -> String {
    shorten_middle(&ticket.to_string(), 40)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_middle_returns_value_unchanged_when_within_limit() {
        assert_eq!(shorten_middle("hello", 10), "hello");
        assert_eq!(shorten_middle("hello", 5), "hello");
    }

    #[test]
    fn shorten_middle_truncates_long_strings_with_ellipsis() {
        let result = shorten_middle("abcdefghijklmnopqrstuvwxyz", 10);
        assert_eq!(result.len(), 10);
        assert!(result.contains("..."));
    }

    #[test]
    fn shorten_middle_preserves_head_and_tail() {
        let result = shorten_middle("abcdefghijklmnopqrstuvwxyz", 11);
        assert!(result.starts_with("abc"));
        assert!(result.ends_with("xyz"));
    }
}

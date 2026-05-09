//! Formatting utilities for CLI output.

use irosh::transport::ticket::Ticket;

/// Shortens a ticket string to a more manageable length for display.
pub fn shorten_ticket(ticket: &Ticket) -> String {
    let s = ticket.to_string();
    if s.len() <= 24 {
        return s;
    }
    format!("{}...{}", &s[..12], &s[s.len() - 8..])
}

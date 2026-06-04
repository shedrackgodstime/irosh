//! P2P connection ticket format for peer addressing and discovery.

use iroh::EndpointAddr;
use iroh_tickets::endpoint::EndpointTicket;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Errors that can occur when parsing or manipulating connection tickets.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TicketError {
    /// The ticket string format is invalid.
    #[error("invalid connection ticket format: {0}")]
    InvalidFormat(String),
}

/// A high-level representation of an irosh connection ticket.
///
/// `Ticket` wraps Iroh's official `EndpointTicket` format so the crate can
/// expose a library-owned ticket type while remaining compatible with the Iroh
/// ecosystem.
///
/// The string form of a `Ticket` is intended for out-of-band sharing between a
/// server and a client.
///
/// # Example
///
/// ```no_run
/// # use std::error::Error;
/// use irosh::Ticket;
///
/// # fn main() -> Result<(), Box<dyn Error>> {
/// let ticket: Ticket = "endpoint...".parse()?;
/// let serialized = ticket.to_string();
/// # let _ = serialized;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket {
    /// The wrapped Iroh addressing information.
    pub(crate) inner: EndpointTicket,
}

impl Serialize for Ticket {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Ticket {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Ticket {
    /// Creates a new ticket from an Iroh EndpointAddr.
    #[must_use]
    pub fn new(addr: EndpointAddr) -> Self {
        Self {
            inner: EndpointTicket::new(addr),
        }
    }

    /// Returns a cloned copy of the underlying endpoint address.
    #[must_use]
    pub fn to_addr(&self) -> EndpointAddr {
        self.inner.endpoint_addr().clone()
    }
}

impl From<Ticket> for String {
    fn from(ticket: Ticket) -> Self {
        ticket.to_string()
    }
}

impl fmt::Display for Ticket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Use Iroh's native EndpointTicket string representation.
        write!(f, "{}", self.inner)
    }
}

impl FromStr for Ticket {
    type Err = TicketError;

    /// Parses an irosh ticket from the native endpoint ticket string format.
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let s = s.trim();

        s.parse::<EndpointTicket>()
            .map(|inner| Self { inner })
            .map_err(|_| TicketError::InvalidFormat(format!("could not parse as ticket: {s}")))
    }
}

impl TryFrom<&str> for Ticket {
    type Error = TicketError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl TryFrom<String> for Ticket {
    type Error = TicketError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

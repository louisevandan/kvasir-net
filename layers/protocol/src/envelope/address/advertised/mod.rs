//! What a process decides to call itself.
//!
//! A bound address answers "where do I listen"; it does not answer "what do
//! peers put in an envelope". Those differ the moment a process binds a
//! wildcard, which is the normal case, and the difference is invisible on one
//! machine because loopback and wildcard both resolve to something that
//! answers. Across a fleet they do not, and the symptom is a lap that returns
//! to the wrong machine's own loopback rather than a connection error.
//!
//! So the hint a deployment supplies is parsed rather than pasted. Given
//! `HOST` it takes the bound port; given `HOST:PORT` it takes both. Appending
//! the bound port to a value that already carried one produced
//! `192.168.0.29:52001:52001` — an address that parses, resolves to nothing,
//! and reports itself as ready.

use super::{Address, Scheme};
use crate::ProtocolError;

impl Address {
    /// Resolves what to advertise from an optional hint and the bound socket.
    ///
    /// `None` means "use what I bound", which is correct on one machine and
    /// wrong across a fleet — [`Address::is_local_only`] is what says so.
    pub fn advertised(
        hint: Option<&str>,
        bound_host: &str,
        bound_port: u16,
    ) -> Result<Self, ProtocolError> {
        let Some(hint) = hint.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(Self::tcp(bound_host, bound_port));
        };
        if hint.contains("://") {
            return hint.parse();
        }
        if carries_port(hint) {
            return format!("{}://{hint}", Scheme::Tcp.as_str()).parse();
        }
        if bound_port == 0 {
            return Err(ProtocolError::new("address port must not be zero"));
        }
        Ok(Self::tcp(hint, bound_port))
    }

    /// Whether only the machine holding this address can reach it.
    ///
    /// A wildcard or loopback identity is legitimate for a single-machine run
    /// and never right for a fleet, so it is worth saying out loud rather than
    /// leaving to be diagnosed from a stalled chain.
    pub fn is_local_only(&self) -> bool {
        let host = self.host.trim_start_matches('[').trim_end_matches(']');
        host == "0.0.0.0"
            || host == "::"
            || host == "::1"
            || host.eq_ignore_ascii_case("localhost")
            || host.starts_with("127.")
    }
}

/// Whether a hint already states a port.
///
/// The rule has to survive IPv6, where a bare literal carries colons of its
/// own: `::1` is a host and `[::1]:52001` is a host and a port.
fn carries_port(hint: &str) -> bool {
    match hint.rfind(']') {
        Some(bracket) => hint[bracket + 1..].starts_with(':'),
        None => hint.matches(':').count() == 1,
    }
}

#[cfg(test)]
mod tests;

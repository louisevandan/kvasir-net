//! Where a message is going, stated by the message itself.
//!
//! A relay has to decide "is this mine" without consulting anything, or the
//! decision needs state and stops being a relay. So the address travels in the
//! envelope rather than being looked up from an id — which is also why there
//! is no agent id any more: the address is the identity.
//!
//! Moves when a transport kind is added. `Scheme` exists for that day; nothing
//! else in the envelope needs to know it happened.

use crate::ProtocolError;
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scheme {
    Tcp,
}

impl Scheme {
    fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Address {
    pub scheme: Scheme,
    pub host: String,
    pub port: u16,
}

impl Address {
    pub fn tcp(host: impl Into<String>, port: u16) -> Self {
        Self {
            scheme: Scheme::Tcp,
            host: host.into(),
            port,
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}://{}:{}",
            self.scheme.as_str(),
            self.host,
            self.port
        )
    }
}

impl FromStr for Address {
    type Err = ProtocolError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (scheme, rest) = value
            .split_once("://")
            .ok_or_else(|| ProtocolError::new("address must be scheme://host:port"))?;
        let scheme = match scheme {
            "tcp" => Scheme::Tcp,
            other => return Err(ProtocolError::new(format!("unknown address scheme {other}"))),
        };
        // Split from the right so an IPv6 literal keeps its own colons.
        let (host, port) = rest
            .rsplit_once(':')
            .ok_or_else(|| ProtocolError::new("address must carry a port"))?;
        if host.is_empty() {
            return Err(ProtocolError::new("address must carry a host"));
        }
        let port = port
            .parse::<u16>()
            .map_err(|_| ProtocolError::new("address port must be a number"))?;
        if port == 0 {
            return Err(ProtocolError::new("address port must not be zero"));
        }
        Ok(Self {
            scheme,
            host: host.to_owned(),
            port,
        })
    }
}

#[cfg(test)]
mod tests;

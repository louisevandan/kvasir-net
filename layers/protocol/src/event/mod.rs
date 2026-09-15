//! P4 v2 self-describing events.
//!
//! Relays inspect only the envelope. Payload interpretation belongs to the
//! concrete adapter named by `adapter_kind`.

pub mod hop;
pub mod lifecycle;
mod wire;

use crate::{Address, ProtocolError};

pub use wire::{decode, encode};

/// Read-only agent inspection request. See `docs/event-protocol-v2.md#agent-inspection`.
pub const AGENT_INSPECT_CONTENT_TYPE: &str = "application/vnd.p4.agent.inspect-v1+json";

/// Machine and registered-node snapshot returned for an agent inspection.
pub const AGENT_SNAPSHOT_CONTENT_TYPE: &str = "application/vnd.p4.agent.snapshot-v1+json";

pub type EventId = String;
pub type CorrelationId = String;
pub type NodeId = String;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// Delivery through the reception agent's existing OUTER connection.
/// This contains no dialable OUTER address: `ingress_agent` is the network
/// destination; channel and generation select local delivery at that agent.
pub struct OuterEndpoint {
    pub ingress_agent: Address,
    pub channel: String,
    pub connection_generation: u64,
}

impl OuterEndpoint {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.channel.is_empty() {
            return Err(ProtocolError::new("outer endpoint requires a channel"));
        }
        if self.connection_generation == 0 {
            return Err(ProtocolError::new(
                "outer endpoint requires a connection generation",
            ));
        }
        Ok(())
    }
}

/// Request-owned return identity. Adapters may carry several of these in an
/// opaque aggregate, but must select the relevant owner when emitting a reply.
/// Transport never looks up a request or decodes an adapter payload to route it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReturnContext {
    pub route: OuterEndpoint,
    pub correlation_id: CorrelationId,
    pub deadline_unix_ms: Option<u64>,
}

impl ReturnContext {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.route.validate()?;
        if self.correlation_id.is_empty() || self.deadline_unix_ms == Some(0) {
            return Err(ProtocolError::new(
                "return context requires correlation and a valid deadline",
            ));
        }
        Ok(())
    }

    /// Select one request's context, keeping the actual causal event identity.
    pub fn reply(
        &self,
        base: &Envelope,
        event_id: impl Into<EventId>,
        source: Endpoint,
        class: EventClass,
        sequence: u64,
        content_type: impl Into<String>,
    ) -> Result<Envelope, ProtocolError> {
        self.validate()?;
        let mut envelope = base.next(
            event_id,
            source,
            Endpoint::Outer(self.route.clone()),
            class,
            sequence,
            content_type,
        );
        envelope.return_route = Some(self.route.clone());
        envelope.correlation_id = self.correlation_id.clone();
        envelope.deadline_unix_ms = self.deadline_unix_ms;
        envelope.validate()?;
        Ok(envelope)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Endpoint {
    Agent(Address),
    Node {
        agent: Address,
        node: NodeId,
        generation: u64,
    },
    Outer(OuterEndpoint),
}

impl Endpoint {
    pub fn agent(address: Address) -> Self {
        Self::Agent(address)
    }

    pub fn node(agent: Address, node: impl Into<NodeId>, generation: u64) -> Self {
        Self::Node {
            agent,
            node: node.into(),
            generation,
        }
    }

    pub fn outer(
        ingress_agent: Address,
        channel: impl Into<String>,
        connection_generation: u64,
    ) -> Self {
        Self::Outer(OuterEndpoint {
            ingress_agent,
            channel: channel.into(),
            connection_generation,
        })
    }

    /// Agent that must receive the event next. For OUTER this is always the
    /// ingress agent that owns the connection generation.
    pub fn agent_address(&self) -> &Address {
        match self {
            Self::Agent(address) | Self::Node { agent: address, .. } => address,
            Self::Outer(outer) => &outer.ingress_agent,
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Node { node, .. } if node.is_empty() => {
                Err(ProtocolError::new("node endpoint requires a node id"))
            }
            Self::Node { generation: 0, .. } => {
                Err(ProtocolError::new("node endpoint requires a generation"))
            }
            Self::Outer(outer) => outer.validate(),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventClass {
    Control,
    Data,
    Output,
    Telemetry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub protocol_version: u16,
    pub event_id: EventId,
    pub correlation_id: CorrelationId,
    pub causation_id: Option<EventId>,
    pub source: Endpoint,
    pub target: Endpoint,
    /// Mandatory for valid events. Option preserves P4E3 framing and lets a
    /// refused malformed event retain its exact original representation.
    pub return_route: Option<OuterEndpoint>,
    pub class: EventClass,
    pub sequence: u64,
    pub deadline_unix_ms: Option<u64>,
    pub adapter_kind: Option<String>,
    pub payload_content_type: String,
}

impl Envelope {
    pub const VERSION: u16 = 3;

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.protocol_version != Self::VERSION {
            return Err(ProtocolError::new(format!(
                "event protocol version {} is not {}",
                self.protocol_version,
                Self::VERSION
            )));
        }
        if self.event_id.is_empty()
            || self.correlation_id.is_empty()
            || self.payload_content_type.is_empty()
        {
            return Err(ProtocolError::new(
                "event, correlation and content-type identity are required",
            ));
        }
        if self.causation_id.as_deref().is_some_and(str::is_empty) {
            return Err(ProtocolError::new("causation identity cannot be empty"));
        }
        if self.adapter_kind.as_deref().is_some_and(str::is_empty) {
            return Err(ProtocolError::new("adapter kind cannot be empty"));
        }
        if self.deadline_unix_ms == Some(0) {
            return Err(ProtocolError::new("event deadline cannot be zero"));
        }
        self.source.validate()?;
        self.target.validate()?;
        let route = self
            .return_route
            .as_ref()
            .ok_or_else(|| ProtocolError::new("event requires an explicit OUTER return route"))?;
        route.validate()?;
        for endpoint in [&self.source, &self.target] {
            if let Endpoint::Outer(outer) = endpoint {
                if outer != route {
                    return Err(ProtocolError::new(
                        "OUTER endpoint differs from the event return route",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn return_context(&self) -> Result<ReturnContext, ProtocolError> {
        let context = ReturnContext {
            route: self.return_route.clone().ok_or_else(|| {
                ProtocolError::new("event requires an explicit OUTER return route")
            })?,
            correlation_id: self.correlation_id.clone(),
            deadline_unix_ms: self.deadline_unix_ms,
        };
        context.validate()?;
        Ok(context)
    }

    pub fn reply_target(&self) -> Result<Endpoint, ProtocolError> {
        Ok(Endpoint::Outer(self.return_context()?.route))
    }

    pub fn next(
        &self,
        event_id: impl Into<EventId>,
        source: Endpoint,
        target: Endpoint,
        class: EventClass,
        sequence: u64,
        payload_content_type: impl Into<String>,
    ) -> Self {
        Self {
            protocol_version: Self::VERSION,
            event_id: event_id.into(),
            correlation_id: self.correlation_id.clone(),
            causation_id: Some(self.event_id.clone()),
            source,
            target,
            return_route: self.return_route.clone(),
            class,
            sequence,
            deadline_unix_ms: self.deadline_unix_ms,
            adapter_kind: self.adapter_kind.clone(),
            payload_content_type: payload_content_type.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub envelope: Envelope,
    pub payload: Vec<u8>,
}

impl Event {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.envelope.validate()
    }
}

#[cfg(test)]
mod tests;

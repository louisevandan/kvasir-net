use super::{Endpoint, Envelope, Event, EventClass, OuterEndpoint};
use crate::{Address, ProtocolError};

const MAGIC: [u8; 4] = *b"P4E3";
const MAX_TEXT: usize = 256 * 1024;
const MAX_PAYLOAD: usize = 2 * 1024 * 1024 * 1024;

pub fn encode(event: &Event) -> Result<Vec<u8>, ProtocolError> {
    event.validate()?;
    if event.payload.len() > MAX_PAYLOAD {
        return Err(ProtocolError::new("event payload too large"));
    }
    let mut envelope = Vec::with_capacity(256);
    put_u16(&mut envelope, event.envelope.protocol_version);
    put_text(&mut envelope, &event.envelope.event_id)?;
    put_text(&mut envelope, &event.envelope.correlation_id)?;
    put_optional_text(&mut envelope, event.envelope.causation_id.as_deref())?;
    put_endpoint(&mut envelope, &event.envelope.source)?;
    put_endpoint(&mut envelope, &event.envelope.target)?;
    match &event.envelope.return_route {
        Some(route) => {
            envelope.push(1);
            put_outer(&mut envelope, route)?;
        }
        None => envelope.push(0),
    }
    envelope.push(class_tag(event.envelope.class));
    put_u64(&mut envelope, event.envelope.sequence);
    put_optional_u64(&mut envelope, event.envelope.deadline_unix_ms);
    put_optional_text(&mut envelope, event.envelope.adapter_kind.as_deref())?;
    put_text(&mut envelope, &event.envelope.payload_content_type)?;

    let envelope_len = u32::try_from(envelope.len())
        .map_err(|_| ProtocolError::new("event envelope too large"))?;
    let payload_len = u32::try_from(event.payload.len())
        .map_err(|_| ProtocolError::new("event payload too large"))?;
    let mut result = Vec::with_capacity(12 + envelope.len() + event.payload.len());
    result.extend_from_slice(&MAGIC);
    result.extend_from_slice(&envelope_len.to_le_bytes());
    result.extend_from_slice(&payload_len.to_le_bytes());
    result.extend_from_slice(&envelope);
    result.extend_from_slice(&event.payload);
    Ok(result)
}

pub fn decode(bytes: &[u8]) -> Result<Event, ProtocolError> {
    if bytes.len() < 12 || bytes[..4] != MAGIC {
        return Err(ProtocolError::new("event frame magic mismatch"));
    }
    let envelope_len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let payload_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if envelope_len > MAX_TEXT || payload_len > MAX_PAYLOAD {
        return Err(ProtocolError::new(
            "event frame declares an impossible length",
        ));
    }
    let total = 12usize
        .checked_add(envelope_len)
        .and_then(|value| value.checked_add(payload_len))
        .ok_or_else(|| ProtocolError::new("event frame length overflow"))?;
    if total != bytes.len() {
        return Err(ProtocolError::new("event frame length mismatch"));
    }
    let mut cursor = Cursor::new(&bytes[12..12 + envelope_len]);
    let envelope = Envelope {
        protocol_version: cursor.u16()?,
        event_id: cursor.text()?,
        correlation_id: cursor.text()?,
        causation_id: cursor.optional_text()?,
        source: cursor.endpoint()?,
        target: cursor.endpoint()?,
        return_route: cursor.optional_outer()?,
        class: decode_class(cursor.byte()?)?,
        sequence: cursor.u64()?,
        deadline_unix_ms: cursor.optional_u64()?,
        adapter_kind: cursor.optional_text()?,
        payload_content_type: cursor.text()?,
    };
    if !cursor.finished() {
        return Err(ProtocolError::new("trailing event envelope bytes"));
    }
    let event = Event {
        envelope,
        payload: bytes[12 + envelope_len..].to_vec(),
    };
    event.validate()?;
    Ok(event)
}

fn put_endpoint(out: &mut Vec<u8>, endpoint: &Endpoint) -> Result<(), ProtocolError> {
    match endpoint {
        Endpoint::Agent(address) => {
            out.push(0);
            put_text(out, &address.to_string())?;
        }
        Endpoint::Node {
            agent,
            node,
            generation,
        } => {
            out.push(1);
            put_text(out, &agent.to_string())?;
            put_text(out, node)?;
            put_u64(out, *generation);
        }
        Endpoint::Outer(outer) => {
            out.push(2);
            put_outer(out, outer)?;
        }
    }
    Ok(())
}

fn put_outer(out: &mut Vec<u8>, outer: &OuterEndpoint) -> Result<(), ProtocolError> {
    put_text(out, &outer.ingress_agent.to_string())?;
    put_text(out, &outer.channel)?;
    put_u64(out, outer.connection_generation);
    Ok(())
}

fn put_optional_text(out: &mut Vec<u8>, value: Option<&str>) -> Result<(), ProtocolError> {
    match value {
        Some(value) => {
            out.push(1);
            put_text(out, value)?;
        }
        None => out.push(0),
    }
    Ok(())
}

fn put_optional_u64(out: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            out.push(1);
            put_u64(out, value);
        }
        None => out.push(0),
    }
}

fn put_text(out: &mut Vec<u8>, value: &str) -> Result<(), ProtocolError> {
    if value.len() > MAX_TEXT {
        return Err(ProtocolError::new("event text field too large"));
    }
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn class_tag(class: EventClass) -> u8 {
    match class {
        EventClass::Control => 0,
        EventClass::Data => 1,
        EventClass::Output => 2,
        EventClass::Telemetry => 3,
    }
}

fn decode_class(tag: u8) -> Result<EventClass, ProtocolError> {
    match tag {
        0 => Ok(EventClass::Control),
        1 => Ok(EventClass::Data),
        2 => Ok(EventClass::Output),
        3 => Ok(EventClass::Telemetry),
        _ => Err(ProtocolError::new("unknown event class")),
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn byte(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ProtocolError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn text(&mut self) -> Result<String, ProtocolError> {
        let len = self.u32()? as usize;
        if len > MAX_TEXT {
            return Err(ProtocolError::new("event text field too large"));
        }
        String::from_utf8(self.take(len)?.to_vec())
            .map_err(|_| ProtocolError::new("event text must be UTF-8"))
    }

    fn optional_text(&mut self) -> Result<Option<String>, ProtocolError> {
        match self.byte()? {
            0 => Ok(None),
            1 => self.text().map(Some),
            _ => Err(ProtocolError::new("unknown optional text flag")),
        }
    }

    fn optional_u64(&mut self) -> Result<Option<u64>, ProtocolError> {
        match self.byte()? {
            0 => Ok(None),
            1 => self.u64().map(Some),
            _ => Err(ProtocolError::new("unknown optional number flag")),
        }
    }

    fn endpoint(&mut self) -> Result<Endpoint, ProtocolError> {
        match self.byte()? {
            0 => Ok(Endpoint::Agent(self.address()?)),
            1 => Ok(Endpoint::Node {
                agent: self.address()?,
                node: self.text()?,
                generation: self.u64()?,
            }),
            2 => self.outer().map(Endpoint::Outer),
            _ => Err(ProtocolError::new("unknown endpoint kind")),
        }
    }

    fn optional_outer(&mut self) -> Result<Option<OuterEndpoint>, ProtocolError> {
        match self.byte()? {
            0 => Ok(None),
            1 => self.outer().map(Some),
            _ => Err(ProtocolError::new("unknown return-route flag")),
        }
    }

    fn outer(&mut self) -> Result<OuterEndpoint, ProtocolError> {
        Ok(OuterEndpoint {
            ingress_agent: self.address()?,
            channel: self.text()?,
            connection_generation: self.u64()?,
        })
    }

    fn address(&mut self) -> Result<Address, ProtocolError> {
        self.text()?.parse()
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| ProtocolError::new("event envelope offset overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| ProtocolError::new("truncated event envelope"))?;
        self.offset = end;
        Ok(value)
    }
}

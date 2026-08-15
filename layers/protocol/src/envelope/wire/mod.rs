//! The envelope's bytes.
//!
//! Kept separate from the frame that carries it because a relay reads only
//! this part. The body stays a byte slice the relay never looks at, which is
//! what makes forwarding cost the same whatever the message turns out to be.

use super::{Address, Chain, Envelope, Link, Recipient};
use crate::codec::fields::{Cursor, MAX_ELEMENTS, put_text, put_u32, put_u64};
use crate::{ProtocolError, QueueClass};

const RECIPIENT_AGENT: u8 = 0;
const RECIPIENT_NODE: u8 = 1;
const ABSENT: u8 = 0;
const PRESENT: u8 = 1;

pub(crate) fn encode(envelope: &Envelope) -> Result<Vec<u8>, ProtocolError> {
    let mut bytes = Vec::with_capacity(256);
    put_text(&mut bytes, &envelope.target.to_string())?;
    match &envelope.recipient {
        Recipient::Agent => bytes.push(RECIPIENT_AGENT),
        Recipient::Node(id) => {
            bytes.push(RECIPIENT_NODE);
            put_text(&mut bytes, id)?;
        }
    }
    bytes.push(lane_tag(envelope.lane));
    put_text(&mut bytes, &envelope.route)?;
    put_u64(&mut bytes, envelope.deadline_unix_ms);
    match &envelope.reply_to {
        None => bytes.push(ABSENT),
        Some(address) => {
            bytes.push(PRESENT);
            put_text(&mut bytes, &address.to_string())?;
        }
    }
    match &envelope.chain {
        None => bytes.push(ABSENT),
        Some(chain) => {
            bytes.push(PRESENT);
            if chain.len() > MAX_ELEMENTS {
                return Err(ProtocolError::new("chain names too many nodes"));
            }
            put_u32(&mut bytes, chain.len() as u32);
            put_u32(&mut bytes, chain.position() as u32);
            for link in chain.links() {
                put_text(&mut bytes, &link.address.to_string())?;
                put_text(&mut bytes, &link.node)?;
                put_text(&mut bytes, &link.binding)?;
                put_u64(&mut bytes, link.generation);
            }
        }
    }
    Ok(bytes)
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Envelope, ProtocolError> {
    let mut cursor = Cursor { bytes, offset: 0 };
    let target = address(&mut cursor)?;
    let recipient = match cursor.byte()? {
        RECIPIENT_AGENT => Recipient::Agent,
        RECIPIENT_NODE => Recipient::Node(cursor.text()?),
        _ => return Err(ProtocolError::new("unknown envelope recipient")),
    };
    let lane = lane(cursor.byte()?)?;
    let route = cursor.text()?;
    let deadline_unix_ms = cursor.u64()?;
    let reply_to = match cursor.byte()? {
        ABSENT => None,
        PRESENT => Some(address(&mut cursor)?),
        _ => return Err(ProtocolError::new("unknown envelope reply flag")),
    };
    let chain = match cursor.byte()? {
        ABSENT => None,
        PRESENT => Some(chain(&mut cursor)?),
        _ => return Err(ProtocolError::new("unknown envelope chain flag")),
    };
    if cursor.offset != bytes.len() {
        return Err(ProtocolError::new("trailing envelope bytes"));
    }
    Ok(Envelope {
        target,
        recipient,
        lane,
        route,
        deadline_unix_ms,
        reply_to,
        chain,
    })
}

fn address(cursor: &mut Cursor<'_>) -> Result<Address, ProtocolError> {
    cursor.text()?.parse()
}

fn chain(cursor: &mut Cursor<'_>) -> Result<Chain, ProtocolError> {
    let count = cursor.u32()? as usize;
    if count > MAX_ELEMENTS {
        return Err(ProtocolError::new("chain names too many nodes"));
    }
    let position = cursor.u32()? as usize;
    let mut links = Vec::with_capacity(count);
    for _ in 0..count {
        links.push(Link {
            address: address(cursor)?,
            node: cursor.text()?,
            binding: cursor.text()?,
            generation: cursor.u64()?,
        });
    }
    Chain::at(links, position)
}

fn lane_tag(lane: QueueClass) -> u8 {
    match lane {
        QueueClass::Control => 0,
        QueueClass::Prefill => 1,
        QueueClass::Decode => 2,
        QueueClass::Response => 3,
    }
}

fn lane(tag: u8) -> Result<QueueClass, ProtocolError> {
    Ok(match tag {
        0 => QueueClass::Control,
        1 => QueueClass::Prefill,
        2 => QueueClass::Decode,
        3 => QueueClass::Response,
        _ => return Err(ProtocolError::new("unknown envelope lane")),
    })
}

#[cfg(test)]
mod tests;

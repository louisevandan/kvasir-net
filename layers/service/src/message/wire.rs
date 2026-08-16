//! Body bytes.
//!
//! A tag and length-prefixed fields, nothing else. Bodies are the part a relay
//! copies without reading, so the encoding only has to be cheap to write and
//! unambiguous to read — there is no gain in making it clever.

use super::{Reply, ToAgent, ToNode};

const MAX_TEXT: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Malformed(pub String);

impl std::fmt::Display for Malformed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Malformed {}

type Decoded<T> = Result<T, Malformed>;

// Tags are explicit rather than derived from declaration order, so reordering
// a variant cannot silently change what a peer reads.
const CREATE_NODE: u8 = 1;
const DELETE_NODE: u8 = 2;
const INSPECT: u8 = 3;
const CANCEL: u8 = 4;
const STATUS: u8 = 5;
const LOAD: u8 = 16;
const UNLOAD: u8 = 17;
const EXECUTE: u8 = 18;
const PERSIST: u8 = 19;
const RESTORE: u8 = 20;
const FORK: u8 = 21;
const DISCARD: u8 = 22;
const ACCEPTED: u8 = 32;
const PROGRESS: u8 = 33;
const BOUND: u8 = 34;
const RELEASED: u8 = 35;
const TOKEN: u8 = 36;
const DONE: u8 = 37;
const FAILED: u8 = 38;
const MACHINE: u8 = 39;
const STATUS_REPLY: u8 = 40;
const CACHED: u8 = 41;

pub fn encode_to_agent(message: &ToAgent) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    match message {
        ToAgent::CreateNode { node, adapter } => {
            out.push(CREATE_NODE);
            text(&mut out, node);
            text(&mut out, adapter);
        }
        ToAgent::DeleteNode { node } => {
            out.push(DELETE_NODE);
            text(&mut out, node);
        }
        ToAgent::Inspect => out.push(INSPECT),
        ToAgent::Cancel { route } => {
            out.push(CANCEL);
            text(&mut out, route);
        }
        ToAgent::Status => out.push(STATUS),
    }
    out
}

pub fn decode_to_agent(bytes: &[u8]) -> Decoded<ToAgent> {
    let mut cursor = Cursor::new(bytes);
    let message = match cursor.tag()? {
        CREATE_NODE => ToAgent::CreateNode {
            node: cursor.text()?,
            adapter: cursor.text()?,
        },
        DELETE_NODE => ToAgent::DeleteNode {
            node: cursor.text()?,
        },
        INSPECT => ToAgent::Inspect,
        CANCEL => ToAgent::Cancel {
            route: cursor.text()?,
        },
        STATUS => ToAgent::Status,
        tag => return Err(Malformed(format!("unknown agent message {tag}"))),
    };
    cursor.finished()?;
    Ok(message)
}

pub fn encode_to_node(message: &ToNode) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    match message {
        ToNode::Load {
            plan,
            artifact,
            ceiling,
        } => {
            out.push(LOAD);
            text(&mut out, plan);
            text(&mut out, artifact);
            number(&mut out, *ceiling);
        }
        ToNode::Unload => out.push(UNLOAD),
        ToNode::Execute {
            prompt,
            max_tokens,
            options,
        } => {
            out.push(EXECUTE);
            text(&mut out, prompt);
            number(&mut out, *max_tokens);
            text(&mut out, options);
        }
        ToNode::Persist { sequence } => {
            out.push(PERSIST);
            text(&mut out, sequence);
        }
        ToNode::Restore { sequence } => {
            out.push(RESTORE);
            text(&mut out, sequence);
        }
        ToNode::Fork { sequence, into } => {
            out.push(FORK);
            text(&mut out, sequence);
            text(&mut out, into);
        }
        ToNode::Discard { sequence } => {
            out.push(DISCARD);
            text(&mut out, sequence);
        }
    }
    out
}

pub fn decode_to_node(bytes: &[u8]) -> Decoded<ToNode> {
    let mut cursor = Cursor::new(bytes);
    let message = match cursor.tag()? {
        LOAD => ToNode::Load {
            plan: cursor.text()?,
            artifact: cursor.text()?,
            ceiling: cursor.number()?,
        },
        UNLOAD => ToNode::Unload,
        EXECUTE => ToNode::Execute {
            prompt: cursor.text()?,
            max_tokens: cursor.number()?,
            options: cursor.text()?,
        },
        PERSIST => ToNode::Persist {
            sequence: cursor.text()?,
        },
        RESTORE => ToNode::Restore {
            sequence: cursor.text()?,
        },
        FORK => ToNode::Fork {
            sequence: cursor.text()?,
            into: cursor.text()?,
        },
        DISCARD => ToNode::Discard {
            sequence: cursor.text()?,
        },
        tag => return Err(Malformed(format!("unknown node message {tag}"))),
    };
    cursor.finished()?;
    Ok(message)
}

pub fn encode_reply(reply: &Reply) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    match reply {
        Reply::Accepted { detail } => {
            out.push(ACCEPTED);
            text(&mut out, detail);
        }
        Reply::Progress { stage, percent } => {
            out.push(PROGRESS);
            number(&mut out, *stage);
            number(&mut out, *percent);
        }
        Reply::Bound { generation } => {
            out.push(BOUND);
            out.extend_from_slice(&generation.to_le_bytes());
        }
        Reply::Released => out.push(RELEASED),
        Reply::Token { index, text: value } => {
            out.push(TOKEN);
            number(&mut out, *index);
            text(&mut out, value);
        }
        Reply::Done { reason, generated } => {
            out.push(DONE);
            text(&mut out, reason);
            number(&mut out, *generated);
        }
        Reply::Failed { detail } => {
            out.push(FAILED);
            text(&mut out, detail);
        }
        Reply::Machine { snapshot } => {
            out.push(MACHINE);
            text(&mut out, snapshot);
        }
        Reply::Status { snapshot } => {
            out.push(STATUS_REPLY);
            text(&mut out, snapshot);
        }
        Reply::Cached {
            sequence,
            bytes,
            detail,
        } => {
            out.push(CACHED);
            text(&mut out, sequence);
            wide(&mut out, *bytes);
            text(&mut out, detail);
        }
    }
    out
}

pub fn decode_reply(bytes: &[u8]) -> Decoded<Reply> {
    let mut cursor = Cursor::new(bytes);
    let reply = match cursor.tag()? {
        ACCEPTED => Reply::Accepted {
            detail: cursor.text()?,
        },
        PROGRESS => Reply::Progress {
            stage: cursor.number()?,
            percent: cursor.number()?,
        },
        BOUND => Reply::Bound {
            generation: cursor.wide()?,
        },
        RELEASED => Reply::Released,
        TOKEN => Reply::Token {
            index: cursor.number()?,
            text: cursor.text()?,
        },
        DONE => Reply::Done {
            reason: cursor.text()?,
            generated: cursor.number()?,
        },
        FAILED => Reply::Failed {
            detail: cursor.text()?,
        },
        MACHINE => Reply::Machine {
            snapshot: cursor.text()?,
        },
        STATUS_REPLY => Reply::Status {
            snapshot: cursor.text()?,
        },
        CACHED => Reply::Cached {
            sequence: cursor.text()?,
            bytes: cursor.wide()?,
            detail: cursor.text()?,
        },
        tag => return Err(Malformed(format!("unknown reply {tag}"))),
    };
    cursor.finished()?;
    Ok(reply)
}

fn text(out: &mut Vec<u8>, value: &str) {
    number(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}

fn wide(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn number(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn tag(&mut self) -> Decoded<u8> {
        let value = *self
            .bytes
            .get(self.offset)
            .ok_or_else(|| Malformed("body is empty".into()))?;
        self.offset += 1;
        Ok(value)
    }

    fn number(&mut self) -> Decoded<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn wide(&mut self) -> Decoded<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }

    fn text(&mut self) -> Decoded<String> {
        let length = self.number()? as usize;
        if length > MAX_TEXT {
            return Err(Malformed("text field too large".into()));
        }
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| Malformed("text must be UTF-8".into()))
    }

    fn take(&mut self, length: usize) -> Decoded<&'a [u8]> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| Malformed("length overflow".into()))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| Malformed("body ends early".into()))?;
        self.offset = end;
        Ok(bytes)
    }

    /// Trailing bytes mean the sender and the reader disagree about the shape,
    /// which is worth failing on rather than ignoring.
    fn finished(&self) -> Decoded<()> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(Malformed("trailing body bytes".into()))
        }
    }
}

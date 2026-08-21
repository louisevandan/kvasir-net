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
const INSPECT_MODEL: u8 = 6;
const ACKNOWLEDGE: u8 = 7;
const LOAD: u8 = 16;
const UNLOAD: u8 = 17;
const EXECUTE: u8 = 18;
const CONTINUE: u8 = 28;
const PERSIST: u8 = 19;
const RESTORE: u8 = 20;
const FORK: u8 = 21;
const DISCARD: u8 = 22;
const PREPARE_PERSIST: u8 = 23;
const PREPARE_RESTORE: u8 = 24;
const PREPARE_DISCARD: u8 = 25;
const COMMIT: u8 = 26;
const ABORT: u8 = 27;
const RECONCILE: u8 = 29;
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
const MODEL: u8 = 42;
const STATUS_SNAPSHOT: u8 = 43;
const CACHE_FAILED: u8 = 44;
const CACHE_STATUS: u8 = 45;

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
        ToAgent::InspectModel { artifact, adapter } => {
            out.push(INSPECT_MODEL);
            text(&mut out, artifact);
            text(&mut out, adapter);
        }
        ToAgent::Cancel {
            route,
            request_id,
            stream_id,
            return_channel,
            generation,
        } => {
            out.push(CANCEL);
            text(&mut out, route);
            text(&mut out, request_id);
            text(&mut out, stream_id);
            text(&mut out, return_channel);
            wide(&mut out, *generation);
        }
        ToAgent::Status => out.push(STATUS),
        ToAgent::Acknowledge {
            return_channel,
            stream_id,
            event_seq,
        } => {
            out.push(ACKNOWLEDGE);
            text(&mut out, return_channel);
            text(&mut out, stream_id);
            wide(&mut out, *event_seq);
        }
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
        INSPECT_MODEL => ToAgent::InspectModel {
            artifact: cursor.text()?,
            adapter: cursor.text()?,
        },
        CANCEL => ToAgent::Cancel {
            route: cursor.text()?,
            request_id: cursor.text()?,
            stream_id: cursor.text()?,
            return_channel: cursor.text()?,
            generation: cursor.wide()?,
        },
        STATUS => ToAgent::Status,
        ACKNOWLEDGE => ToAgent::Acknowledge {
            return_channel: cursor.text()?,
            stream_id: cursor.text()?,
            event_seq: cursor.wide()?,
        },
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
            capability_snapshot_id,
            capability_expires_at,
        } => {
            out.push(LOAD);
            text(&mut out, plan);
            text(&mut out, artifact);
            number(&mut out, *ceiling);
            text(&mut out, capability_snapshot_id);
            wide(&mut out, *capability_expires_at);
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
        ToNode::Continue {
            remaining,
            emitted,
            options,
            state,
        } => {
            out.push(CONTINUE);
            number(&mut out, *remaining);
            number(&mut out, *emitted);
            text(&mut out, options);
            blob(&mut out, state);
        }
        ToNode::Persist { sequence } => {
            out.push(PERSIST);
            text(&mut out, sequence);
        }
        ToNode::PreparePersist { sequence } => {
            out.push(PREPARE_PERSIST);
            text(&mut out, sequence);
        }
        ToNode::Restore { sequence } => {
            out.push(RESTORE);
            text(&mut out, sequence);
        }
        ToNode::PrepareRestore { sequence } => {
            out.push(PREPARE_RESTORE);
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
        ToNode::PrepareDiscard { sequence } => {
            out.push(PREPARE_DISCARD);
            text(&mut out, sequence);
        }
        ToNode::Commit { sequence } => {
            out.push(COMMIT);
            text(&mut out, sequence);
        }
        ToNode::Abort { sequence } => {
            out.push(ABORT);
            text(&mut out, sequence);
        }
        ToNode::Reconcile { sequence } => {
            out.push(RECONCILE);
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
            capability_snapshot_id: cursor.text()?,
            capability_expires_at: cursor.wide()?,
        },
        UNLOAD => ToNode::Unload,
        EXECUTE => ToNode::Execute {
            prompt: cursor.text()?,
            max_tokens: cursor.number()?,
            options: cursor.text()?,
        },
        CONTINUE => ToNode::Continue {
            remaining: cursor.number()?,
            emitted: cursor.number()?,
            options: cursor.text()?,
            state: cursor.blob()?,
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
        PREPARE_PERSIST => ToNode::PreparePersist {
            sequence: cursor.text()?,
        },
        PREPARE_RESTORE => ToNode::PrepareRestore {
            sequence: cursor.text()?,
        },
        PREPARE_DISCARD => ToNode::PrepareDiscard {
            sequence: cursor.text()?,
        },
        COMMIT => ToNode::Commit {
            sequence: cursor.text()?,
        },
        ABORT => ToNode::Abort {
            sequence: cursor.text()?,
        },

        RECONCILE => ToNode::Reconcile {
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
        Reply::Done {
            reason,
            generated,
            final_token,
        } => {
            out.push(DONE);
            text(&mut out, reason);
            number(&mut out, *generated);
            match final_token {
                Some((index, value)) => {
                    out.push(1);
                    number(&mut out, *index);
                    text(&mut out, value);
                }
                None => out.push(0),
            }
        }
        Reply::Failed { detail } => {
            out.push(FAILED);
            text(&mut out, detail);
        }
        Reply::CacheFailed {
            deployment,
            stage_id,
            generation,
            operation_id,
            sequence,
            detail,
        } => {
            out.push(CACHE_FAILED);
            text(&mut out, deployment);
            text(&mut out, stage_id);
            wide(&mut out, *generation);
            text(&mut out, operation_id);
            text(&mut out, sequence);
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
        Reply::StatusSnapshot { snapshot } => {
            out.push(STATUS_SNAPSHOT);
            encode_status_snapshot(&mut out, snapshot);
        }
        Reply::Model {
            artifact,
            adapter,
            profile,
            capability_snapshot_id,
            generated_at,
            expires_at,
        } => {
            out.push(MODEL);
            text(&mut out, artifact);
            text(&mut out, adapter);
            text(&mut out, profile);
            text(&mut out, capability_snapshot_id);
            wide(&mut out, *generated_at);
            wide(&mut out, *expires_at);
        }
        Reply::Cached {
            deployment,
            stage_id,
            generation,
            operation_id,
            sequence,
            bytes,
            detail,
        } => {
            out.push(CACHED);
            text(&mut out, deployment);
            text(&mut out, stage_id);
            wide(&mut out, *generation);
            text(&mut out, operation_id);
            text(&mut out, sequence);
            wide(&mut out, *bytes);
            text(&mut out, detail);
        }
        Reply::CacheStatus {
            deployment,
            stage_id,
            generation,
            operation_id,
            sequence,
            state,
            bytes,
            detail,
        } => {
            out.push(CACHE_STATUS);
            text(&mut out, deployment);
            text(&mut out, stage_id);
            wide(&mut out, *generation);
            text(&mut out, operation_id);
            text(&mut out, sequence);
            text(&mut out, state);
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
        DONE => {
            let reason = cursor.text()?;
            let generated = cursor.number()?;
            let final_token = if cursor.remaining() == 0 {
                None
            } else {
                match cursor.byte()? {
                    0 => None,
                    1 => Some((cursor.number()?, cursor.text()?)),
                    _ => return Err(Malformed("invalid final token flag".into())),
                }
            };
            Reply::Done {
                reason,
                generated,
                final_token,
            }
        }
        FAILED => Reply::Failed {
            detail: cursor.text()?,
        },
        CACHE_FAILED => Reply::CacheFailed {
            deployment: cursor.text()?,
            stage_id: cursor.text()?,
            generation: cursor.wide()?,
            operation_id: cursor.text()?,
            sequence: cursor.text()?,
            detail: cursor.text()?,
        },
        MACHINE => Reply::Machine {
            snapshot: cursor.text()?,
        },
        STATUS_REPLY => Reply::Status {
            snapshot: cursor.text()?,
        },
        STATUS_SNAPSHOT => Reply::StatusSnapshot {
            snapshot: decode_status_snapshot(&mut cursor)?,
        },
        MODEL => Reply::Model {
            artifact: cursor.text()?,
            adapter: cursor.text()?,
            profile: cursor.text()?,
            capability_snapshot_id: cursor.text()?,
            generated_at: cursor.wide()?,
            expires_at: cursor.wide()?,
        },
        CACHED => Reply::Cached {
            deployment: cursor.text()?,
            stage_id: cursor.text()?,
            generation: cursor.wide()?,
            operation_id: cursor.text()?,
            sequence: cursor.text()?,
            bytes: cursor.wide()?,
            detail: cursor.text()?,
        },
        CACHE_STATUS => Reply::CacheStatus {
            deployment: cursor.text()?,
            stage_id: cursor.text()?,
            generation: cursor.wide()?,
            operation_id: cursor.text()?,
            sequence: cursor.text()?,
            state: cursor.text()?,
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

/// Opaque adapter state. Unlike `text` this has no length ceiling of its own:
/// what a backend needs to carry a session is the backend's business, and the
/// frame's own limit is the only bound that means anything here.
fn blob(out: &mut Vec<u8>, value: &[u8]) {
    number(out, value.len() as u32);
    out.extend_from_slice(value);
}

fn wide(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn number(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn encode_status_snapshot(out: &mut Vec<u8>, snapshot: &crate::status::StatusSnapshot) {
    number(out, u32::from(snapshot.schema));
    wide(out, snapshot.snapshot_seq);
    wide(out, snapshot.generated_at_unix_ms);
    text(out, &snapshot.address);
    for value in [
        snapshot.traffic.forwarded,
        snapshot.traffic.consumed,
        snapshot.traffic.to_nodes,
        snapshot.traffic.unrouted,
        snapshot.traffic.refused,
        snapshot.traffic.emergency_lost,
        snapshot.lanes.control,
        snapshot.lanes.prefill,
        snapshot.lanes.decode,
        snapshot.lanes.response,
        snapshot.peers,
        snapshot.continuations,
        snapshot.subscription_pending,
        snapshot.subscription_unacked,
        snapshot.subscription_dropped,
    ] {
        wide(out, value as u64);
    }
    if snapshot.schema >= 2 {
        wide(out, snapshot.subscription_ack_rejected as u64);
    }
    number(out, snapshot.nodes.len() as u32);
    for node in &snapshot.nodes {
        text(out, &node.node);
        wide(out, node.depth as u64);
        wide(out, node.running as u64);
        text(out, &node.backend);
        if snapshot.schema >= 6 {
            wide(out, node.outbox_lost as u64);
        }
        number(out, node.waiting.len() as u32);
        for route in &node.waiting {
            text(out, route);
        }
        if snapshot.schema >= 3 {
            number(out, node.waiting_requests.len() as u32);
            for request in &node.waiting_requests {
                text(out, &request.route);
                text(out, &request.request_id);
                text(out, &request.stream_id);
                out.push(status_lane_tag(request.lane));
                wide(out, request.deadline_unix_ms);
            }
        }
        if snapshot.schema >= 4 {
            match &node.active_hop {
                Some(active) => {
                    out.push(1);
                    wide(out, active.id);
                    out.push(active_hop_lane_tag(active.lane));
                    number(out, active.requests.len() as u32);
                    for request in &active.requests {
                        encode_status_request(out, request);
                    }
                    if snapshot.schema >= 5 {
                        out.push(u8::from(active.timed_out));
                    }
                }
                None => out.push(0),
            }
        }
    }
}

fn decode_status_snapshot(cursor: &mut Cursor<'_>) -> Decoded<crate::status::StatusSnapshot> {
    let schema = cursor.number()? as u16;
    if !(crate::status::MIN_SUPPORTED_SCHEMA..=crate::status::MAX_SUPPORTED_SCHEMA)
        .contains(&schema)
    {
        return Err(Malformed(format!("unsupported status schema {schema}")));
    }
    let snapshot_seq = cursor.wide()?;
    let generated_at_unix_ms = cursor.wide()?;
    let address = cursor.text()?;
    let mut values = [0u64; 16];
    for value in &mut values[..15] {
        *value = cursor.wide()?;
    }
    if schema >= 2 {
        values[15] = cursor.wide()?;
    }
    let node_count = cursor.number()? as usize;
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        let node = cursor.text()?;
        let depth = cursor.wide()? as usize;
        let running = cursor.wide()? as usize;
        let backend = cursor.text()?;
        let outbox_lost = if schema >= 6 {
            cursor.wide()? as usize
        } else {
            0
        };
        let waiting_count = cursor.number()? as usize;
        let mut waiting = Vec::with_capacity(waiting_count);
        for _ in 0..waiting_count {
            waiting.push(cursor.text()?);
        }
        let mut waiting_requests = Vec::new();
        if schema >= 3 {
            let request_count = cursor.number()? as usize;
            waiting_requests.reserve(request_count);
            for _ in 0..request_count {
                waiting_requests.push(crate::status::RequestSnapshot {
                    route: cursor.text()?,
                    request_id: cursor.text()?,
                    stream_id: cursor.text()?,
                    lane: status_lane(cursor.tag()?)?,
                    deadline_unix_ms: cursor.wide()?,
                });
            }
        }
        let active_hop = if schema >= 4 {
            match cursor.tag()? {
                0 => None,
                1 => {
                    let id = cursor.wide()?;
                    let lane = active_hop_lane(cursor.tag()?)?;
                    let count = cursor.number()? as usize;
                    let mut requests = Vec::with_capacity(count);
                    for _ in 0..count {
                        requests.push(decode_status_request(cursor)?);
                    }
                    let timed_out = if schema >= 5 {
                        match cursor.tag()? {
                            0 => false,
                            1 => true,
                            other => {
                                return Err(Malformed(format!(
                                    "invalid active hop timeout marker {other}"
                                )));
                            }
                        }
                    } else {
                        false
                    };
                    Some(crate::status::ActiveHopSnapshot {
                        id,
                        lane,
                        timed_out,
                        requests,
                    })
                }
                other => return Err(Malformed(format!("invalid active hop marker {other}"))),
            }
        } else {
            None
        };
        nodes.push(crate::status::NodeSnapshot {
            node,
            depth,
            running,
            outbox_lost,
            waiting,
            backend,
            waiting_requests,
            active_hop,
        });
    }
    Ok(crate::status::StatusSnapshot {
        schema,
        snapshot_seq,
        generated_at_unix_ms,
        address,
        traffic: crate::status::TrafficSnapshot {
            forwarded: values[0] as usize,
            consumed: values[1] as usize,
            to_nodes: values[2] as usize,
            unrouted: values[3] as usize,
            refused: values[4] as usize,
            emergency_lost: values[5] as usize,
        },
        lanes: crate::status::LaneSnapshot {
            control: values[6] as usize,
            prefill: values[7] as usize,
            decode: values[8] as usize,
            response: values[9] as usize,
        },
        peers: values[10] as usize,
        continuations: values[11] as usize,
        subscription_pending: values[12] as usize,
        subscription_unacked: values[13] as usize,
        subscription_dropped: values[14] as usize,
        subscription_ack_rejected: values[15] as usize,
        nodes,
    })
}

fn status_lane_tag(lane: p4_protocol::QueueClass) -> u8 {
    match lane {
        p4_protocol::QueueClass::Control => 0,
        p4_protocol::QueueClass::Prefill => 1,
        p4_protocol::QueueClass::Decode => 2,
        p4_protocol::QueueClass::Response => 3,
    }
}

fn status_lane(tag: u8) -> Decoded<p4_protocol::QueueClass> {
    match tag {
        0 => Ok(p4_protocol::QueueClass::Control),
        1 => Ok(p4_protocol::QueueClass::Prefill),
        2 => Ok(p4_protocol::QueueClass::Decode),
        3 => Ok(p4_protocol::QueueClass::Response),
        other => Err(Malformed(format!("unknown status lane {other}"))),
    }
}

/// Codec for `ActiveHopSnapshot::lane`, kept deliberately separate from
/// `status_lane_tag`/`status_lane` above.
///
/// This field's byte has meant `Prefill -> 0, Decode -> 1` since before
/// `QueueClass` existed, and a shipped schema 6 peer still expects exactly
/// that. `status_lane_tag` is correct where it is used — `RequestSnapshot`
/// really is a four-valued field and has always used all four tags — but
/// reusing it here would widen this field's wire domain out from under any
/// peer that has already shipped against it. See `ActiveHopLane`'s doc
/// comment in `crate::status` for the full story.
fn active_hop_lane_tag(lane: crate::status::ActiveHopLane) -> u8 {
    match lane {
        crate::status::ActiveHopLane::Prefill => 0,
        crate::status::ActiveHopLane::Decode => 1,
    }
}

fn active_hop_lane(tag: u8) -> Decoded<crate::status::ActiveHopLane> {
    match tag {
        0 => Ok(crate::status::ActiveHopLane::Prefill),
        1 => Ok(crate::status::ActiveHopLane::Decode),
        other => Err(Malformed(format!("unknown active hop lane {other}"))),
    }
}

fn encode_status_request(out: &mut Vec<u8>, request: &crate::status::RequestSnapshot) {
    text(out, &request.route);
    text(out, &request.request_id);
    text(out, &request.stream_id);
    out.push(status_lane_tag(request.lane));
    wide(out, request.deadline_unix_ms);
}

fn decode_status_request(cursor: &mut Cursor<'_>) -> Decoded<crate::status::RequestSnapshot> {
    Ok(crate::status::RequestSnapshot {
        route: cursor.text()?,
        request_id: cursor.text()?,
        stream_id: cursor.text()?,
        lane: status_lane(cursor.tag()?)?,
        deadline_unix_ms: cursor.wide()?,
    })
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

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn byte(&mut self) -> Decoded<u8> {
        Ok(self.take(1)?[0])
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

    fn blob(&mut self) -> Decoded<Vec<u8>> {
        let length = self.number()? as usize;
        Ok(self.take(length)?.to_vec())
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

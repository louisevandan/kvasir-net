//! Reply-shaped encoding: everything `Bodies` writes back, as opposed to
//! `mod.rs`'s inbound half that reads an incoming body into work.
//!
//! Split out for the same reason `node::outcome::close` is kept apart from
//! `node::outcome::next`: the inbound half changes when P4's body catalog
//! for *incoming* work changes, and this changes independently when what a
//! node writes back does -- a new `Reply` variant, or a new
//! `SessionClose`/`SessionClosed` shape, touches only this file and not the
//! one that decides what a frame means on the way in.
//!
//! Free functions rather than a second `impl Payload for Bodies`: Rust
//! allows only one implementation of a given trait for a given type in a
//! crate, so `mod.rs`'s impl block still has to name every method -- each
//! body here is just a one-line call into this module.

use crate::message::wire::{encode_reply, encode_to_node};
use crate::message::{Reply, ToNode};
use p4_protocol::frame::Frame;

pub(super) fn token(text: &str, index: u32) -> Vec<u8> {
    encode_reply(&Reply::Token {
        index,
        text: text.to_owned(),
    })
}

pub(super) fn finished(reason: &str, generated: u32) -> Vec<u8> {
    encode_reply(&Reply::Done {
        reason: reason.to_owned(),
        generated,
        final_token: None,
    })
}

pub(super) fn finished_with_token(reason: &str, generated: u32, index: u32, text: &str) -> Vec<u8> {
    encode_reply(&Reply::Done {
        reason: reason.to_owned(),
        generated,
        final_token: Some((index, text.to_owned())),
    })
}

pub(super) fn failure(detail: &str) -> Vec<u8> {
    encode_reply(&Reply::Failed {
        detail: detail.to_owned(),
    })
}

/// The identity-bearing half of a cache failure, once the caller has already
/// found both the current chain link and the cache work this frame named --
/// both require `self.lifecycle(frame)`, which only `mod.rs`'s trait method
/// can call, so this only does the part that needs nothing but what was
/// already found.
pub(super) fn cache_failure(
    link_binding: &str,
    link_node: &str,
    link_generation: u64,
    request_id: &str,
    subject: &str,
    detail: &str,
) -> Vec<u8> {
    encode_reply(&Reply::CacheFailed {
        deployment: link_binding.to_owned(),
        stage_id: link_node.to_owned(),
        generation: link_generation,
        operation_id: request_id.to_owned(),
        sequence: subject.to_owned(),
        detail: detail.to_owned(),
    })
}

pub(super) fn progress(stage: u32, percent: u32) -> Vec<u8> {
    encode_reply(&Reply::Progress { stage, percent })
}

pub(super) fn bound(generation: u64) -> Vec<u8> {
    encode_reply(&Reply::Bound { generation })
}

pub(super) fn released() -> Vec<u8> {
    encode_reply(&Reply::Released)
}

/// Not a reply -- this is what the core sends *to* another node of the same
/// chain when a sequence is over, so it goes through `ToNode` rather than
/// `Reply`. See `ToNode::SessionClose`.
pub(super) fn close(sequence: &str, close_id: u64, session_epoch: u64) -> Vec<u8> {
    encode_to_node(&ToNode::SessionClose {
        sequence: sequence.to_owned(),
        close_id,
        session_epoch,
    })
}

/// Also not a reply, for the same reason `close` is not: it targets the node
/// that sent the close, not OUTER, so it goes through `ToNode` rather than
/// `Reply`. See `ToNode::SessionClosed`.
pub(super) fn session_closed(sequence: &str, close_id: u64) -> Vec<u8> {
    encode_to_node(&ToNode::SessionClosed {
        sequence: sequence.to_owned(),
        close_id,
    })
}

/// Reads a frame's body back as the `(sequence, close_id)` a `close` call
/// produced. See that trait method's own doc for why `close_id` alone is
/// what a resend is recognized by.
pub(super) fn close_identity(frame: &Frame) -> Option<(String, u64)> {
    match super::decode_to_node(&frame.body).ok()? {
        ToNode::SessionClose {
            sequence, close_id, ..
        } => Some((sequence, close_id)),
        _ => None,
    }
}

/// Reads a frame's `session_epoch`, whichever hop-shaped or `SessionClose`
/// variant carries one. See `Payload::session_epoch`'s own doc for what
/// this identifies and why it is checked apart from `close_identity`.
pub(super) fn session_epoch(frame: &Frame) -> Option<u64> {
    match super::decode_to_node(&frame.body).ok()? {
        ToNode::Execute { session_epoch, .. } => Some(session_epoch),
        ToNode::Continue { session_epoch, .. } => Some(session_epoch),
        ToNode::SessionClose { session_epoch, .. } => Some(session_epoch),
        _ => None,
    }
}

/// Reads a frame's body back as the `(sequence, close_id)` a
/// `session_closed` call produced. Checked first on every frame a node
/// receives -- see that trait method's own doc.
pub(super) fn session_closed_ack(frame: &Frame) -> Option<(String, u64)> {
    match super::decode_to_node(&frame.body).ok()? {
        ToNode::SessionClosed { sequence, close_id } => Some((sequence, close_id)),
        _ => None,
    }
}

pub(super) fn cached(
    deployment: &str,
    stage_id: &str,
    generation: u64,
    operation_id: &str,
    sequence: &str,
    bytes: u64,
    detail: &str,
) -> Vec<u8> {
    encode_reply(&Reply::Cached {
        deployment: deployment.to_owned(),
        stage_id: stage_id.to_owned(),
        generation,
        operation_id: operation_id.to_owned(),
        sequence: sequence.to_owned(),
        bytes,
        detail: detail.to_owned(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn cache_status(
    deployment: &str,
    stage_id: &str,
    generation: u64,
    operation_id: &str,
    sequence: &str,
    state: &str,
    bytes: u64,
    detail: &str,
) -> Vec<u8> {
    encode_reply(&Reply::CacheStatus {
        deployment: deployment.to_owned(),
        stage_id: stage_id.to_owned(),
        generation,
        operation_id: operation_id.to_owned(),
        sequence: sequence.to_owned(),
        state: state.to_owned(),
        bytes,
        detail: detail.to_owned(),
    })
}

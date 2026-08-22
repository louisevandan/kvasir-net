//! The lifecycle half of reading a node-bound body into work.
//!
//! Split out of `mod.rs` on line count alone: `lifecycle` and the cache
//! instruction it dispatches to are the largest single piece of that file,
//! and moving them here (a free function taking exactly what they need,
//! rather than `&Bodies`) is what keeps `impl Payload for Bodies` itself --
//! which Rust allows only one of per crate, so every trait method has to be
//! named somewhere in that one block -- as short as the trait's own method
//! count requires.

use crate::message::ToNode;
use crate::message::wire::decode_to_node;
use p4_adapter::{Cache, CacheAction, Close, Load, Unload, Work};
use p4_protocol::frame::Frame;

/// Reads a lifecycle-shaped frame into the `Work` it describes, given the
/// deployment its own chain link already named.
pub(super) fn lifecycle_work(frame: &Frame, deployment: String) -> Option<Work> {
    match decode_to_node(&frame.body).ok()? {
        ToNode::Load {
            plan,
            artifact,
            capability_snapshot_id,
            capability_expires_at,
            ..
        } => Some(Work::Load(Load {
            deployment,
            plan,
            artifact,
            capability_snapshot_id,
            capability_expires_at,
        })),
        ToNode::Unload => Some(Work::Unload(Unload { deployment })),
        // Cache instructions are lifecycle-shaped: one instruction about
        // one thing, run alone rather than batched into a window. That
        // they are about a sequence and a load is about a deployment makes
        // no difference to the node, which cares only that they do not
        // batch.
        ToNode::Persist { sequence } => {
            Some(cache(frame, deployment, sequence, CacheAction::Persist))
        }
        ToNode::PreparePersist { sequence } => Some(cache(
            frame,
            deployment,
            sequence,
            CacheAction::PreparePersist,
        )),
        ToNode::Restore { sequence } => {
            Some(cache(frame, deployment, sequence, CacheAction::Restore))
        }
        ToNode::PrepareRestore { sequence } => Some(cache(
            frame,
            deployment,
            sequence,
            CacheAction::PrepareRestore,
        )),
        ToNode::Fork { sequence, into } => Some(cache(
            frame,
            deployment,
            sequence,
            CacheAction::Fork { into },
        )),
        ToNode::Discard { sequence } => {
            Some(cache(frame, deployment, sequence, CacheAction::Discard))
        }
        ToNode::PrepareDiscard { sequence } => Some(cache(
            frame,
            deployment,
            sequence,
            CacheAction::PrepareDiscard,
        )),
        ToNode::Commit { sequence } => {
            Some(cache(frame, deployment, sequence, CacheAction::Commit))
        }
        ToNode::Abort { sequence } => Some(cache(frame, deployment, sequence, CacheAction::Abort)),
        ToNode::Reconcile { sequence } => {
            Some(cache(frame, deployment, sequence, CacheAction::Reconcile))
        }
        ToNode::SessionClose {
            sequence,
            session_epoch,
            ..
        } => Some(Work::Close(Close {
            deployment,
            generation: frame
                .envelope
                .chain
                .as_ref()
                .map(|chain| chain.current().generation)
                .unwrap_or_default(),
            sequence,
            session_epoch,
        })),
        // Not lifecycle-shaped for this node: an acknowledgement never
        // reaches this match arm in practice, because `Bodies` also
        // implements `session_closed_ack`, and the node checks that
        // before `lifecycle` ever runs on an incoming frame -- see
        // `Node::run`. Explicit `None` here is the fallback if that
        // ordering is ever bypassed, so a stray ack is refused as
        // unschedulable work rather than misread as a fresh hop.
        ToNode::SessionClosed { .. } => None,
        ToNode::Execute { .. } | ToNode::Continue { .. } => None,
    }
}

/// One cache instruction, for a node that will run it alone.
fn cache(frame: &Frame, deployment: String, sequence: String, action: CacheAction) -> Work {
    let generation = frame
        .envelope
        .chain
        .as_ref()
        .map(|chain| chain.current().generation)
        .unwrap_or_default();
    let stage_id = frame
        .envelope
        .chain
        .as_ref()
        .map(|chain| chain.current().node.clone())
        .unwrap_or_default();
    Work::Cache(Cache {
        deployment,
        stage_id,
        generation,
        operation_id: frame.envelope.request_id.clone(),
        sequence,
        action,
    })
}

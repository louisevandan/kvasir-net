//! Reading a node-bound body into work.
//!
//! One implementation of the core's `Payload` seam. Everything backend-shaped
//! stays opaque: a plan goes to the adapter as text and nothing here looks
//! inside it.

use crate::message::wire::{decode_to_node, encode_reply};
use crate::message::{Reply, ToNode};
use p4_adapter::{Cache, CacheAction, Load, Sequence, Unload, Work};
use p4_agent_core::node::payload::Payload;
use p4_protocol::frame::Frame;

pub struct Bodies;

impl Payload for Bodies {
    fn sequence(&self, frame: &Frame) -> Option<Sequence> {
        let ToNode::Execute {
            prompt,
            max_tokens,
            options,
        } = decode_to_node(&frame.body).ok()?
        else {
            return None;
        };
        Some(Sequence {
            // The transport route names this sequence for its whole life, so a
            // reply can be matched to a caller without a second identifier to
            // keep in step.
            sequence: frame.envelope.route.clone(),
            position: 0,
            // Present on the node that begins the work. A later stage
            // continues from state it holds, and reads the prompt only because
            // the same body travels the chain.
            prompt: Some(prompt),
            remaining: max_tokens,
            options,
        })
    }

    fn lifecycle(&self, frame: &Frame) -> Option<Work> {
        let deployment = self.deployment(frame)?;
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
            ToNode::Persist { sequence } => Some(cache(deployment, sequence, CacheAction::Persist)),
            ToNode::Restore { sequence } => Some(cache(deployment, sequence, CacheAction::Restore)),
            ToNode::Fork { sequence, into } => {
                Some(cache(deployment, sequence, CacheAction::Fork { into }))
            }
            ToNode::Discard { sequence } => Some(cache(deployment, sequence, CacheAction::Discard)),
            ToNode::Execute { .. } => None,
        }
    }

    fn ceiling(&self, frame: &Frame) -> Option<usize> {
        let ToNode::Load { ceiling, .. } = decode_to_node(&frame.body).ok()? else {
            return None;
        };
        usize::try_from(ceiling).ok()
    }

    fn lifecycle_error(&self, frame: &Frame) -> Option<String> {
        let ToNode::Load {
            capability_snapshot_id,
            capability_expires_at,
            ..
        } = decode_to_node(&frame.body).ok()?
        else {
            return None;
        };
        if capability_snapshot_id.is_empty() {
            return None;
        }
        if capability_expires_at == 0 {
            return Some("capability snapshot has no expiry".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis() as u64;
        (capability_expires_at <= now).then(|| {
            format!(
                "capability snapshot {capability_snapshot_id} expired at {capability_expires_at}"
            )
        })
    }

    // The outbound half. Replies are the same vocabulary a caller sent in, so
    // one decoder reads everything that comes back.

    fn token(&self, text: &str, index: u32) -> Vec<u8> {
        encode_reply(&Reply::Token {
            index,
            text: text.to_owned(),
        })
    }

    fn finished(&self, reason: &str, generated: u32) -> Vec<u8> {
        encode_reply(&Reply::Done {
            reason: reason.to_owned(),
            generated,
        })
    }

    fn failure(&self, detail: &str) -> Vec<u8> {
        encode_reply(&Reply::Failed {
            detail: detail.to_owned(),
        })
    }

    fn progress(&self, stage: u32, percent: u32) -> Vec<u8> {
        encode_reply(&Reply::Progress { stage, percent })
    }

    fn bound(&self, generation: u64) -> Vec<u8> {
        encode_reply(&Reply::Bound { generation })
    }

    fn released(&self) -> Vec<u8> {
        encode_reply(&Reply::Released)
    }

    fn cached(&self, sequence: &str, bytes: u64, detail: &str) -> Vec<u8> {
        encode_reply(&Reply::Cached {
            sequence: sequence.to_owned(),
            bytes,
            detail: detail.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests;

/// One cache instruction, for a node that will run it alone.
fn cache(deployment: String, sequence: String, action: CacheAction) -> Work {
    Work::Cache(Cache {
        deployment,
        sequence,
        action,
    })
}

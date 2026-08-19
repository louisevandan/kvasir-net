//! Reading a node-bound body into work.
//!
//! One implementation of the core's `Payload` seam. Everything backend-shaped
//! stays opaque: a plan goes to the adapter as text and nothing here looks
//! inside it.

use crate::capability::CapabilityRegistry;
use crate::message::wire::{decode_to_node, encode_reply, encode_to_node};
use crate::message::{Reply, ToNode};
use p4_adapter::{Cache, CacheAction, Load, Outcome, Sequence, Unload, Work};
use p4_agent_core::node::payload::Payload;
use p4_protocol::frame::Frame;

#[derive(Default)]
pub struct Bodies {
    capabilities: Option<CapabilityRegistry>,
}

impl Bodies {
    pub fn with_capabilities(capabilities: CapabilityRegistry) -> Self {
        Self {
            capabilities: Some(capabilities),
        }
    }
}

impl Payload for Bodies {
    fn sequence(&self, frame: &Frame) -> Option<Sequence> {
        let (inbound_cut_set, body) = if p4_adapter::is_continuation(&frame.body) {
            let (cut_set, original) = p4_adapter::decode_continuation(&frame.body)?;
            (Some(cut_set), original)
        } else {
            (None, frame.body.clone())
        };
        let decoded = decode_to_node(&body).ok()?;
        let (prompt, position, remaining, token, options) = match decoded {
            ToNode::Execute {
                prompt,
                max_tokens,
                options,
            } => (Some(prompt), 0, max_tokens, None, options),
            ToNode::Continue {
                position,
                remaining,
                token,
                options,
            } => (None, position, remaining, token, options),
            _ => return None,
        };
        let first_stage = inbound_cut_set.is_none();
        Some(Sequence {
            // The logical request owns the backend sequence. Route is only a
            // transport/continuation key and may change across reconnect or
            // relay, so using it here would make KV state impossible to map
            // back to the request that produced it.
            sequence: if frame.envelope.request_id.is_empty() {
                frame.envelope.route.clone()
            } else {
                frame.envelope.request_id.clone()
            },
            inbound_cut_set,
            position,
            // Present on the node that begins the work. A later stage
            // continues from state it holds, and reads the prompt only because
            // the same body travels the chain.
            prompt: first_stage.then_some(prompt).flatten(),
            initial_tokens: first_stage
                .then_some(token.map(|value| vec![value as i32]))
                .flatten(),
            remaining,
            options,
        })
    }

    fn continue_body(&self, carrier: &Frame, outcome: &Outcome) -> Vec<u8> {
        let original = p4_adapter::decode_continuation(&carrier.body)
            .map(|(_, body)| body)
            .unwrap_or_else(|| carrier.body.clone());
        let Ok(message) = decode_to_node(&original) else {
            return carrier.body.clone();
        };
        let (remaining, options) = match message {
            ToNode::Execute {
                max_tokens,
                options,
                ..
            } => (max_tokens, options),
            ToNode::Continue {
                remaining, options, ..
            } => (remaining, options),
            _ => return carrier.body.clone(),
        };
        encode_to_node(&ToNode::Continue {
            position: outcome.position,
            // This field is the request's total token bound in the existing
            // P4 contract. Position is the progress state; reducing both
            // would make the mock and served backends stop early.
            remaining,
            token: outcome.token.and_then(|value| u32::try_from(value).ok()),
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
            ToNode::Abort { sequence } => {
                Some(cache(frame, deployment, sequence, CacheAction::Abort))
            }
            ToNode::Reconcile { sequence } => {
                Some(cache(frame, deployment, sequence, CacheAction::Reconcile))
            }
            ToNode::Execute { .. } | ToNode::Continue { .. } => None,
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
            artifact,
            capability_snapshot_id,
            capability_expires_at,
            ..
        } = decode_to_node(&frame.body).ok()?
        else {
            return None;
        };
        if capability_snapshot_id.is_empty() {
            return Some("load requires a capability snapshot id".into());
        }
        if capability_expires_at == 0 {
            return Some("capability snapshot has no expiry".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis() as u64;
        if capability_expires_at <= now {
            return Some(format!(
                "capability snapshot {capability_snapshot_id} expired at {capability_expires_at}"
            ));
        }
        if let Some(capabilities) = &self.capabilities
            && !capabilities.matches(&capability_snapshot_id, &artifact, capability_expires_at)
        {
            return Some(format!(
                "capability snapshot {capability_snapshot_id} does not match this agent load"
            ));
        }
        None
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
            final_token: None,
        })
    }

    fn finished_with_token(
        &self,
        reason: &str,
        generated: u32,
        index: u32,
        text: &str,
    ) -> Option<Vec<u8>> {
        Some(encode_reply(&Reply::Done {
            reason: reason.to_owned(),
            generated,
            final_token: Some((index, text.to_owned())),
        }))
    }

    fn failure(&self, detail: &str) -> Vec<u8> {
        encode_reply(&Reply::Failed {
            detail: detail.to_owned(),
        })
    }

    fn cache_failure(&self, frame: &Frame, detail: &str) -> Vec<u8> {
        let Some(Work::Cache(cache)) = self.lifecycle(frame) else {
            return self.failure(detail);
        };
        let Some(link) = frame.envelope.chain.as_ref().map(|chain| chain.current()) else {
            return self.failure(detail);
        };
        encode_reply(&Reply::CacheFailed {
            deployment: link.binding.clone(),
            stage_id: link.node.clone(),
            generation: link.generation,
            operation_id: frame.envelope.request_id.clone(),
            sequence: cache.subject().clone(),
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

    fn cached(
        &self,
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

    fn cache_status(
        &self,
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
}

#[cfg(test)]
mod tests;

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

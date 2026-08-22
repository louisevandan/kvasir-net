//! Reading a node-bound body into work.
//!
//! One implementation of the core's `Payload` seam. Everything backend-shaped
//! stays opaque: a plan goes to the adapter as text and nothing here looks
//! inside it.

use crate::capability::CapabilityRegistry;
use crate::message::ToNode;
use crate::message::wire::{decode_to_node, encode_to_node};
use p4_adapter::deployment::Submit;
use p4_adapter::{Outcome, Sequence, Work};
use p4_agent_core::node::payload::Payload;
use p4_protocol::frame::Frame;
use serde_json::{Value, json};

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
        // A body is either the request as OUTER stated it or whatever the
        // adapter last produced. Nothing here unwraps the second: it is moved,
        // not read.
        let (prompt, remaining, state, options, session_epoch) =
            match decode_to_node(&frame.body).ok()? {
                ToNode::Execute {
                    prompt,
                    max_tokens,
                    options,
                    session_epoch,
                    ..
                } => (Some(prompt), max_tokens, None, options, session_epoch),
                ToNode::Continue {
                    remaining,
                    options,
                    state,
                    session_epoch,
                    ..
                } => (None, remaining, Some(state), options, session_epoch),
                _ => return None,
            };
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
            session_epoch,
            prompt,
            state,
            remaining,
            options,
        })
    }

    /// Builds the `Submit` a broker relay sends to a registered deployment
    /// client for one fresh request, or `None` for anything that is not one
    /// (a continuation, a lifecycle frame, or a frame this vocabulary does
    /// not recognise at all) -- `sequence`'s own `prompt: Option<String>`
    /// already draws exactly this line, so this reuses it rather than
    /// re-deciding it.
    ///
    /// `deployment_id` and `deployment_generation` come from the chain's
    /// current link, the same source `deployment` reads; `submission_id`
    /// reuses `sequence`'s own request identity, so a resend of one P4
    /// request still names one submission on the wire (`Submit`'s own doc
    /// requires this). The chat-completion shape of `request` -- `messages`,
    /// `max_tokens`, `stream` -- is what `apps/llama`'s submission-stream
    /// server requires (`parseRingChatRequest`); building it here, rather
    /// than in the backend-neutral relay that calls this, is the same split
    /// `served/`'s own `chat::Request::body()` already draws for the hop
    /// path -- the OpenAI-compatible surface is vocabulary the wire needs to
    /// agree on, not something the core may invent per adapter.
    fn submission(&self, frame: &Frame) -> Option<Submit> {
        let sequence = self.sequence(frame)?;
        let prompt = sequence.prompt?;
        let deployment_id = self.deployment(frame)?;
        let deployment_generation = frame.envelope.chain.as_ref()?.current().generation;
        let mut request = json!({
            "messages": [{ "role": "user", "content": prompt }],
            "max_tokens": sequence.remaining,
            "stream": true,
        });
        if let Ok(Value::Object(options)) = serde_json::from_str::<Value>(&sequence.options) {
            let map = request.as_object_mut().expect("request root is an object");
            for (key, value) in options {
                map.insert(key, value);
            }
        }
        Some(Submit {
            deployment_id,
            deployment_generation,
            submission_id: sequence.sequence,
            request,
        })
    }

    fn continue_body(&self, carrier: &Frame, outcome: &Outcome) -> Vec<u8> {
        let Ok(message) = decode_to_node(&carrier.body) else {
            return carrier.body.clone();
        };
        let (remaining, emitted, options, session_epoch) = match message {
            ToNode::Execute {
                max_tokens,
                options,
                session_epoch,
                ..
            } => (max_tokens, 0, options, session_epoch),
            ToNode::Continue {
                remaining,
                emitted,
                options,
                session_epoch,
                ..
            } => (remaining, emitted, options, session_epoch),
            _ => return carrier.body.clone(),
        };
        encode_to_node(&ToNode::Continue {
            // The request's total token bound, which is P4's contract with
            // whoever asked. How far along the session is belongs to the
            // adapter and travels in `state`.
            remaining,
            // One more only when this hop actually produced something for
            // whoever asked. A stage that forwards state and no text has not
            // spent any of the request's budget.
            emitted: emitted.saturating_add(u32::from(!outcome.text.is_empty())),
            options,
            state: outcome.forward.clone().unwrap_or_default(),
            // Carried unchanged -- see `ToNode::Execute::session_epoch`'s own
            // doc for why this must never be re-minted on a lap.
            session_epoch,
        })
    }

    fn emitted(&self, carrier: &Frame) -> u32 {
        match decode_to_node(&carrier.body) {
            Ok(ToNode::Continue { emitted, .. }) => emitted,
            _ => 0,
        }
    }

    fn lifecycle(&self, frame: &Frame) -> Option<Work> {
        let deployment = self.deployment(frame)?;
        inbound::lifecycle_work(frame, deployment)
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

    // The outbound half -- everything a node writes back -- has its
    // encoding in `outbound.rs` instead of here, because it changes
    // independently of what this half reads. Rust allows only one `impl
    // Payload for Bodies` per crate, so every method still has to be named
    // in this one block; each body below is a one-line call into that
    // module, except `cache_failure`, which needs `self.lifecycle` to find
    // its identity fields before `outbound::cache_failure` can encode them.

    fn token(&self, text: &str, index: u32) -> Vec<u8> {
        outbound::token(text, index)
    }

    fn finished(&self, reason: &str, generated: u32) -> Vec<u8> {
        outbound::finished(reason, generated)
    }

    fn finished_with_token(
        &self,
        reason: &str,
        generated: u32,
        index: u32,
        text: &str,
    ) -> Option<Vec<u8>> {
        Some(outbound::finished_with_token(
            reason, generated, index, text,
        ))
    }

    fn failure(&self, detail: &str) -> Vec<u8> {
        outbound::failure(detail)
    }

    fn cache_failure(&self, frame: &Frame, detail: &str) -> Vec<u8> {
        let Some(Work::Cache(cache)) = self.lifecycle(frame) else {
            return outbound::failure(detail);
        };
        let Some(link) = frame.envelope.chain.as_ref().map(|chain| chain.current()) else {
            return outbound::failure(detail);
        };
        outbound::cache_failure(
            &link.binding,
            &link.node,
            link.generation,
            &frame.envelope.request_id,
            cache.subject(),
            detail,
        )
    }

    fn progress(&self, stage: u32, percent: u32) -> Vec<u8> {
        outbound::progress(stage, percent)
    }

    fn bound(&self, generation: u64) -> Vec<u8> {
        outbound::bound(generation)
    }

    fn released(&self) -> Vec<u8> {
        outbound::released()
    }

    fn close(&self, sequence: &str, close_id: u64, session_epoch: u64) -> Vec<u8> {
        outbound::close(sequence, close_id, session_epoch)
    }

    fn session_closed(&self, sequence: &str, close_id: u64) -> Vec<u8> {
        outbound::session_closed(sequence, close_id)
    }

    fn close_identity(&self, frame: &Frame) -> Option<(String, u64)> {
        outbound::close_identity(frame)
    }

    fn session_epoch(&self, frame: &Frame) -> Option<u64> {
        outbound::session_epoch(frame)
    }

    fn session_closed_ack(&self, frame: &Frame) -> Option<(String, u64)> {
        outbound::session_closed_ack(frame)
    }

    fn supports_close(&self) -> bool {
        true
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
        outbound::cached(
            deployment,
            stage_id,
            generation,
            operation_id,
            sequence,
            bytes,
            detail,
        )
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
        outbound::cache_status(
            deployment,
            stage_id,
            generation,
            operation_id,
            sequence,
            state,
            bytes,
            detail,
        )
    }
}

mod inbound;
mod outbound;

#[cfg(test)]
mod tests;

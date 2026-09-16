//! What an agent does with a message addressed to itself.
//!
//! Three things, and they are the three the agent owns: its node registry,
//! taking in an inference, and facts about its machine. Anything else arriving
//! here is a reply nobody claimed, which is reported rather than dropped.
//!
//! Every branch is a procedure. Nothing waits, and the only output is a frame
//! on the queue.

use crate::capability::{Capability, CapabilityRegistry};
use crate::machine;
use crate::message::wire::{decode_to_agent, encode_reply};
use crate::message::{Reply, ToAgent};
use crate::registry::Registry;
use crate::status;
use p4_agent_core::agent::{Agent, Duties};
use p4_protocol::frame::Frame;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CancellationKey {
    request_id: String,
    stream_id: String,
    return_channel: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CancellationState {
    Pending,
    Cancelled,
    ActiveHop,
}

pub struct Standard {
    registry: Registry,
    capabilities: CapabilityRegistry,
    /// Ceiling a node starts with, before a load declares its own. One, so a
    /// node that was never loaded cannot batch against nothing.
    initial_ceiling: usize,
    cancellations: Arc<Mutex<HashMap<CancellationKey, CancellationState>>>,
}

impl Standard {
    pub fn new(registry: Registry) -> Self {
        Self::with_capabilities(registry, CapabilityRegistry::default())
    }

    pub fn with_capabilities(registry: Registry, capabilities: CapabilityRegistry) -> Self {
        Self {
            registry,
            capabilities,
            initial_ceiling: 1,
            cancellations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn adapters(&self) -> Vec<String> {
        self.registry.kinds()
    }

    pub fn capabilities(&self) -> CapabilityRegistry {
        self.capabilities.clone()
    }

    fn create(&self, agent: &Arc<Agent>, frame: &Frame, node: String, adapter: String) {
        let Some(built) = self.registry.build(&adapter, &node) else {
            // A node named against a backend this process does not have is a
            // placement mistake, and the caller is the only one who can fix it.
            return answer(
                agent,
                frame,
                Reply::Failed {
                    detail: format!("no adapter registered as {adapter}"),
                },
            );
        };
        let ceiling = self.initial_ceiling;
        let reply = reply_frame(
            frame,
            Reply::Accepted {
                detail: format!("node {node} created on {adapter}"),
            },
        );
        // The registry that holds nodes is behind an async lock, so this
        // finishes on its own task. Waiting here is what the CPS rule forbids,
        // and the reply is a message like any other.
        let agent = Arc::clone(agent);
        tokio::spawn(async move {
            agent.create_node(node, built, ceiling).await;
            if let Some(reply) = reply {
                let _ = agent.enqueue(reply);
            }
        });
    }

    fn inspect_model(&self, agent: &Arc<Agent>, frame: &Frame, artifact: String, adapter: String) {
        let Some(adapter_instance) = self.registry.build(&adapter, "inspect") else {
            return answer(
                agent,
                frame,
                Reply::Failed {
                    detail: format!("no adapter registered as {adapter}"),
                },
            );
        };
        let frame = frame.clone();
        let agent = Arc::clone(agent);
        let generated_at = unix_ms();
        let expires_at = generated_at.saturating_add(5 * 60 * 1000);
        let capability_snapshot_id = format!("cap-{generated_at}-{}", frame.envelope.route);
        let capabilities = self.capabilities.clone();
        tokio::task::spawn_blocking(move || {
            let reply = match adapter_instance.inspect_model(&artifact) {
                Ok(profile) => {
                    capabilities.insert(
                        capability_snapshot_id.clone(),
                        Capability {
                            artifact: artifact.clone(),
                            adapter: adapter.clone(),
                            profile: profile.clone(),
                            expires_at,
                        },
                    );
                    Reply::Model {
                        artifact,
                        adapter,
                        profile,
                        capability_snapshot_id,
                        generated_at,
                        expires_at,
                    }
                }
                Err(detail) => Reply::Failed { detail },
            };
            if let Some(reply) = reply_frame(&frame, reply) {
                let _ = agent.enqueue(reply);
            }
        });
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

impl Duties for Standard {
    fn handle(&self, frame: Frame, agent: &Arc<Agent>) {
        let message = match decode_to_agent(&frame.body) {
            Ok(message) => message,
            Err(error) => {
                // A reply this agent asked for lands here when nothing claimed
                // it, and it will not parse as an agent message. Saying so
                // beats a silent drop, which looks the same as a lost route.
                return answer(
                    agent,
                    &frame,
                    Reply::Failed {
                        detail: format!("unreadable agent message: {error}"),
                    },
                );
            }
        };
        match message {
            ToAgent::CreateNode { node, adapter } => self.create(agent, &frame, node, adapter),
            ToAgent::DeleteNode { node } => delete(agent, &frame, node),
            ToAgent::Inspect => answer(
                agent,
                &frame,
                Reply::Machine {
                    snapshot: machine::snapshot(&self.adapters()),
                },
            ),
            ToAgent::InspectModel { artifact, adapter } => {
                self.inspect_model(agent, &frame, artifact, adapter)
            }
            ToAgent::Cancel {
                route,
                request_id,
                stream_id,
                return_channel,
                generation,
            } => cancel(
                agent,
                &frame,
                self.cancellations.clone(),
                CancelRequest {
                    route,
                    request_id,
                    stream_id,
                    return_channel,
                    generation,
                },
            ),
            ToAgent::Status => status(agent, &frame),
            ToAgent::Acknowledge {
                return_channel,
                stream_id,
                event_seq,
            } => {
                // The body names the journal key, but only the envelope was
                // stamped by the ingress reader. Do not let an ACK received
                // on one channel erase another channel's journal.
                if frame.envelope.return_channel.as_deref() != Some(return_channel.as_str()) {
                    agent.record_ack_rejection();
                    return;
                }
                let generation = frame.envelope.ingress_generation;
                let agent = Arc::clone(agent);
                tokio::spawn(async move {
                    let _ = agent
                        .acknowledge_subscription(
                            &return_channel,
                            generation,
                            &stream_id,
                            event_seq,
                        )
                        .await;
                });
            }
        }
    }
}

/// Stops one request, and says whether there was one to stop.
///
/// The distinction matters to a caller: nothing waiting means the request had
/// already finished, which is a different outcome from having cancelled it,
/// and a caller that cannot tell them apart cannot report either.
// See docs/protocol-outer.md#results-and-retries.
struct CancelRequest {
    route: String,
    request_id: String,
    stream_id: String,
    return_channel: String,
    generation: u64,
}

fn cancel(
    agent: &Arc<Agent>,
    frame: &Frame,
    cancellations: Arc<Mutex<HashMap<CancellationKey, CancellationState>>>,
    request: CancelRequest,
) {
    let CancelRequest {
        route,
        request_id,
        stream_id,
        return_channel,
        generation,
    } = request;
    let key = CancellationKey {
        request_id: request_id.clone(),
        stream_id: stream_id.clone(),
        return_channel: return_channel.clone(),
    };
    let stopped = reply_frame(
        frame,
        Reply::Accepted {
            detail: format!("cancel accepted for {route}"),
        },
    );
    let active_reply = reply_frame(
        frame,
        Reply::Accepted {
            detail: format!(
                "cancel requested for active hop {route}; backend interruption is not claimed"
            ),
        },
    );
    let duplicate = reply_frame(
        frame,
        Reply::Accepted {
            detail: format!("cancel already recorded for {route}"),
        },
    );
    let stale = reply_frame(
        frame,
        Reply::Failed {
            detail: format!("stale cancellation fence for {route}"),
        },
    );
    let nothing = reply_frame(
        frame,
        Reply::Failed {
            detail: format!("request already terminal: {route}"),
        },
    );
    let source_channel = frame_return_channel(frame).to_owned();
    let source_generation = frame_generation(frame);
    let agent = Arc::clone(agent);
    tokio::spawn(async move {
        {
            let mut states = cancellations.lock().expect("cancellation registry lock");
            if states.contains_key(&key) {
                if let Some(reply) = duplicate {
                    let _ = agent.enqueue(reply);
                }
                return;
            }
            states.insert(key.clone(), CancellationState::Pending);
        }

        if return_channel != source_channel || generation != source_generation {
            cancellations
                .lock()
                .expect("cancellation registry lock")
                .remove(&key);
            if let Some(reply) = stale {
                let _ = agent.enqueue(reply);
            }
            return;
        }

        let cancelled = agent.cancel_frame(&route).await;
        if let Some(carrier) = cancelled {
            let fenced = carrier.envelope.request_id == request_id
                && carrier.envelope.stream_id == stream_id
                && carrier.envelope.return_channel.as_deref() == Some(return_channel.as_str())
                && carrier.envelope.ingress_generation == generation;
            if !fenced {
                let _ = agent.enqueue(carrier);
                cancellations
                    .lock()
                    .expect("cancellation registry lock")
                    .remove(&key);
                if let Some(reply) = stale {
                    let _ = agent.enqueue(reply);
                }
                return;
            }
            cancellations
                .lock()
                .expect("cancellation registry lock")
                .insert(key, CancellationState::Cancelled);
            if let Some(reply) = terminal_reply_frame(
                &carrier,
                Reply::Failed {
                    detail: format!("cancelled before start: {route}"),
                },
            ) {
                let _ = agent.enqueue(reply);
            }
            if let Some(reply) = stopped {
                let _ = agent.enqueue(reply);
            }
        } else {
            let active = agent.node_status().await.into_iter().any(|node| {
                node.active_hop.is_some_and(|hop| {
                    hop.requests.iter().any(|candidate| {
                        candidate.route == route
                            && candidate.request_id == request_id
                            && candidate.stream_id == stream_id
                    })
                })
            });
            if active {
                cancellations
                    .lock()
                    .expect("cancellation registry lock")
                    .insert(key, CancellationState::ActiveHop);
                if let Some(reply) = active_reply {
                    let _ = agent.enqueue(reply);
                }
            } else {
                cancellations
                    .lock()
                    .expect("cancellation registry lock")
                    .remove(&key);
                if let Some(reply) = nothing {
                    let _ = agent.enqueue(reply);
                }
            }
        }
    });
}

fn frame_return_channel(frame: &Frame) -> &str {
    frame.envelope.return_channel.as_deref().unwrap_or_default()
}

fn frame_generation(frame: &Frame) -> u64 {
    frame.envelope.ingress_generation
}

/// What the agent is doing, as of now.
///
/// Taken asynchronously because reading the nodes takes their lock, and a
/// duties handler that waited on it would put a node's lock on the worker
/// path — the thing the two-tier queue exists to avoid.
fn status(agent: &Arc<Agent>, frame: &Frame) {
    let frame = frame.clone();
    let agent = Arc::clone(agent);
    tokio::spawn(async move {
        let snapshot = status::typed_snapshot(&agent).await;
        if let Some(reply) = reply_frame(&frame, Reply::StatusSnapshot { snapshot }) {
            let _ = agent.enqueue(reply);
        }
    });
}

fn delete(agent: &Arc<Agent>, frame: &Frame, node: String) {
    let removed_reply = reply_frame(frame, Reply::Released);
    let missing_reply = reply_frame(
        frame,
        Reply::Failed {
            detail: format!("no node {node} on this agent"),
        },
    );
    let agent = Arc::clone(agent);
    tokio::spawn(async move {
        let reply = if agent.delete_node(&node).await {
            removed_reply
        } else {
            missing_reply
        };
        if let Some(reply) = reply {
            let _ = agent.enqueue(reply);
        }
    });
}

fn answer(agent: &Arc<Agent>, frame: &Frame, reply: Reply) {
    if let Some(frame) = reply_frame(frame, reply) {
        let _ = agent.enqueue(frame);
    }
}

/// Builds the reply to a frame, or nothing when nobody asked for one.
fn reply_frame(frame: &Frame, reply: Reply) -> Option<Frame> {
    Some(Frame {
        envelope: frame.envelope.to_reply()?,
        body: encode_reply(&reply),
    })
}

/// Makes a cancellation terminal replayable even when the queued carrier has
/// not emitted an event yet. See docs/protocol-outer.md#results-and-retries.
fn terminal_reply_frame(frame: &Frame, reply: Reply) -> Option<Frame> {
    let mut reply = reply_frame(frame, reply)?;
    reply.envelope.event_seq = frame.envelope.event_seq.saturating_add(1);
    Some(reply)
}

#[cfg(test)]
mod tests;

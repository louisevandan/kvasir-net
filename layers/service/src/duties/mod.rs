//! What an agent does with a message addressed to itself.
//!
//! Three things, and they are the three the agent owns: its node registry,
//! taking in an inference, and facts about its machine. Anything else arriving
//! here is a reply nobody claimed, which is reported rather than dropped.
//!
//! Every branch is a procedure. Nothing waits, and the only output is a frame
//! on the queue.

use crate::machine;
use crate::message::wire::{decode_to_agent, encode_reply};
use crate::message::{Reply, ToAgent};
use crate::registry::Registry;
use crate::status;
use p4_agent_core::agent::{Agent, Duties};
use p4_protocol::frame::Frame;
use std::sync::Arc;

pub struct Standard {
    registry: Registry,
    /// Ceiling a node starts with, before a load declares its own. One, so a
    /// node that was never loaded cannot batch against nothing.
    initial_ceiling: usize,
}

impl Standard {
    pub fn new(registry: Registry) -> Self {
        Self {
            registry,
            initial_ceiling: 1,
        }
    }

    pub fn adapters(&self) -> Vec<String> {
        self.registry.kinds()
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
        tokio::task::spawn_blocking(move || {
            let reply = match adapter_instance.inspect_model(&artifact) {
                Ok(profile) => Reply::Model {
                    artifact,
                    adapter,
                    profile,
                },
                Err(detail) => Reply::Failed { detail },
            };
            if let Some(reply) = reply_frame(&frame, reply) {
                let _ = agent.enqueue(reply);
            }
        });
    }
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
            ToAgent::Cancel { route } => cancel(agent, &frame, route),
            ToAgent::Status => status(agent, &frame),
        }
    }
}

/// Stops one request, and says whether there was one to stop.
///
/// The distinction matters to a caller: nothing waiting means the request had
/// already finished, which is a different outcome from having cancelled it,
/// and a caller that cannot tell them apart cannot report either.
fn cancel(agent: &Arc<Agent>, frame: &Frame, route: String) {
    let stopped = reply_frame(
        frame,
        Reply::Accepted {
            detail: format!("cancelled {route}"),
        },
    );
    let nothing = reply_frame(
        frame,
        Reply::Failed {
            detail: format!("nothing waiting for {route}"),
        },
    );
    let agent = Arc::clone(agent);
    tokio::spawn(async move {
        let reply = if agent.cancel(&route).await {
            stopped
        } else {
            nothing
        };
        if let Some(reply) = reply {
            let _ = agent.enqueue(reply);
        }
    });
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
        let snapshot = status::snapshot(&agent).await;
        if let Some(reply) = reply_frame(&frame, Reply::Status { snapshot }) {
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

#[cfg(test)]
mod tests;

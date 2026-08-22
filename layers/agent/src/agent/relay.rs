//! The broker relay: `Agent::dispatch`'s alternative to composing a hop.
//!
//! `SEALED-CONTRACT.md` §9.1/§9.4 says what a P4 broker keeps: select a
//! client by `DeploymentId`, forward `Submit`/`Cancel` to its bounded queue,
//! and relay `Accepted`/`Rejected`/`Produced`/`Settled` back to whoever
//! asked. This module is that relay's whole implementation. It is a
//! submodule of `agent` rather than a sibling one so it can reach `Agent`'s
//! private fields directly -- `submission_routes`, `deployments`, `payload`,
//! `emergency` -- the same way `super::worker` or any other part of this
//! struct's own implementation would, without widening any of their
//! visibility past what the rest of this crate already has.
//!
//! **The crux this module exists to answer**: a `Client` is handed one
//! long-lived `Arc<dyn Sink>` at construction (`p4_adapter::deployment`'s own
//! doc explains why -- a per-call sink cannot outlive an async `Produced`),
//! so a `Produced`/`Settled` arriving long after `dispatch` returned has to
//! find its way back to a specific in-flight request from nothing but a
//! `submission_id`. `submission_routes` is that routing table, and it lives
//! on `Agent` itself (not on the sink) because the sink is a fixed,
//! process-lifetime object while this table's rows come and go with every
//! submission -- see `AgentDeploymentSink`'s own doc for the ownership shape
//! that follows from that.

use super::{
    Agent, DeploymentEvent, DeploymentSink, Produced, Rejected, RejectedReason, Settled,
    SettledReason,
};
use p4_protocol::frame::Frame;
use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::Ordering;
use std::time::Duration;

impl Agent {
    /// Where a P4 broker relay registers the client(s) `dispatch` calls
    /// `try_submit`/`cancel` on -- `SEALED-CONTRACT.md` §9.1's "select an
    /// adapter instance by `DeploymentId`", and nothing more. Whoever
    /// constructs this agent registers into it after construction, once an
    /// `AgentDeploymentSink` can hold a `Weak` back to this very agent (see
    /// that type's own doc for why late binding, rather than a constructor
    /// parameter, is what breaks that ordering cycle).
    pub fn deployments(&self) -> &super::DeploymentRegistry {
        &self.deployments
    }

    /// How many frames the relay diverted to a deployment client instead of
    /// a node's hop queue, since the process started. The hop-path
    /// equivalent of `to_nodes` -- see `dispatch`'s own doc for where the
    /// two are decided.
    pub fn to_deployment(&self) -> usize {
        self.to_deployment.load(Ordering::Relaxed)
    }

    /// Diverts one fresh submission to its deployment client instead of a
    /// node's hop queue -- `dispatch`'s own doc explains the gate that leads
    /// here. `carrier` is kept as the seed every reply for this
    /// `submission_id` gets built against; `submit.submission_id` is the key
    /// `relay_deployment_event` uses to find it again when that client's
    /// sink raises something about it, arbitrarily far in the future.
    pub(super) fn relay_submit(
        self: &Arc<Self>,
        carrier: Frame,
        submit: p4_adapter::deployment::Submit,
    ) {
        self.to_deployment.fetch_add(1, Ordering::Relaxed);
        let submission_id = submit.submission_id.clone();
        self.submission_routes
            .lock()
            .expect("submission route lock")
            .insert(submission_id.clone(), carrier);
        // `try_submit` is an enqueue attempt, not an admission verdict --
        // `Client::try_submit`'s own doc. `Err` here means the client itself
        // could not take the submission at all (e.g. it is already closed),
        // which is the one outcome that will never also arrive later on the
        // sink, so nothing but answering now would ever close this route out.
        if self.deployments.try_submit(submit).is_err() {
            let carrier = self
                .submission_routes
                .lock()
                .expect("submission route lock")
                .remove(&submission_id);
            if let Some(carrier) = carrier {
                self.answer_locally(carrier, "deployment client could not enqueue submission");
            }
        }
    }

    /// How many submissions the relay is still tracking a reply route for.
    ///
    /// A test's only window onto whether a failed delivery kept the state it
    /// would need to try again; nothing outside tests has a use for it.
    #[cfg(test)]
    pub(super) fn submission_route_count(&self) -> usize {
        self.submission_routes
            .lock()
            .expect("submission route lock")
            .len()
    }

    /// Cancels a relayed submission by the route its carrier arrived on,
    /// returning that carrier so the caller can terminalize the original
    /// request exactly as the hop path does.
    ///
    /// Without this the ingress cancel walked only the node queues, so a
    /// request already diverted to a deployment client was reported
    /// "already terminal" while it went on generating -- the client's own
    /// lossless cancel delivery had nothing calling it. The route is keyed
    /// by `submission_id`, but a cancel names `envelope.route`, which is
    /// why this searches rather than looks up.
    pub(super) fn cancel_submission(&self, route: &str) -> Option<Frame> {
        let found = {
            let mut routes = self
                .submission_routes
                .lock()
                .expect("submission route lock");
            let submission_id = routes
                .iter()
                .find(|(_, carrier)| carrier.envelope.route == route)
                .map(|(submission_id, _)| submission_id.clone())?;
            routes
                .remove(&submission_id)
                .map(|carrier| (submission_id, carrier))
        };
        let (submission_id, carrier) = found?;
        // Forwarded even when the deployment id cannot be recovered from the
        // carrier: the route is already gone by here, so declining to ask
        // the client to stop would leave the backend generating for a
        // request nothing is listening to any more.
        if let Some(deployment_id) = self.payload.deployment(&carrier) {
            self.deployments.cancel(&deployment_id, submission_id);
        }
        Some(carrier)
    }

    /// What `AgentDeploymentSink::raise` calls for every event a registered
    /// deployment client raises, for any submission this agent's relay ever
    /// diverted -- including one that settled, or was cancelled, well before
    /// this call. `submission_routes` is the table that makes a late event
    /// findable at all: the sink itself is long-lived and constructed once,
    /// so it cannot hold per-submission reply state, and this is where that
    /// state actually lives instead.
    ///
    /// `Accepted` has no OUTER-facing effect, matching the hop path: neither
    /// acknowledges `Execute` itself, only the first real output does.
    /// `Rejected { Full }` is ordinary backpressure -- `SEALED-CONTRACT.md`
    /// §1/§9.2 -- so it is retried rather than answered as a failure; every
    /// other `Rejected` reason is terminal and answered like any other local
    /// refusal. `Produced`/`Settled` replay through the exact rule
    /// `node::runner::response::response_frame` uses for a hop's own
    /// replies -- `carrier.envelope.to_reply()`, `event_seq` advanced by
    /// exactly one per event -- so a requester cannot tell a relayed
    /// submission's stream from a hop's own.
    fn relay_deployment_event(self: &Arc<Self>, event: DeploymentEvent) {
        match event {
            DeploymentEvent::Accepted(_) => {}
            DeploymentEvent::Rejected(Rejected {
                submission_id,
                reason: RejectedReason::Full,
            }) => self.retry_submission_later(submission_id),
            DeploymentEvent::Rejected(Rejected {
                submission_id,
                reason,
            }) => {
                let carrier = self
                    .submission_routes
                    .lock()
                    .expect("submission route lock")
                    .remove(&submission_id);
                if let Some(carrier) = carrier {
                    self.answer_locally(carrier, &format!("submission rejected: {reason:?}"));
                }
            }
            DeploymentEvent::Produced(Produced {
                submission_id,
                event_ordinal,
                text,
                ..
            }) => {
                let body = self.payload.token(&text, event_ordinal as u32);
                self.relay_submission_reply(&submission_id, body);
            }
            DeploymentEvent::Settled(Settled {
                submission_id,
                reason,
                generated_tokens,
            }) => {
                let body = self
                    .payload
                    .finished(settled_reason_str(reason), generated_tokens);
                self.finish_submission_route(&submission_id, body);
            }
        }
    }

    /// `Rejected { Full }` names ordinary backpressure, not a failed
    /// request (`SEALED-CONTRACT.md` §1/§9.2), so it is never turned into a
    /// reply here. Instead the same `Submit` is retried after a short delay
    /// -- safe because a `Full` rejection is never recorded by a client's
    /// own ledger (`coordinator.ts`'s `admitSubmission` never calls
    /// `ledger.begin` before that check), so a resend under the identical
    /// `submission_id` starts nothing twice; it is simply the first attempt
    /// this client ever actually admits. Rebuilt from the stored carrier
    /// rather than an original `Submit` kept around, so a retry always
    /// reflects the same request a caller would get by decoding the carrier
    /// fresh -- there is only one source of truth for what this submission
    /// asked for.
    ///
    /// No backoff cap and no give-up: P4 does not compute a backend's
    /// remaining capacity (§9.2), so it has no basis for deciding "long
    /// enough" either. A route already removed (settled, or its client
    /// rejected it for a real reason) makes this a no-op.
    fn retry_submission_later(self: &Arc<Self>, submission_id: String) {
        const FULL_RETRY_DELAY: Duration = Duration::from_millis(20);
        let carrier = self
            .submission_routes
            .lock()
            .expect("submission route lock")
            .get(&submission_id)
            .cloned();
        let Some(carrier) = carrier else {
            return;
        };
        let Some(submit) = self.payload.submission(&carrier) else {
            return;
        };
        let agent = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(FULL_RETRY_DELAY).await;
            let still_pending = agent
                .submission_routes
                .lock()
                .expect("submission route lock")
                .contains_key(&submission_id);
            if still_pending {
                let _ = agent.deployments.try_submit(submit);
            }
        });
    }

    /// Sends one relay reply for `submission_id` and advances its stored
    /// carrier's `event_seq` by exactly one, the same rule
    /// `node::runner::response::response_frame` applies to a hop's own
    /// replies. Keeps the route open -- more `Produced` events, or the
    /// eventual `Settled`, may still follow. A missing route (already
    /// settled, cancelled, or a stray event naming an id this agent never
    /// diverted) makes this a silent no-op, matching how a hop's own
    /// `reply` behaves when there is nowhere left to answer.
    fn relay_submission_reply(&self, submission_id: &str, body: Vec<u8>) {
        let frame = {
            let mut routes = self
                .submission_routes
                .lock()
                .expect("submission route lock");
            let Some(carrier) = routes.get_mut(submission_id) else {
                return;
            };
            let Some(mut envelope) = carrier.envelope.to_reply() else {
                return;
            };
            envelope.event_seq = carrier.envelope.event_seq.saturating_add(1);
            carrier.envelope.event_seq = envelope.event_seq;
            Frame { envelope, body }
        };
        self.relay_frame(frame);
    }

    /// The terminal counterpart to `relay_submission_reply`: removes the
    /// route first, because nothing may follow a `Settled` -- exactly the
    /// invariant `p4_adapter::deployment`'s own contract requires of it.
    fn finish_submission_route(&self, submission_id: &str, body: Vec<u8>) {
        let frame = {
            let mut routes = self
                .submission_routes
                .lock()
                .expect("submission route lock");
            let Some(carrier) = routes.get_mut(submission_id) else {
                return;
            };
            let Some(mut envelope) = carrier.envelope.to_reply() else {
                routes.remove(submission_id);
                return;
            };
            envelope.event_seq = carrier.envelope.event_seq.saturating_add(1);
            carrier.envelope.event_seq = envelope.event_seq;
            Frame { envelope, body }
        };
        // The route is dropped only once the terminal is actually away.
        // Removing it first and then failing to enqueue would destroy the
        // one piece of state a resend could be rebuilt from, turning a full
        // output queue into a request that is never answered at all.
        if self.relay_frame(frame) {
            self.submission_routes
                .lock()
                .expect("submission route lock")
                .remove(submission_id);
        }
    }

    /// Delivers one relay-built frame the same way `answer_locally` delivers
    /// a refusal: the normal queue first, the bounded emergency lane if that
    /// is full. Deliberately not `answer_locally` itself -- that method
    /// always counts `refused` and always encodes `detail` through
    /// `payload.failure`, neither of which describes a token or a terminal
    /// this submission actually produced.
    /// Returns whether the frame is on its way. A `false` means both lanes
    /// refused it and this token or terminal is lost -- the caller decides
    /// what to keep so the loss stays recoverable rather than silent.
    fn relay_frame(&self, frame: Frame) -> bool {
        let Err(refused) = self.enqueue(frame) else {
            return true;
        };
        if self.emergency.try_send(refused).is_ok() {
            return true;
        }
        self.emergency_lost.fetch_add(1, Ordering::Relaxed);
        false
    }
}

/// Where every event a registered deployment client raises arrives, and
/// where it turns into a reply for whichever OUTER request the relay is
/// still tracking under that `submission_id` -- see
/// `Agent::relay_deployment_event`.
///
/// Holds `Weak` rather than `Arc` deliberately. This sink is itself reached
/// *through* the agent it replies on: whoever wires a deployment client in
/// registers it into `Agent::deployments()`, so the chain is
/// `Agent -> Registry -> Arc<dyn Client> -> Arc<dyn Sink>`. An owning `Arc`
/// back to the agent here would close that into a reference cycle neither
/// side would ever break, leaking the agent for the life of the process. A
/// `raise` arriving after the agent itself is gone has nowhere left to
/// reply and is dropped.
pub struct AgentDeploymentSink(Weak<Agent>);

impl AgentDeploymentSink {
    pub fn new(agent: Weak<Agent>) -> Self {
        Self(agent)
    }
}

impl DeploymentSink for AgentDeploymentSink {
    fn raise(&self, event: DeploymentEvent) {
        if let Some(agent) = self.0.upgrade() {
            agent.relay_deployment_event(event);
        }
    }
}

fn settled_reason_str(reason: SettledReason) -> &'static str {
    match reason {
        SettledReason::Stop => "stop",
        SettledReason::Length => "length",
        SettledReason::Canceled => "canceled",
        SettledReason::Error => "error",
    }
}

#[cfg(test)]
mod tests;

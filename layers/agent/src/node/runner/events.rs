//! What the adapter says back, and the channel it says it on.
//!
//! The other direction from the loop next door. `mod.rs` decides what to hand
//! a backend; this decides what an answer means — a token routed on, a hop
//! ended, a load bound or refused. The two change apart: scheduling moves when
//! batching does, and this moves when the adapter's event vocabulary does.

use super::{Bound, Node};
use crate::node::outcome::next;
use p4_adapter::{CacheReceiptState, Event, EventSink, Work};
use p4_protocol::frame::Frame;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

/// An adapter event together with the private execution lease that produced
/// it. The lease is deliberately not part of `p4_adapter::Event`: it is an
/// agent-local fence, not a wire/protocol field. A late callback from an older
/// adapter invocation therefore cannot mutate the current node state even
/// when it repeats the same deployment and sequence names.
#[derive(Debug)]
pub(super) struct RaisedEvent {
    pub token: u64,
    pub event: Event,
}

/// Sends adapter events to the node that owns them.
///
/// Bounded by the node ingress budget. `raise` tries a non-blocking send
/// first and only falls back to `blocking_send` -- which applies
/// backpressure rather than dropping a completion -- once the channel is
/// observed full, so `blocked` counts exactly the sends that had to wait.
#[derive(Clone)]
pub(super) struct Sink {
    events: mpsc::Sender<RaisedEvent>,
    raised: Arc<AtomicUsize>,
    lost: Arc<AtomicUsize>,
    blocked: Arc<AtomicUsize>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    token: u64,
}

impl Sink {
    pub(super) fn new(
        events: mpsc::Sender<RaisedEvent>,
        raised: Arc<AtomicUsize>,
        lost: Arc<AtomicUsize>,
        blocked: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            events,
            raised,
            lost,
            blocked,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            token: 0,
        }
    }

    pub(super) fn for_operation(&self, token: u64) -> Self {
        Self {
            events: self.events.clone(),
            raised: Arc::clone(&self.raised),
            lost: Arc::clone(&self.lost),
            blocked: Arc::clone(&self.blocked),
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            token,
        }
    }

    pub(super) fn for_hop(
        &self,
        token: u64,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            events: self.events.clone(),
            raised: Arc::clone(&self.raised),
            lost: Arc::clone(&self.lost),
            blocked: Arc::clone(&self.blocked),
            cancelled,
            token,
        }
    }
}

impl EventSink for Sink {
    fn raise(&self, event: Event) {
        self.raised.fetch_add(1, Ordering::Relaxed);
        let raised = RaisedEvent {
            token: self.token,
            event,
        };
        // A plain `blocking_send` cannot tell a caller whether it had to
        // wait. Trying a non-blocking send first, and only falling back to
        // `blocking_send` when that reports the channel full, gives `raise`
        // the exact same delivery behaviour as before while making the wait
        // itself observable through `blocked`.
        match self.events.try_send(raised) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(raised)) => {
                self.blocked.fetch_add(1, Ordering::Relaxed);
                if self.events.blocking_send(raised).is_err() {
                    self.lost.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.lost.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Node {
    pub(super) async fn on_event(&self, raised: RaisedEvent) {
        if *self
            .active_event_token
            .lock()
            .expect("active event token lock")
            != Some(raised.token)
        {
            self.counts.orphaned.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let event = raised.event;
        match event {
            Event::SequenceAcquired {
                deployment,
                sequence,
            } => {
                if !self.accepts_sequence_event(&deployment, &sequence) {
                    self.counts.invalid_events.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                self.active_sequences
                    .lock()
                    .expect("active sequence lock")
                    .insert(sequence);
            }
            Event::SequenceReleased {
                deployment,
                sequence,
            } => {
                if !self.accepts_sequence_event(&deployment, &sequence) {
                    self.counts.invalid_events.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                self.active_sequences
                    .lock()
                    .expect("active sequence lock")
                    .remove(&sequence);
                if std::env::var_os("P4_AGENT_TRACE_SEQUENCE").is_some() {
                    let active = self.active_sequences.lock().expect("active sequence lock");
                    eprintln!(
                        "P4_AGENT_SEQUENCE_RELEASE route={} active={}",
                        sequence,
                        active.len()
                    );
                }
            }
            Event::HopComplete {
                hop_id,
                deployment,
                expected,
                outcomes,
                ..
            } => {
                let active = self
                    .active_status
                    .lock()
                    .expect("active telemetry lock")
                    .clone();
                if active.as_ref().map(|hop| hop.id) != Some(hop_id) {
                    self.counts.orphaned.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                if active.as_ref().map(|hop| hop.deployment.as_str()) != Some(deployment.as_str()) {
                    self.counts.invalid_events.fetch_add(1, Ordering::Relaxed);
                    let failed =
                        std::mem::take(&mut *self.in_flight.lock().expect("in-flight lock"))
                            .into_values()
                            .collect::<Vec<_>>();
                    self.clear_active_telemetry();
                    *self.active_cancel.lock().expect("cancel lock") = None;
                    self.queue.finished();
                    for frame in failed {
                        self.reply_error(&frame, "adapter returned a hop for the wrong deployment")
                            .await;
                    }
                    self.drain().await;
                    return;
                }
                if self.timed_out.lock().expect("timeout lock").remove(&hop_id) {
                    let sequences = self
                        .in_flight
                        .lock()
                        .expect("in-flight lock")
                        .keys()
                        .cloned()
                        .collect::<HashSet<_>>();
                    self.in_flight.lock().expect("in-flight lock").clear();
                    self.release_active_sequences(&sequences);
                    self.clear_active_telemetry();
                    *self.active_cancel.lock().expect("cancel lock") = None;
                    self.queue.finished();
                    self.drain().await;
                    return;
                }
                let expected_set: HashSet<_> = expected.iter().cloned().collect();
                let outcome_set: HashSet<_> = outcomes
                    .iter()
                    .map(|outcome| outcome.sequence.clone())
                    .collect();
                let in_flight_set: HashSet<_> = self
                    .in_flight
                    .lock()
                    .expect("in-flight lock")
                    .keys()
                    .cloned()
                    .collect();
                let exact = expected.len() == expected_set.len()
                    && outcomes.len() == outcome_set.len()
                    && expected_set == outcome_set
                    && expected_set == in_flight_set;
                if !exact {
                    self.counts.invalid_events.fetch_add(1, Ordering::Relaxed);
                    let failed =
                        std::mem::take(&mut *self.in_flight.lock().expect("in-flight lock"))
                            .into_values()
                            .collect::<Vec<_>>();
                    let sequences = expected.iter().cloned().collect::<HashSet<_>>();
                    self.release_active_sequences(&sequences);
                    self.clear_active_telemetry();
                    *self.active_cancel.lock().expect("cancel lock") = None;
                    self.queue.finished();
                    for frame in failed {
                        self.reply_error(&frame, "adapter returned an incomplete or duplicate hop")
                            .await;
                    }
                    self.drain().await;
                    return;
                }
                self.counts.completions.fetch_add(1, Ordering::Relaxed);
                self.counts
                    .outcomes
                    .fetch_add(outcomes.len(), Ordering::Relaxed);
                let carriers = std::mem::take(&mut *self.in_flight.lock().expect("in-flight lock"));
                // Released before the outcomes are routed, so a lap this hop
                // produces can be picked up by the drain below rather than
                // waiting for the next arrival.
                self.queue.finished();
                self.clear_active_telemetry();
                *self.active_cancel.lock().expect("cancel lock") = None;
                for outcome in outcomes {
                    let Some(carrier) = carriers.get(&outcome.sequence) else {
                        self.counts.orphaned.fetch_add(1, Ordering::Relaxed);
                        continue;
                    };
                    self.counts.routed.fetch_add(1, Ordering::Relaxed);
                    for frame in next(carrier, &outcome, self.payload.as_ref()).frames() {
                        self.emit(frame).await;
                    }
                }
                self.drain().await;
            }
            Event::Failed {
                sequence,
                hop_id,
                detail,
                ..
            } => {
                if let Some(hop_id) = hop_id {
                    if self.active_hop.lock().expect("active hop lock").as_ref() != Some(&hop_id) {
                        self.counts.orphaned.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    if self.timed_out.lock().expect("timeout lock").remove(&hop_id) {
                        let sequences = self
                            .in_flight
                            .lock()
                            .expect("in-flight lock")
                            .keys()
                            .cloned()
                            .collect::<HashSet<_>>();
                        self.in_flight.lock().expect("in-flight lock").clear();
                        self.release_active_sequences(&sequences);
                        self.clear_active_telemetry();
                        *self.active_cancel.lock().expect("cancel lock") = None;
                        self.queue.finished();
                        self.drain().await;
                        return;
                    }
                }
                let (mut failed, failed_sequences): (Vec<Frame>, HashSet<String>) = {
                    let mut in_flight = self.in_flight.lock().expect("in-flight lock");
                    match sequence {
                        Some(ref id) => (
                            in_flight.remove(id).into_iter().collect(),
                            [id.clone()].into_iter().collect(),
                        ),
                        None => {
                            let ids = in_flight.keys().cloned().collect();
                            (std::mem::take(&mut *in_flight).into_values().collect(), ids)
                        }
                    }
                };
                self.active_sequences
                    .lock()
                    .expect("active sequence lock")
                    .retain(|active| !failed_sequences.contains(active));
                let remaining = self.in_flight.lock().expect("in-flight lock").len();
                // A load can fail too, and its caller is waiting on the same
                // route. Leaving it here would hold the node running forever
                // over an instruction that already ended.
                let lifecycle = self.lifecycle.lock().expect("lifecycle lock").take();
                let load_failed = lifecycle.as_ref().is_some_and(|frame| {
                    matches!(self.payload.lifecycle(frame), Some(Work::Load(_)))
                });
                if load_failed {
                    // A stage of a distributed model that did not load is a
                    // stage that must not serve. Its neighbours may have bound
                    // perfectly well, and a chain that runs anyway produces
                    // answers from half a model — which looks like a working
                    // deployment and is the worst outcome available.
                    *self.bound.lock().expect("generation lock") = Bound::Refused;
                }
                failed.extend(lifecycle);
                for frame in failed {
                    self.reply_error(&frame, &detail).await;
                }
                if remaining > 0 {
                    // A per-sequence failure is not a hop completion. The
                    // other sequences in the same adapter batch still own
                    // the node, so starting another hop here would overlap
                    // work and let a later completion consume the wrong map.
                    return;
                }
                self.clear_active_telemetry();
                *self.active_cancel.lock().expect("cancel lock") = None;
                self.queue.finished();
                self.drain().await;
            }
            // Load and unload reporting belongs to whoever asked, and reaches
            // them through the same reply path as anything else.
            // A cache instruction is lifecycle-shaped — one instruction, run
            // alone, answered to whoever asked — so it ends the same way, and
            // notably does not touch the binding: persisting a conversation
            // says nothing about which model is loaded.
            Event::Cached {
                deployment,
                stage_id,
                generation,
                operation_id,
                sequence,
                bytes,
                detail,
            } => {
                self.finish_cache_event(
                    deployment,
                    stage_id,
                    generation,
                    operation_id,
                    sequence,
                    bytes,
                    detail,
                    None,
                )
                .await
            }
            Event::CacheStatus {
                deployment,
                stage_id,
                generation,
                operation_id,
                sequence,
                state,
                bytes,
                detail,
            } => {
                self.finish_cache_event(
                    deployment,
                    stage_id,
                    generation,
                    operation_id,
                    sequence,
                    bytes,
                    detail,
                    Some(state),
                )
                .await
            }
            Event::Loaded {
                deployment,
                generation,
                ..
            } => {
                if !self.accepts_loaded_event(&deployment, generation) {
                    self.reject_lifecycle_event(
                        "adapter returned a loaded event for the wrong deployment or generation",
                    )
                    .await;
                    return;
                }
                *self.bound.lock().expect("generation lock") = Bound::At(generation);
                self.finish_lifecycle(self.payload.bound(generation)).await
            }
            Event::Unloaded { deployment } => {
                if !self.accepts_unload_event(&deployment) {
                    self.reject_lifecycle_event(
                        "adapter returned an unloaded event for the wrong deployment",
                    )
                    .await;
                    return;
                }
                self.active_sequences
                    .lock()
                    .expect("active sequence lock")
                    .clear();
                self.finish_lifecycle(self.payload.released()).await
            }
            // Progress is reported as it happens rather than held until the
            // end, because a distributed load's slowest stage is the fact
            // worth seeing early.
            Event::LoadProgress {
                deployment,
                stage,
                percent,
                ..
            } => {
                if !self.accepts_load_event(&deployment) {
                    self.reject_lifecycle_event(
                        "adapter returned load progress for the wrong deployment",
                    )
                    .await;
                } else {
                    self.reply_lifecycle_progress(stage, percent).await;
                }
            }
        }
    }

    fn accepts_loaded_event(&self, deployment: &str, generation: u64) -> bool {
        let lifecycle = self.lifecycle.lock().expect("lifecycle lock").clone();
        let Some(frame) = lifecycle else {
            return false;
        };
        match self.payload.lifecycle(&frame) {
            Some(Work::Load(load)) => {
                load.deployment == deployment
                    && frame
                        .envelope
                        .chain
                        .as_ref()
                        .is_some_and(|chain| chain.current().generation == generation)
            }
            _ => false,
        }
    }

    fn accepts_load_event(&self, deployment: &str) -> bool {
        let lifecycle = self.lifecycle.lock().expect("lifecycle lock").clone();
        let Some(frame) = lifecycle else {
            return false;
        };
        matches!(
            self.payload.lifecycle(&frame),
            Some(Work::Load(load)) if load.deployment == deployment
        )
    }

    fn accepts_unload_event(&self, deployment: &str) -> bool {
        let lifecycle = self.lifecycle.lock().expect("lifecycle lock").clone();
        let Some(frame) = lifecycle else {
            return false;
        };
        match self.payload.lifecycle(&frame) {
            Some(Work::Unload(unload)) => unload.deployment == deployment,
            _ => false,
        }
    }

    fn accepts_sequence_event(&self, deployment: &str, sequence: &str) -> bool {
        let active = self
            .active_status
            .lock()
            .expect("active telemetry lock")
            .clone();
        if active
            .as_ref()
            .is_none_or(|hop| hop.timed_out || hop.deployment != deployment)
        {
            return false;
        }
        self.in_flight
            .lock()
            .expect("in-flight lock")
            .contains_key(sequence)
    }

    /// Fails the load this event claimed to be about.
    ///
    /// The permit is released only when there was a lifecycle carrier holding
    /// it. A load event arriving with none is an orphan — a late report from a
    /// deployment that already finished — and the node may well be running a
    /// hop instead. `finished()` is the node's only mutual exclusion over a
    /// hop, so freeing it here would let `drain()` start a second one beside
    /// the first, and the completion that came back would consume the wrong
    /// in-flight map.
    async fn reject_lifecycle_event(&self, detail: &str) {
        let carrier = self.lifecycle.lock().expect("lifecycle lock").take();
        let Some(carrier) = carrier else {
            self.counts.orphaned.fetch_add(1, Ordering::Relaxed);
            return;
        };
        self.counts.invalid_events.fetch_add(1, Ordering::Relaxed);
        self.clear_event_fence();
        self.queue.finished();
        self.reply_error(&carrier, detail).await;
        self.drain().await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn finish_cache_event(
        &self,
        deployment: String,
        stage_id: String,
        generation: u64,
        operation_id: String,
        sequence: String,
        bytes: u64,
        detail: String,
        state: Option<CacheReceiptState>,
    ) {
        let carrier = self.lifecycle.lock().expect("lifecycle lock").clone();
        let Some(carrier) = carrier else {
            self.counts.invalid_events.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let expected_deployment = self.payload.deployment(&carrier);
        let expected_generation = carrier
            .envelope
            .chain
            .as_ref()
            .map(|chain| chain.current().generation);
        let expected_stage = carrier
            .envelope
            .chain
            .as_ref()
            .map(|chain| chain.current().node.as_str());
        let expected_sequence = match self.payload.lifecycle(&carrier) {
            Some(Work::Cache(cache)) => Some(cache.subject().clone()),
            _ => None,
        };
        let exact = expected_deployment.as_deref() == Some(deployment.as_str())
            && expected_stage == Some(stage_id.as_str())
            && expected_generation == Some(generation)
            && carrier.envelope.request_id == operation_id
            && expected_sequence.as_deref() == Some(sequence.as_str());
        if !exact {
            self.counts.invalid_events.fetch_add(1, Ordering::Relaxed);
            let Some(carrier) = self.lifecycle.lock().expect("lifecycle lock").take() else {
                return;
            };
            self.reply_error(
                &carrier,
                "adapter returned a cache event for the wrong deployment, generation, or operation",
            )
            .await;
            self.queue.finished();
            self.drain().await;
            return;
        }
        let body = match state {
            Some(state) => self.payload.cache_status(
                &deployment,
                &stage_id,
                generation,
                &operation_id,
                &sequence,
                state.as_str(),
                bytes,
                &detail,
            ),
            None => self.payload.cached(
                &deployment,
                &stage_id,
                generation,
                &operation_id,
                &sequence,
                bytes,
                &detail,
            ),
        };
        self.finish_lifecycle(body).await
    }

    async fn finish_lifecycle(&self, body: Vec<u8>) {
        let carrier = self.lifecycle.lock().expect("lifecycle lock").take();
        self.clear_event_fence();
        self.queue.finished();
        if let Some(carrier) = carrier {
            self.reply(&carrier, body).await;
        }
        self.drain().await;
    }

    pub(super) fn clear_event_fence(&self) {
        *self
            .active_event_token
            .lock()
            .expect("active event token lock") = None;
    }
}

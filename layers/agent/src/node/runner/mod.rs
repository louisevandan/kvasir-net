//! The node's event loop.
//!
//! It advances on work arriving, a hop ending, or an explicit deadline
//! watchdog. The watchdog only fences and reports an expired hop; adapter
//! cancellation remains cooperative at the boundary.

pub mod bound;
pub mod events;
pub mod handle;

pub use handle::{ActiveHop, Counts, Handle, WaitingRequest};

use bound::Bound;
use events::{RaisedEvent, Sink};

use crate::node::payload::Payload;
use crate::node::queue::NodeQueue;
use crate::node::window::{compose, expired_items};
use crate::queue::main::Sender;
use p4_adapter::{Adapter, Hop, Phase, Work};
use p4_protocol::QueueClass;
use p4_protocol::frame::Frame;
use std::collections::{HashMap, HashSet};
use std::future::pending;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;
use tokio::sync::{Mutex as AsyncMutex, mpsc, watch};

const BLOCKING_EXECUTOR_LIMIT: usize = 64;

fn blocking_executor() -> Arc<Semaphore> {
    static LIMIT: OnceLock<Arc<Semaphore>> = OnceLock::new();
    Arc::clone(LIMIT.get_or_init(|| Arc::new(Semaphore::new(BLOCKING_EXECUTOR_LIMIT))))
}

pub struct Node {
    queue: Arc<NodeQueue>,
    adapter: Arc<dyn Adapter>,
    payload: Arc<dyn Payload>,
    /// What the load declared. A ceiling: never derived here, never exceeded.
    ceiling: Mutex<usize>,
    /// Frames of the hop in flight, keyed by the sequence id they were given,
    /// so an outcome can be matched back to the route it came from.
    in_flight: Mutex<HashMap<String, Frame>>,
    /// Native staged adapters report which routes occupy backend sequence
    /// slots. The node keeps new prefills queued while those slots are full;
    /// adapters that do not emit the optional events retain the old behavior.
    active_sequences: Mutex<HashSet<String>>,
    /// The adapter execution currently represented by `in_flight`.
    active_hop: Mutex<Option<u64>>,
    active_status: Arc<Mutex<Option<ActiveHop>>>,
    active_cancel: Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>,
    /// Private provenance fence for adapter callbacks. This is separate from
    /// the P4 deployment generation and does not change the public wire. It
    /// is intentionally not a process-restart identity: restart continuity
    /// requires a future host/process identity contract outside this runner.
    active_event_token: Mutex<Option<u64>>,
    timed_out: Mutex<HashSet<u64>>,
    next_hop: AtomicU64,
    next_event_token: AtomicU64,
    /// The load or unload in flight, if any. Kept apart from `in_flight`
    /// because it belongs to a deployment rather than to a sequence.
    lifecycle: Mutex<Option<Frame>>,
    /// Whether this node may serve, and for which deployment.
    bound: Mutex<Bound>,
    events: Sink,
    counts: Arc<Counts>,
    /// Frames on their way to the agent queue. Bounded so a slow downstream
    /// lane applies backpressure all the way to this node's event loop.
    outbox: mpsc::Sender<Frame>,
    /// Serializes output admission with pump shutdown so a producer cannot
    /// enqueue a frame after the pump has finished accounting its remainder.
    outbox_gate: Arc<AsyncMutex<bool>>,
    /// A separate watch used only after the runner's bounded shutdown grace
    /// expires, so teardown replies get a normal chance to enter the outbox.
    outbox_stop: watch::Receiver<bool>,
}

impl Node {
    /// Starts the node and returns the handle used to feed it.
    /// What the backend behind this node says it is doing.
    ///
    /// Relayed, never read. The core has no idea what the string means, which
    /// is the same arrangement a plan has going the other way.
    pub fn report(&self) -> String {
        self.adapter.report()
    }

    pub fn spawn(
        adapter: Arc<dyn Adapter>,
        payload: Arc<dyn Payload>,
        out: Sender,
        ceiling: usize,
    ) -> Handle {
        Self::spawn_with_capacity(adapter, payload, out, ceiling, 4096)
    }

    pub fn spawn_with_capacity(
        adapter: Arc<dyn Adapter>,
        payload: Arc<dyn Payload>,
        out: Sender,
        ceiling: usize,
        max_queue_depth: usize,
    ) -> Handle {
        let (work_tx, work_rx) = mpsc::channel(max_queue_depth.max(1));
        let (stop_tx, stop_rx) = watch::channel(false);
        let (done_tx, _done_rx) = watch::channel(false);
        let (outbox_done_tx, _outbox_done_rx) = watch::channel(false);
        let (outbox_stop_tx, outbox_stop_rx) = watch::channel(false);
        let admission_closed = Arc::new(Mutex::new(false));
        let outbox_gate = Arc::new(AsyncMutex::new(false));
        let capacity = max_queue_depth.max(1);
        let (event_tx, event_rx) = mpsc::channel(capacity);
        let (outbox_tx, mut outbox_rx) = mpsc::channel::<Frame>(capacity);
        let queue = Arc::new(NodeQueue::with_capacity(max_queue_depth));
        let reporting = Arc::clone(&adapter);
        let counts = Arc::new(Counts::default());
        let outbox_lost = Arc::clone(&counts.outbox_lost);
        let outbox_done = outbox_done_tx.clone();
        let queued = out;
        let node = Node {
            queue: Arc::clone(&queue),
            adapter,
            payload,
            ceiling: Mutex::new(ceiling.max(1)),
            in_flight: Mutex::new(HashMap::new()),
            active_sequences: Mutex::new(HashSet::new()),
            active_hop: Mutex::new(None),
            active_status: Arc::new(Mutex::new(None)),
            active_cancel: Mutex::new(None),
            active_event_token: Mutex::new(None),
            timed_out: Mutex::new(HashSet::new()),
            next_hop: AtomicU64::new(1),
            next_event_token: AtomicU64::new(1),
            lifecycle: Mutex::new(None),
            bound: Mutex::new(Bound::Never),
            events: Sink::new(
                event_tx,
                Arc::clone(&counts.raised),
                Arc::clone(&counts.lost),
            ),
            counts: Arc::clone(&counts),
            outbox: outbox_tx,
            outbox_gate: Arc::clone(&outbox_gate),
            outbox_stop: outbox_stop_rx.clone(),
        };
        tokio::spawn(async move {
            // Waits for room rather than dropping. A node that outruns its
            // agent is held here, which slows its next hop without ever
            // blocking the task that has to see that hop end.
            let mut stop = outbox_stop_rx;
            while let Some(frame) = outbox_rx.recv().await {
                let delivered = tokio::select! {
                    result = queued.send(frame) => result.is_ok(),
                    changed = stop.changed() => {
                        if changed.is_ok() && *stop.borrow() {
                            let mut closed = outbox_gate.lock().await;
                            *closed = true;
                            outbox_lost.fetch_add(1, Ordering::Relaxed);
                            while outbox_rx.try_recv().is_ok() {
                                outbox_lost.fetch_add(1, Ordering::Relaxed);
                            }
                            let _ = outbox_done.send(true);
                            return;
                        }
                        true
                    }
                };
                if !delivered {
                    let mut closed = outbox_gate.lock().await;
                    *closed = true;
                    outbox_lost.fetch_add(1, Ordering::Relaxed);
                    while outbox_rx.try_recv().is_ok() {
                        outbox_lost.fetch_add(1, Ordering::Relaxed);
                    }
                    let _ = outbox_done.send(true);
                    return;
                }
            }
            let mut closed = outbox_gate.lock().await;
            *closed = true;
            let _ = outbox_done.send(true);
        });
        let active = Arc::clone(&node.active_status);
        tokio::spawn(node.run(work_rx, event_rx, stop_rx, done_tx.clone()));
        Handle::new(
            work_tx,
            queue,
            counts,
            reporting,
            active,
            admission_closed,
            stop_tx,
            done_tx,
            outbox_done_tx,
            outbox_stop_tx,
        )
    }

    async fn run(
        self,
        mut work: mpsc::Receiver<Frame>,
        mut events: mpsc::Receiver<RaisedEvent>,
        mut stop: watch::Receiver<bool>,
        done: watch::Sender<bool>,
    ) {
        loop {
            // Deliberately not biased. Preferring events looked right — a hop
            // ending is what frees the node — but a node under load produces
            // an event per hop without pause, and a biased select then never
            // polls arrivals at all. They sat unread in the channel: not in the
            // node's queue, not in the agent's, invisible to every depth
            // reading, and the request behind each one never answered.
            //
            // Fairness costs nothing here. An event still reaches `on_event`
            // on the next turn of the loop, and the hop it reports has already
            // finished by the time it was sent.
            let deadline = self.next_deadline();
            let deadline_wait = async move {
                match deadline {
                    Some(deadline) => {
                        let now = now_unix_ms();
                        tokio::time::sleep(std::time::Duration::from_millis(
                            deadline.saturating_sub(now),
                        ))
                        .await;
                    }
                    None => pending::<()>().await,
                }
            };
            tokio::pin!(deadline_wait);
            tokio::select! {
                changed = stop.changed() => {
                    if changed.is_ok() && *stop.borrow() {
                        self.shutdown().await;
                        let _ = done.send(true);
                        return;
                    }
                },
                Some(event) = events.recv() => self.on_event(event).await,
                _ = &mut deadline_wait => self.on_deadline().await,
                frame = work.recv() => match frame {
                    Some(frame) => {
                        self.counts.received.fetch_add(1, Ordering::Relaxed);
                        if let Some(refused) = self.refusal(&frame) {
                            self.reply_error(&frame, &refused).await;
                            continue;
                        }
                        if let Some(refused) = self.payload.lifecycle_error(&frame) {
                            self.reply_error(&frame, &refused).await;
                            continue;
                        }
                        if !self.queue.push(frame.clone()) {
                            self.reply_error(&frame, "node queue is full").await;
                            continue;
                        }
                        self.counts.queued.fetch_add(1, Ordering::Relaxed);
                        self.drain().await;
                    }
                    // The handle is gone: this node was deleted or replaced,
                    // and nothing can reach it again. Returning is what frees
                    // it — the node holds its own event sender, so waiting for
                    // that channel to close waits forever. Written as a
                    // disabled `Some(...)` branch it parked here instead,
                    // keeping the adapter, the queue and the in-flight map for
                    // the life of the process. Every replaced node was still
                    // resident; the count only ever went up.
                    None => {
                        self.shutdown().await;
                        let _ = done.send(true);
                        return;
                    }
                },
                else => {
                    self.shutdown().await;
                    let _ = done.send(true);
                    return;
                }
            }
        }
    }

    async fn shutdown(&self) {
        if let Some(cancel) = self
            .active_cancel
            .lock()
            .expect("active cancellation lock")
            .take()
        {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }

        let mut carriers = self.queue.drain();
        let active = {
            let mut in_flight = self.in_flight.lock().expect("in-flight lock");
            std::mem::take(&mut *in_flight)
                .into_values()
                .collect::<Vec<_>>()
        };
        carriers.extend(active);
        if let Some(frame) = self.lifecycle.lock().expect("lifecycle lock").take() {
            carriers.push(frame);
        }

        for carrier in carriers {
            self.reply_error(&carrier, "node removed before request completed")
                .await;
        }
        *self.active_hop.lock().expect("active hop lock") = None;
        *self.active_status.lock().expect("active status lock") = None;
        self.clear_event_fence();
    }

    fn next_deadline(&self) -> Option<u64> {
        if !self.timed_out.lock().expect("timeout lock").is_empty() {
            return None;
        }
        self.in_flight
            .lock()
            .expect("in-flight lock")
            .values()
            .filter_map(|frame| {
                (frame.envelope.deadline_unix_ms > 0).then_some(frame.envelope.deadline_unix_ms)
            })
            .min()
    }

    async fn on_deadline(&self) {
        let Some(hop_id) = *self.active_hop.lock().expect("active hop lock") else {
            return;
        };
        if self
            .timed_out
            .lock()
            .expect("timeout lock")
            .contains(&hop_id)
        {
            return;
        }
        let now = now_unix_ms();
        let expired = self
            .in_flight
            .lock()
            .expect("in-flight lock")
            .values()
            .any(|frame| {
                frame.envelope.deadline_unix_ms > 0 && frame.envelope.deadline_unix_ms <= now
            });
        if !expired {
            return;
        }
        self.timed_out.lock().expect("timeout lock").insert(hop_id);
        if let Some(active) = self
            .active_status
            .lock()
            .expect("active telemetry lock")
            .as_mut()
            && active.id == hop_id
        {
            active.timed_out = true;
        }
        if let Some(cancel) = self.active_cancel.lock().expect("cancel lock").as_ref() {
            cancel.store(true, Ordering::Relaxed);
        }
        let frames = self
            .in_flight
            .lock()
            .expect("in-flight lock")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for frame in frames {
            self.reply_error(&frame, "deadline passed while this hop was in flight")
                .await;
        }
    }

    /// Why this frame must not run here, if it must not.
    ///
    /// Only sequence work is checked. Lifecycle is how a node stops being
    /// refused, so a load that had to pass the check to be allowed to fix the
    /// thing the check is complaining about could never succeed.
    fn refusal(&self, frame: &Frame) -> Option<String> {
        let generation = frame.envelope.chain.as_ref()?.current().generation;
        if let Some(work) = self.payload.lifecycle(frame) {
            return match work {
                // These operations establish or remove the binding and must
                // be admitted before the normal generation gate can apply.
                Work::Load(_) | Work::Unload(_) => None,
                // A cache operation is lifecycle-shaped for scheduling, but
                // it is still work against an already bound deployment.
                Work::Cache(cache) => {
                    let bound = *self.bound.lock().expect("generation lock");
                    (!bound.admits(cache.generation)).then(|| bound.why(cache.generation))
                }
                Work::Hop(_) => None,
            };
        }
        let bound = *self.bound.lock().expect("generation lock");
        (!bound.admits(generation)).then(|| bound.why(generation))
    }

    fn staged_prefill_slots(&self, ceiling: usize) -> usize {
        let active = self.active_sequences.lock().expect("active sequence lock");
        if std::env::var_os("P4_AGENT_TRACE_SEQUENCE").is_some() {
            eprintln!(
                "P4_AGENT_SEQUENCE_CAPACITY active={} ceiling={} available={}",
                active.len(),
                ceiling,
                ceiling.saturating_sub(active.len())
            );
        }
        ceiling.saturating_sub(active.len())
    }

    /// Starts a hop if one can start. Called after every event, which is what
    /// makes progress event-driven rather than timed.
    async fn drain(&self) {
        loop {
            if self.queue.is_running() {
                return;
            }
            let now = now_unix_ms();
            for stale in expired_items(&self.queue.waiting(), now) {
                if let Some(frame) = self.queue.remove(&stale.route) {
                    self.reply_error(&frame, "deadline passed before this work started")
                        .await;
                }
            }
            if self.start_lifecycle() {
                return;
            }
            let ceiling = *self.ceiling.lock().expect("ceiling lock");
            for frame in self.queue.frames() {
                if let Some(reason) = self.refusal(&frame)
                    && let Some(stale) = self.queue.remove(&frame.envelope.route)
                {
                    self.reply_error(&stale, &reason).await;
                }
            }
            let waiting = self.queue.frames();
            let lifecycle = waiting
                .iter()
                .find(|frame| {
                    matches!(
                        self.payload.lifecycle(frame),
                        Some(Work::Load(_)) | Some(Work::Cache(_))
                    )
                })
                .or_else(|| {
                    waiting
                        .iter()
                        .find(|frame| self.payload.lifecycle(frame).is_some())
                });
            let scheduling = if let Some(frame) = lifecycle {
                if !matches!(self.payload.lifecycle(frame), Some(Work::Unload(_))) {
                    return;
                }
                waiting
                    .iter()
                    .filter(|candidate| self.payload.sequence(candidate).is_some())
                    .map(|candidate| crate::node::window::Waiting {
                        route: candidate.envelope.route.clone(),
                        lane: candidate.envelope.lane,
                        deadline_unix_ms: candidate.envelope.deadline_unix_ms,
                    })
                    .collect::<Vec<_>>()
            } else {
                self.queue.waiting()
            };
            // The admission check must account for the whole candidate
            // window, not evaluate every queued prefill against the same
            // snapshot.  If one native sequence slot is free and four new
            // prefills are waiting, only one may pass this filter.  The old
            // per-item predicate admitted all four, then `compose` truncated
            // nothing until after the queue had already exceeded the adapter's
            // sequence table.
            let mut staged_prefill_slots = self.staged_prefill_slots(ceiling);
            let scheduling = scheduling
                .into_iter()
                .filter(|item| {
                    if item.lane != QueueClass::Prefill {
                        return true;
                    }
                    if staged_prefill_slots == 0 {
                        return false;
                    }
                    staged_prefill_slots -= 1;
                    true
                })
                .collect::<Vec<_>>();
            let Some(window) = compose(&scheduling, ceiling, now) else {
                return;
            };
            let routes: Vec<String> = window.items.iter().map(|item| item.route.clone()).collect();
            let claimed = self.queue.claim(&routes);
            if claimed.is_empty() {
                return;
            }
            self.counts
                .claimed
                .fetch_add(claimed.len(), Ordering::Relaxed);
            match self.hop(&claimed, window.lane) {
                Some(hop) => {
                    *self.active_hop.lock().expect("active hop lock") = Some(hop.id);
                    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    *self.active_cancel.lock().expect("cancel lock") = Some(Arc::clone(&cancelled));
                    let mut in_flight = self.in_flight.lock().expect("in-flight lock");
                    for (frame, sequence) in claimed.iter().zip(hop.sequences.iter()) {
                        in_flight.insert(sequence.sequence.clone(), frame.clone());
                    }
                    drop(in_flight);
                    if self.adapter.reserves_sequence_slots() && hop.phase == Phase::Prefill {
                        let mut active =
                            self.active_sequences.lock().expect("active sequence lock");
                        active.extend(
                            hop.sequences
                                .iter()
                                .map(|sequence| sequence.sequence.clone()),
                        );
                        if std::env::var_os("P4_AGENT_TRACE_SEQUENCE").is_some() {
                            eprintln!(
                                "P4_AGENT_SEQUENCE_RESERVE routes={} active={} ceiling={}",
                                hop.sequences
                                    .iter()
                                    .map(|sequence| sequence.sequence.as_str())
                                    .collect::<Vec<_>>()
                                    .join(","),
                                active.len(),
                                ceiling
                            );
                        }
                    }
                    *self.active_status.lock().expect("active telemetry lock") = Some(ActiveHop {
                        id: hop.id,
                        deployment: hop.deployment.clone(),
                        phase: hop.phase,
                        timed_out: false,
                        requests: claimed
                            .iter()
                            .map(|frame| WaitingRequest {
                                route: frame.envelope.route.clone(),
                                request_id: frame.envelope.request_id.clone(),
                                stream_id: frame.envelope.stream_id.clone(),
                                lane: frame.envelope.lane,
                                deadline_unix_ms: frame.envelope.deadline_unix_ms,
                            })
                            .collect(),
                    });
                    // An adapter is a procedure and is allowed to block — a real
                    // one waits on a device. Running it on a blocking thread is
                    // what keeps that from stalling the workers that still have to
                    // relay and answer while this node is busy.
                    let adapter = Arc::clone(&self.adapter);
                    let event_token = self.begin_event_fence();
                    let events = self.events.for_hop(event_token, cancelled);
                    let permit = blocking_executor();
                    self.counts.hops.fetch_add(1, Ordering::Relaxed);
                    tokio::spawn(async move {
                        let Ok(permit) = permit.acquire_owned().await else {
                            return;
                        };
                        let _ = tokio::task::spawn_blocking(move || {
                            adapter.start(Work::Hop(hop), &events);
                            drop(permit);
                        })
                        .await;
                    });
                }
                None => {
                    self.queue.finished();
                    for frame in claimed {
                        self.reply_error(&frame, "work could not be read as a sequence")
                            .await;
                    }
                }
            }
        }
    }

    pub(super) fn clear_active_telemetry(&self) {
        *self.active_status.lock().expect("active telemetry lock") = None;
        *self.active_hop.lock().expect("active hop lock") = None;
        self.clear_event_fence();
    }

    pub(super) fn release_active_sequences(&self, sequences: &HashSet<String>) {
        self.active_sequences
            .lock()
            .expect("active sequence lock")
            .retain(|active| !sequences.contains(active));
    }

    fn begin_event_fence(&self) -> u64 {
        let token = self.next_event_token.fetch_add(1, Ordering::Relaxed);
        *self
            .active_event_token
            .lock()
            .expect("active event token lock") = Some(token);
        token
    }

    /// Runs a waiting load or unload, alone.
    ///
    /// Lifecycle never shares a hop: materialising or releasing a model is one
    /// instruction about a whole deployment, and batching it beside sequences
    /// would let execution start against something half-built.
    fn start_lifecycle(&self) -> bool {
        let frames = self.queue.frames();
        let Some(frame) = frames
            .iter()
            .find(|frame| {
                matches!(
                    self.payload.lifecycle(frame),
                    Some(Work::Load(_)) | Some(Work::Cache(_))
                )
            })
            .or_else(|| {
                frames
                    .iter()
                    .find(|frame| self.payload.lifecycle(frame).is_some())
            })
        else {
            return false;
        };
        let Some(work) = self.payload.lifecycle(frame) else {
            return false;
        };
        if matches!(work, Work::Unload(_))
            && self.queue.frames().iter().any(|candidate| {
                candidate.envelope.route != frame.envelope.route
                    && self.payload.sequence(candidate).is_none()
            })
        {
            return false;
        }
        if matches!(work, Work::Unload(_))
            && self.queue.frames().iter().any(|candidate| {
                candidate.envelope.route != frame.envelope.route
                    && self.payload.sequence(candidate).is_some()
            })
        {
            return false;
        }
        let route = frame.envelope.route.clone();
        let claimed = self.queue.claim(&[route]);
        if claimed.is_empty() {
            return false;
        }
        if let Some(ceiling) = self.payload.ceiling(frame) {
            *self.ceiling.lock().expect("ceiling lock") = ceiling.max(1);
        }
        *self.lifecycle.lock().expect("lifecycle lock") = Some(frame.clone());
        let adapter = Arc::clone(&self.adapter);
        let event_token = self.begin_event_fence();
        let events = self.events.for_operation(event_token);
        let permit = blocking_executor();
        tokio::spawn(async move {
            let Ok(permit) = permit.acquire_owned().await else {
                return;
            };
            let _ = tokio::task::spawn_blocking(move || {
                adapter.start(work, &events);
                drop(permit);
            })
            .await;
        });
        true
    }

    fn hop(&self, claimed: &[Frame], lane: QueueClass) -> Option<Hop> {
        let deployment = self.payload.deployment(claimed.first()?)?;
        let sequences: Vec<_> = claimed
            .iter()
            .filter_map(|frame| self.payload.sequence(frame))
            .collect();
        if sequences.len() != claimed.len() {
            return None;
        }
        Some(Hop {
            id: self.next_hop.fetch_add(1, Ordering::Relaxed),
            deployment,
            phase: match lane {
                QueueClass::Decode => Phase::Decode,
                _ => Phase::Prefill,
            },
            sequences,
        })
    }

    async fn reply(&self, carrier: &Frame, body: Vec<u8>) {
        let Some(envelope) = carrier.envelope.to_reply() else {
            return;
        };
        // Through the outbox, like every other frame a node produces. A reply
        // that took the direct path would be the one thing this node can still
        // lose to a full lane.
        self.emit(Frame { envelope, body }).await;
    }

    /// Hands a frame to this node's outbox.
    ///
    /// The outbox is drained by a task of its own, which waits for room on the
    /// agent queue. That waiting is backpressure and belongs somewhere — but
    /// not here: this is the same task that receives hop completions, and a
    /// node blocked mid-emit could not observe the hop it is waiting on. The
    /// outbox is the seam that keeps a full lane from becoming a stall.
    async fn emit(&self, frame: Frame) {
        self.counts.emitted.fetch_add(1, Ordering::Relaxed);
        if *self.outbox_gate.lock().await {
            self.counts.outbox_lost.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut stop = self.outbox_stop.clone();
        let sent = tokio::select! {
            result = self.outbox.send(frame) => result.is_ok(),
            changed = stop.changed() => !(changed.is_ok() && *stop.borrow()),
        };
        if !sent {
            self.counts.outbox_lost.fetch_add(1, Ordering::Relaxed);
        }
    }

    async fn reply_error(&self, carrier: &Frame, detail: &str) {
        let body = match self.payload.lifecycle(carrier) {
            Some(Work::Cache(_)) => self.payload.cache_failure(carrier, detail),
            _ => self.payload.failure(detail),
        };
        self.reply(carrier, body).await;
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;

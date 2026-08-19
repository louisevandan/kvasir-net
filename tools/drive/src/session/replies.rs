//! What came back, per route.
//!
//! Separate from the steps that ask, because it changes for a different reason:
//! this file follows the reply vocabulary, while its parent follows the shape
//! of a run. It is also the only thing here touched by another thread — every
//! reply lands on the agent's own task — so keeping the shared state in one
//! file is what makes "who holds which lock" a question with a short answer.

use super::watch::Peaks;
use crate::telemetry::{TelemetryCollector, model::TelemetryEvidence};
use p4_agent_core::agent::{Agent, Duties};
use p4_protocol::frame::Frame;
use p4_service::message::Reply;
use p4_service::message::wire::decode_reply;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// What a route produced.
#[derive(Default, Clone, Debug)]
pub struct Stream {
    pub request_id: String,
    pub stream_id: String,
    pub return_channel: String,
    pub tokens: Vec<u32>,
    /// What the tokens actually said, kept so a run can show an answer rather
    /// than only count one. Four passing verdicts are equally consistent with
    /// every token being empty, which is a failure a real backend has already
    /// produced twice here.
    pub text: String,
    pub done: Option<u32>,
    pub failed: Option<String>,
    pub progress: usize,
    pub bound: bool,
    pub bound_generation: Option<u64>,
    pub released: bool,
    pub cached_sequence: Option<String>,
    pub accepted: bool,
    /// Last wire event observed for this stream. Zero is reserved for legacy
    /// fixtures and control replies; inference responses start at one.
    pub last_event_seq: u64,
    pub duplicate_events: usize,
    pub sequence_gaps: usize,
    /// Monotonic request-to-terminal latency captured by the drive. This is
    /// local evidence and is not part of the P4 wire contract.
    pub latency_us: Option<u64>,
    started_at: Option<Instant>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CachedRecord {
    pub deployment: String,
    pub stage_id: String,
    pub generation: u64,
    pub operation_id: String,
    pub sequence: String,
}

const CACHE_ROUTE_TOMBSTONES: usize = 4096;

#[derive(Default)]
struct CacheRouteTombstones {
    order: VecDeque<String>,
    seen: HashSet<String>,
}

impl CacheRouteTombstones {
    fn contains(&self, route: &str) -> bool {
        self.seen.contains(route)
    }

    fn insert(&mut self, route: String) {
        if !self.seen.insert(route.clone()) {
            return;
        }
        self.order.push_back(route);
        while self.order.len() > CACHE_ROUTE_TOMBSTONES {
            if let Some(evicted) = self.order.pop_front() {
                self.seen.remove(&evicted);
            }
        }
    }
}

impl Stream {
    pub fn is_finished(&self) -> bool {
        self.done.is_some() || self.failed.is_some()
    }

    /// Whether the tokens arrived in the order they were produced. The one
    /// thing a caller cannot check any other way.
    pub fn is_ordered(&self) -> bool {
        // Token IDs are vocabulary indices and have no monotonic relationship
        // to generation order (for example, a perfectly valid stream can be
        // [151645, 198, 42]).  Ordering is carried by the response envelope;
        // duplicate or gapped event sequences are the actual wire-order
        // failure. Legacy fixtures with event_seq=0 remain accepted because
        // they deliberately carry no ordering evidence.
        self.duplicate_events == 0 && self.sequence_gaps == 0
    }
}

#[derive(Default, Clone)]
pub struct Replies {
    pub(super) streams: Arc<Mutex<HashMap<String, Stream>>>,
    /// Counted as replies land, so waiting on progress never has to walk the
    /// streams. Polling by cloning them held the same lock the recording path
    /// needs, and got slower as the tokens it was counting accumulated — the
    /// measurement starving the thing it measured.
    pub(super) finished: Arc<AtomicUsize>,
    pub(super) bound: Arc<AtomicUsize>,
    pub(super) accepted: Arc<AtomicUsize>,
    pub(super) released: Arc<AtomicUsize>,
    pub(super) cached: Arc<AtomicUsize>,
    /// Cache completions grouped by their logical operation, never by a
    /// process-wide counter. A stage may complete late or twice; the caller
    /// compares the deduplicated exact set before declaring success.
    pub(super) cached_operations: Arc<Mutex<HashMap<String, Vec<CachedRecord>>>>,
    /// Cache replies are indexed by the exact outbound route. The route is
    /// the wave/stage fence; operation_id alone is intentionally insufficient
    /// because late replies from an earlier phase may share that identity.
    pub(super) cache_replies: Arc<Mutex<HashMap<String, Reply>>>,
    /// A route that receives two cache replies is poisoned rather than
    /// allowing last-write-wins to choose an arbitrary phase/result.
    duplicate_cache_routes: Arc<Mutex<HashSet<String>>>,
    /// Recently consumed routes reject late duplicates without unbounded
    /// growth. The bounded window is deliberate: it protects a long-lived
    /// session from stale replay while keeping memory usage finite.
    consumed_cache_routes: Arc<Mutex<CacheRouteTombstones>>,
    /// Discovery replies are separate from inference streams so a preflight
    /// can verify every selected agent before creating nodes.
    pub(super) models: Arc<Mutex<HashMap<String, Reply>>>,
    /// Every reply that is progress. What waiting is bounded by: a driver
    /// cannot know how fast a backend is, but it can tell a deployment that is
    /// slow from one that has stopped, and only the second is worth giving up
    /// on. Answers to the driver's own questions are excluded — counting them
    /// would let a dead deployment look busy because we were still asking it
    /// how it was doing.
    pub(super) events: Arc<AtomicUsize>,
    /// The deepest the queues got while the run was in flight.
    pub(super) peaks: Peaks,
    pub(super) telemetry: TelemetryCollector,
}

impl Replies {
    pub(super) fn begin_request(&self, route: &str, return_channel: &str) {
        let key = correlation_key_for_route(route, return_channel);
        let mut streams = self.streams.lock().expect("reply lock");
        let stream = streams.entry(key).or_default();
        stream.request_id = route.to_owned();
        stream.stream_id = route.to_owned();
        stream.return_channel = return_channel.to_owned();
        stream.started_at = Some(Instant::now());
    }

    pub(super) fn telemetry(&self, elapsed: std::time::Duration) -> TelemetryEvidence {
        self.telemetry.evidence(elapsed)
    }
}

impl Duties for Replies {
    fn handle(&self, frame: Frame, _: &Arc<Agent>) {
        let Ok(reply) = decode_reply(&frame.body) else {
            return;
        };
        if std::env::var("P4_DRIVE_TRACE").is_ok_and(|value| value != "0") {
            eprintln!(
                "P4_DRIVE_TRACE reply route={} request={} reply={reply:?}",
                frame.envelope.route, frame.envelope.request_id
            );
            use std::io::Write;
            let _ = std::io::stderr().flush();
        }
        // Facts about a machine and what an agent is doing: each is asked for
        // deliberately, and neither belongs to a route. They are handled here
        // rather than filed under one, because filing them would invent a
        // route that produced nothing and count it among the run's.
        match &reply {
            Reply::Status { snapshot } => return self.peaks.observe(snapshot),
            Reply::StatusSnapshot { snapshot } => {
                self.peaks.observe(&snapshot.to_string());
                self.telemetry.observe(snapshot);
                return;
            }
            Reply::Machine { .. } => return,
            Reply::Model { .. } => {
                self.models
                    .lock()
                    .expect("model lock")
                    .insert(correlation_key(&frame), reply);
                return;
            }
            _ => {}
        }
        self.events.fetch_add(1, SeqCst);
        if matches!(
            reply,
            Reply::Cached { .. } | Reply::CacheFailed { .. } | Reply::CacheStatus { .. }
        ) {
            // Cache replies are consumed by a phase/stage route, but the
            // route alone is not an authenticated operation identity. Reject
            // a foreign operation before it can poison that route's slot.
            let request_id = request_key(&frame);
            if !cache_reply_matches_request(&reply, &request_id) {
                return;
            }
            let route = frame.envelope.route.clone();
            if self
                .consumed_cache_routes
                .lock()
                .expect("consumed cache route lock")
                .contains(&route)
            {
                return;
            }
            let mut duplicates = self
                .duplicate_cache_routes
                .lock()
                .expect("duplicate cache route lock");
            if duplicates.contains(&route) {
                return;
            }
            let mut cache_replies = self.cache_replies.lock().expect("cache reply lock");
            if cache_replies.contains_key(&route) {
                cache_replies.remove(&route);
                duplicates.insert(route);
                return;
            }
            cache_replies.insert(route, reply.clone());
        }
        let mut streams = self.streams.lock().expect("reply lock");
        let stream = streams.entry(correlation_key(&frame)).or_default();
        if stream.request_id.is_empty() {
            stream.request_id = request_key(&frame);
            stream.stream_id = stream_identity(&frame);
            stream.return_channel = channel_identity(&frame);
        } else if stream.request_id != request_key(&frame)
            || stream.stream_id != stream_identity(&frame)
            || stream.return_channel != channel_identity(&frame)
        {
            stream.duplicate_events += 1;
            return;
        }
        if frame.envelope.event_seq > 0 {
            if frame.envelope.event_seq <= stream.last_event_seq {
                stream.duplicate_events += 1;
                return;
            }
            if stream.last_event_seq > 0
                && frame.envelope.event_seq != stream.last_event_seq.saturating_add(1)
            {
                stream.sequence_gaps += 1;
            }
            stream.last_event_seq = frame.envelope.event_seq;
        }
        if let Reply::Cached {
            deployment,
            stage_id,
            generation,
            operation_id,
            sequence,
            ..
        } = &reply
        {
            self.cached_operations
                .lock()
                .expect("cache operation lock")
                .entry(operation_id.clone())
                .or_default()
                .push(CachedRecord {
                    deployment: deployment.clone(),
                    stage_id: stage_id.clone(),
                    generation: *generation,
                    operation_id: operation_id.clone(),
                    sequence: sequence.clone(),
                });
        }
        match reply {
            Reply::Token { index, text } => {
                stream.tokens.push(index);
                stream.text.push_str(&text);
            }
            Reply::Done {
                generated,
                final_token,
                ..
            } => {
                if let Some((index, text)) = final_token {
                    stream.tokens.push(index);
                    stream.text.push_str(&text);
                }
                stream.done = Some(generated);
                stream.latency_us = stream
                    .started_at
                    .map(|started| started.elapsed().as_micros().min(u64::MAX as u128) as u64);
                self.finished.fetch_add(1, SeqCst);
            }
            Reply::Failed { detail } => {
                stream.failed = Some(detail);
                stream.latency_us = stream
                    .started_at
                    .map(|started| started.elapsed().as_micros().min(u64::MAX as u128) as u64);
                self.finished.fetch_add(1, SeqCst);
            }
            Reply::CacheFailed { detail, .. } => {
                stream.failed = Some(detail);
                stream.latency_us = stream
                    .started_at
                    .map(|started| started.elapsed().as_micros().min(u64::MAX as u128) as u64);
                self.finished.fetch_add(1, SeqCst);
            }
            Reply::Progress { .. } => stream.progress += 1,
            Reply::Bound { generation } => {
                stream.bound = true;
                stream.bound_generation = Some(generation);
                self.bound.fetch_add(1, SeqCst);
            }
            Reply::Released => {
                stream.released = true;
                self.released.fetch_add(1, SeqCst);
            }
            Reply::Accepted { .. } => {
                stream.accepted = true;
                self.accepted.fetch_add(1, SeqCst);
            }
            // What became of a cached sequence is asked for deliberately and
            // read where it was asked for, not here.
            Reply::Cached { sequence, .. } => {
                stream.cached_sequence = Some(sequence);
                self.cached.fetch_add(1, SeqCst);
            }
            // Returned above. Left as a quiet arm rather than a panic: this
            // runs on the agent's own task, where an unexpected reply must not
            // be able to take the driver down.
            Reply::Machine { .. }
            | Reply::Model { .. }
            | Reply::Status { .. }
            | Reply::StatusSnapshot { .. }
            | Reply::CacheStatus { .. } => {}
        }
    }
}

impl Replies {
    #[cfg(test)]
    pub(super) fn is_cache_route_poisoned(&self, route: &str) -> bool {
        self.duplicate_cache_routes
            .lock()
            .expect("duplicate cache route lock")
            .contains(route)
    }

    #[cfg(test)]
    pub(super) fn has_consumed_cache_route(&self, route: &str) -> bool {
        self.consumed_cache_routes
            .lock()
            .expect("consumed cache route lock")
            .contains(route)
    }

    pub(super) fn has_cache_reply(&self, route: &str) -> bool {
        self.duplicate_cache_routes
            .lock()
            .expect("duplicate cache route lock")
            .contains(route)
            || self
                .cache_replies
                .lock()
                .expect("cache reply lock")
                .contains_key(route)
    }

    pub(super) fn take_cache_reply(&self, route: &str) -> Option<Reply> {
        if self
            .duplicate_cache_routes
            .lock()
            .expect("duplicate cache route lock")
            .remove(route)
        {
            self.consumed_cache_routes
                .lock()
                .expect("consumed cache route lock")
                .insert(route.to_owned());
            return None;
        }
        let reply = self
            .cache_replies
            .lock()
            .expect("cache reply lock")
            .remove(route);
        if reply.is_some() {
            self.consumed_cache_routes
                .lock()
                .expect("consumed cache route lock")
                .insert(route.to_owned());
        }
        reply
    }
}

pub(super) fn correlation_key_for_route(route: &str, channel: &str) -> String {
    format!("{route}|{route}|{channel}")
}

fn correlation_key(frame: &Frame) -> String {
    format!(
        "{}|{}|{}",
        request_key(frame),
        stream_identity(frame),
        channel_identity(frame)
    )
}

fn request_key(frame: &Frame) -> String {
    if frame.envelope.request_id.is_empty() {
        frame.envelope.route.clone()
    } else {
        frame.envelope.request_id.clone()
    }
}

fn cache_operation_id(reply: &Reply) -> Option<&str> {
    match reply {
        Reply::Cached { operation_id, .. }
        | Reply::CacheFailed { operation_id, .. }
        | Reply::CacheStatus { operation_id, .. } => Some(operation_id),
        _ => None,
    }
}

fn cache_reply_matches_request(reply: &Reply, request_id: &str) -> bool {
    cache_operation_id(reply) == Some(request_id)
}

fn stream_identity(frame: &Frame) -> String {
    if frame.envelope.stream_id.is_empty() {
        frame.envelope.route.clone()
    } else {
        frame.envelope.stream_id.clone()
    }
}

fn channel_identity(frame: &Frame) -> String {
    frame.envelope.return_channel.clone().unwrap_or_else(|| {
        frame
            .envelope
            .reply_to
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::{CacheRouteTombstones, cache_operation_id, cache_reply_matches_request};
    use p4_agent_core::agent::Duties;
    use p4_agent_core::node::payload::Payload;
    use p4_agent_core::queue::lane::{Budget, Lanes};
    use p4_protocol::QueueClass;
    use p4_protocol::envelope::{Address, Envelope, Recipient};
    use p4_protocol::frame;
    use p4_service::message::Reply;
    use std::sync::Arc;

    struct NoopPayload;

    impl Payload for NoopPayload {
        fn sequence(&self, _frame: &p4_protocol::frame::Frame) -> Option<p4_adapter::Sequence> {
            None
        }
    }

    #[test]
    fn cache_reply_operation_identity_is_available_for_every_cache_reply() {
        let cached = Reply::Cached {
            deployment: "d".into(),
            stage_id: "s".into(),
            generation: 1,
            operation_id: "op".into(),
            sequence: "seq".into(),
            bytes: 1,
            detail: "cached".into(),
        };
        let failed = Reply::CacheFailed {
            deployment: "d".into(),
            stage_id: "s".into(),
            generation: 1,
            operation_id: "op".into(),
            sequence: "seq".into(),
            detail: "failed".into(),
        };
        let status = Reply::CacheStatus {
            deployment: "d".into(),
            stage_id: "s".into(),
            generation: 1,
            operation_id: "op".into(),
            sequence: "seq".into(),
            state: "committed".into(),
            bytes: 1,
            detail: "verified".into(),
        };
        assert_eq!(cache_operation_id(&cached), Some("op"));
        assert_eq!(cache_operation_id(&failed), Some("op"));
        assert_eq!(cache_operation_id(&status), Some("op"));
        assert!(cache_reply_matches_request(&status, "op"));
        assert!(!cache_reply_matches_request(&status, "other"));
    }

    #[test]
    fn consumed_cache_routes_are_bounded_tombstones() {
        let mut tombstones = CacheRouteTombstones::default();
        tombstones.insert("first".into());
        assert!(tombstones.contains("first"));
        for index in 0..super::CACHE_ROUTE_TOMBSTONES {
            tombstones.insert(format!("route-{index}"));
        }
        assert!(!tombstones.contains("first"));
        assert!(tombstones.order.len() <= super::CACHE_ROUTE_TOMBSTONES);
    }

    #[test]
    fn poisoned_route_becomes_a_tombstone_when_consumed() {
        let replies = super::Replies::default();
        let route = "cache-commit-s0-1";
        replies.cache_replies.lock().unwrap().insert(
            route.into(),
            Reply::CacheStatus {
                deployment: "d".into(),
                stage_id: "s".into(),
                generation: 1,
                operation_id: "op".into(),
                sequence: "seq".into(),
                state: "committed".into(),
                bytes: 1,
                detail: "ok".into(),
            },
        );
        replies
            .duplicate_cache_routes
            .lock()
            .unwrap()
            .insert(route.into());
        replies.cache_replies.lock().unwrap().remove(route);

        assert!(replies.has_cache_reply(route));
        assert!(replies.take_cache_reply(route).is_none());
        assert!(
            replies
                .consumed_cache_routes
                .lock()
                .unwrap()
                .contains(route)
        );
        assert!(!replies.has_cache_reply(route));
    }

    #[test]
    fn wire_duplicate_replies_poison_the_collector_route() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let own = Address::tcp("127.0.0.1", 52001);
            let replies = Arc::new(super::Replies::default());
            let (agent, _receiver, _in_flight) = p4_agent_core::agent::Agent::new(
                own.clone(),
                replies.clone(),
                Arc::new(NoopPayload),
                Lanes::default(),
                Budget::default(),
            );
            let reply = Reply::CacheStatus {
                deployment: "d".into(),
                stage_id: "s".into(),
                generation: 1,
                operation_id: "op-wire".into(),
                sequence: "seq".into(),
                state: "committed".into(),
                bytes: 1,
                detail: "ok".into(),
            };
            let envelope = Envelope {
                target: own,
                recipient: Recipient::Agent,
                lane: QueueClass::Response,
                route: "wire-cache-route".into(),
                request_id: "op-wire".into(),
                stream_id: "wire-stream".into(),
                origin_agent: None,
                return_channel: Some("outer".into()),
                ingress_generation: 0,
                event_seq: 1,
                deadline_unix_ms: 0,
                reply_to: None,
                chain: None,
            };
            let encoded =
                frame::encode(&envelope, &p4_service::message::wire::encode_reply(&reply)).unwrap();
            let first = frame::decode(&encoded).unwrap();
            let second = frame::decode(&encoded).unwrap();
            <super::Replies as Duties>::handle(&*replies, first, &agent);
            <super::Replies as Duties>::handle(&*replies, second, &agent);

            assert!(replies.has_cache_reply("wire-cache-route"));
            assert!(replies.take_cache_reply("wire-cache-route").is_none());
            assert!(
                replies
                    .consumed_cache_routes
                    .lock()
                    .unwrap()
                    .contains("wire-cache-route")
            );
        });
    }
}

/// What a run is allowed to say afterwards.
#[derive(Default, Debug)]
pub struct Outcome {
    /// How many tokens each unfinished route managed before it stopped. The
    /// shape of this says where a stall is: all zero means work never
    /// started, all near the target means a terminal was lost.
    pub stalled: Vec<usize>,
    pub completed: usize,
    pub failed: usize,
    /// What the first failure said. A count of failures without one of their
    /// reasons is the shape of report that sends an operator to the logs of
    /// every machine in the chain, when the answer was already in hand.
    pub why: Option<String>,
    pub unanswered: usize,
    pub out_of_order: usize,
    pub tokens: usize,
    pub routes: usize,
    /// One monotonic request-to-terminal duration per answered request.
    pub latency_us: Vec<u64>,
    /// One answer, in full. Evidence rather than a verdict: a mock's is
    /// simulated and says nothing, and a real backend's is the only thing that
    /// distinguishes tokens from empty strings that were counted.
    pub sample: String,
    /// The driver stopped waiting because nothing was arriving any more. Said
    /// separately from the verdicts, because "we stopped watching" and "the
    /// deployment stopped working" are different claims, and only the second
    /// is about the thing under test.
    pub quiet: bool,
    /// The deepest a node's own queue got, the most it ever had inside the
    /// adapter at once, the deepest any main-queue lane got, and how many
    /// times these were asked for. All four are needed together: a ceiling
    /// held is only meaningful beside a backlog that existed, and both are
    /// only meaningful if anybody looked.
    pub node_depth: usize,
    pub running: usize,
    pub lane: usize,
    pub samples: usize,
    /// Complete per-request streams retained for evidence output.
    pub streams: Vec<Stream>,
}

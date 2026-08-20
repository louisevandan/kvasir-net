use super::*;
use crate::queue::lane::{Budget, Lanes};
use crate::queue::main::channel;
use p4_adapter::{CacheAction, Distribution, Event, EventSink, Outcome, Work};
use p4_protocol::{Address, Chain, Envelope, Link, Recipient};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver as SyncReceiver, Sender as SyncSender};
use std::time::Duration;

/// Records the width of every hop it is given and answers each one.
struct Recording {
    widths: Arc<Mutex<Vec<usize>>>,
    peak: Arc<AtomicUsize>,
    running: Arc<AtomicUsize>,
    finish_after: usize,
    partial: bool,
}

impl Adapter for Recording {
    fn distribution(&self) -> Distribution {
        Distribution::Staged
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        let Work::Hop(hop) = work else { return };
        self.widths.lock().unwrap().push(hop.width());
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        let mut outcomes: Vec<_> = hop
            .sequences
            .iter()
            .map(|sequence| {
                // This stand-in adapter keeps its own count the way a real one
                // does: in the state P4 hands back to it.
                let lap = sequence
                    .state
                    .as_deref()
                    .and_then(|state| state.first().copied())
                    .unwrap_or(0);
                Outcome {
                    sequence: sequence.sequence.clone(),
                    forward: Some(vec![lap.saturating_add(1)]),
                    text: "t".into(),
                    stop: (u32::from(lap) + 1 >= self.finish_after as u32)
                        .then(|| "stop".to_string()),
                }
            })
            .collect();
        if self.partial {
            outcomes.pop();
        }
        self.running.fetch_sub(1, Ordering::SeqCst);
        events.raise(Event::HopComplete {
            hop_id: hop.id,
            deployment: hop.deployment,
            expected: hop
                .sequences
                .iter()
                .map(|sequence| sequence.sequence.clone())
                .collect(),
            outcomes,
        });
    }
}

struct Bodies;

impl Payload for Bodies {
    fn sequence(&self, frame: &Frame) -> Option<p4_adapter::Sequence> {
        Some(p4_adapter::Sequence {
            sequence: frame.envelope.route.clone(),
            state: None,
            prompt: Some(String::from_utf8_lossy(&frame.body).into_owned()),
            remaining: 4,
            options: "{}".into(),
        })
    }
}

fn link(node: &str, port: u16) -> Link {
    Link {
        address: Address::tcp("127.0.0.1", port),
        node: node.into(),
        binding: "deployment".into(),
        generation: 1,
    }
}

fn work(route: &str, hops: u16) -> Frame {
    let chain = Chain::new(
        (0..hops)
            .map(|i| link(&format!("n{i}"), 52001 + i))
            .collect(),
    )
    .unwrap();
    Frame {
        envelope: Envelope {
            target: chain.current().address.clone(),
            recipient: Recipient::node(chain.current().node.clone()),
            lane: QueueClass::Prefill,
            route: route.into(),
            request_id: route.into(),
            stream_id: route.into(),
            origin_agent: None,
            return_channel: None,
            ingress_generation: 0,
            event_seq: 0,
            deadline_unix_ms: 0,
            reply_to: Some(Address::tcp("10.0.0.1", 19001)),
            chain: Some(chain),
        },
        body: b"prompt".to_vec(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

struct Fixture {
    handle: Handle,
    widths: Arc<Mutex<Vec<usize>>>,
    peak: Arc<AtomicUsize>,
    receiver: crate::queue::main::Receiver,
}

fn fixture_with(ceiling: usize, finish_after: usize, partial: bool) -> Fixture {
    let (sender, receiver, _) = channel(Lanes::default(), Budget::default());
    let widths = Arc::new(Mutex::new(Vec::new()));
    let peak = Arc::new(AtomicUsize::new(0));
    let adapter = Arc::new(Recording {
        widths: Arc::clone(&widths),
        peak: Arc::clone(&peak),
        running: Arc::new(AtomicUsize::new(0)),
        finish_after,
        partial,
    });
    let handle = Node::spawn(adapter, Arc::new(Bodies), sender, ceiling);
    Fixture {
        handle,
        widths,
        peak,
        receiver,
    }
}

fn fixture(ceiling: usize, finish_after: usize) -> Fixture {
    fixture_with(ceiling, finish_after, false)
}

async fn settle() {
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

#[test]
fn a_window_never_exceeds_the_declared_ceiling() {
    runtime().block_on(async {
        let fixture = fixture(4, 1);
        for index in 0..20 {
            let _ = fixture.handle.offer(work(&format!("r{index}"), 1));
        }
        settle().await;

        let widths = fixture.widths.lock().unwrap().clone();
        assert!(!widths.is_empty(), "the node ran at least one hop");
        assert!(
            widths.iter().all(|width| *width <= 4),
            "a hop exceeded the ceiling: {widths:?}"
        );
    });
}

#[test]
fn a_node_runs_one_hop_at_a_time() {
    // Overlapping hops for one deployment is what claiming and marking in a
    // single call exists to prevent.
    runtime().block_on(async {
        let fixture = fixture(4, 1);
        for index in 0..20 {
            let _ = fixture.handle.offer(work(&format!("r{index}"), 1));
        }
        settle().await;
        assert_eq!(fixture.peak.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn every_offered_request_reaches_a_terminal() {
    runtime().block_on(async {
        let mut fixture = fixture(4, 1);
        for index in 0..12 {
            let _ = fixture.handle.offer(work(&format!("r{index}"), 1));
        }
        settle().await;

        let mut terminals = 0;
        while let Ok(frame) = fixture.receiver.take().now_or_never_ok() {
            if frame.envelope.lane == QueueClass::Response {
                terminals += 1;
            }
        }
        assert_eq!(terminals, 12);
    });
}

#[test]
fn a_closed_downstream_queue_counts_outbox_loss() {
    runtime().block_on(async {
        let fixture = fixture(1, 1);
        let Fixture {
            handle, receiver, ..
        } = fixture;
        drop(receiver);
        for index in 0..8 {
            let _ = handle.offer(work(&format!("outbox-loss-{index}"), 1));
        }
        settle().await;
        assert!(
            handle.counts().outbox_lost.load(Ordering::SeqCst) >= 8,
            "a closed downstream queue must account for buffered output loss"
        );
    });
}

#[test]
fn shutdown_is_bounded_when_the_downstream_response_lane_stalls() {
    runtime().block_on(async {
        let (sender, receiver, _) = channel(
            Lanes {
                control: 1,
                prefill: 1,
                decode: 1,
                response: 1,
            },
            Budget::default(),
        );
        let handle = Node::spawn(
            Arc::new(Recording {
                widths: Arc::new(Mutex::new(Vec::new())),
                peak: Arc::new(AtomicUsize::new(0)),
                running: Arc::new(AtomicUsize::new(0)),
                finish_after: 1,
                partial: false,
            }),
            Arc::new(Bodies),
            sender,
            1,
        );
        let _ = handle.offer(work("stalled-0", 1));
        let _ = handle.offer(work("stalled-1", 1));
        tokio::time::sleep(Duration::from_millis(20)).await;

        let result = tokio::time::timeout(Duration::from_secs(4), handle.shutdown()).await;
        assert!(result.is_ok(), "node shutdown must remain bounded");
        drop(receiver);
    });
}

#[test]
fn the_node_empties_and_returns_to_idle() {
    runtime().block_on(async {
        let fixture = fixture(8, 1);
        for index in 0..16 {
            let _ = fixture.handle.offer(work(&format!("r{index}"), 1));
        }
        settle().await;

        assert_eq!(fixture.handle.depth(), 0, "queue drained");
        assert!(!fixture.handle.is_running(), "no hop left running");
    });
}

#[test]
fn a_partial_hop_is_rejected_as_one_terminal_failure_per_sequence() {
    runtime().block_on(async {
        let mut fixture = fixture_with(4, 1, true);
        for index in 0..4 {
            let _ = fixture.handle.offer(work(&format!("partial-{index}"), 1));
        }
        settle().await;

        let mut terminals = 0;
        while let Ok(frame) = fixture.receiver.take().now_or_never_ok() {
            if frame.envelope.lane == QueueClass::Response {
                terminals += 1;
            }
        }
        assert_eq!(terminals, 4, "partial completion must not strand carriers");
        assert!(
            fixture
                .handle
                .counts()
                .invalid_events
                .load(Ordering::SeqCst)
                >= 1,
            "the malformed adapter completion is observable"
        );
        assert_eq!(fixture.handle.depth(), 0);
        assert!(!fixture.handle.is_running());
    });
}

struct WrongCacheEvent;

impl Adapter for WrongCacheEvent {
    fn distribution(&self) -> Distribution {
        Distribution::Internal
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        let Work::Cache(cache) = work else { return };
        let sequence = cache.subject().to_owned();
        events.raise(Event::Cached {
            deployment: cache.deployment,
            stage_id: cache.stage_id,
            generation: cache.generation + 1,
            operation_id: cache.operation_id,
            sequence,
            bytes: 1,
            detail: "wrong generation".into(),
        });
    }
}

struct CacheBodies;

impl Payload for CacheBodies {
    fn sequence(&self, _frame: &Frame) -> Option<p4_adapter::Sequence> {
        None
    }

    fn lifecycle(&self, frame: &Frame) -> Option<Work> {
        Some(Work::Cache(p4_adapter::Cache {
            deployment: "deployment".into(),
            stage_id: "n0".into(),
            generation: frame.envelope.chain.as_ref()?.current().generation,
            operation_id: frame.envelope.request_id.clone(),
            sequence: "sequence".into(),
            action: CacheAction::Persist,
        }))
    }
}

#[test]
fn a_cache_event_for_a_stale_generation_is_rejected() {
    runtime().block_on(async {
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        let handle = Node::spawn(Arc::new(WrongCacheEvent), Arc::new(CacheBodies), sender, 1);
        handle.offer(work("cache-stale", 1)).unwrap();
        settle().await;

        let mut responses = 0;
        while let Ok(frame) = receiver.take().now_or_never_ok() {
            if frame.envelope.lane == QueueClass::Response {
                responses += 1;
            }
        }
        assert_eq!(responses, 1);
        assert!(handle.counts().invalid_events.load(Ordering::SeqCst) >= 1);
        assert_eq!(handle.depth(), 0);
        assert!(!handle.is_running());
    });
}

struct WrongHopDeployment;

impl Adapter for WrongHopDeployment {
    fn distribution(&self) -> Distribution {
        Distribution::Internal
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        let Work::Hop(hop) = work else { return };
        events.raise(Event::HopComplete {
            hop_id: hop.id,
            deployment: "other-deployment".into(),
            expected: hop
                .sequences
                .iter()
                .map(|sequence| sequence.sequence.clone())
                .collect(),
            outcomes: hop
                .sequences
                .iter()
                .map(|sequence| Outcome {
                    sequence: sequence.sequence.clone(),
                    forward: None,
                    text: String::new(),
                    stop: Some("wrong deployment".into()),
                })
                .collect(),
        });
    }
}

#[test]
fn a_hop_completion_for_the_wrong_deployment_is_rejected() {
    runtime().block_on(async {
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        let handle = Node::spawn(Arc::new(WrongHopDeployment), Arc::new(Bodies), sender, 1);
        handle.offer(work("wrong-hop-deployment", 1)).unwrap();
        settle().await;

        let responses = std::iter::from_fn(|| receiver.take().now_or_never_ok().ok())
            .filter(|frame| frame.envelope.lane == QueueClass::Response)
            .count();
        assert_eq!(responses, 1);
        assert!(handle.counts().invalid_events.load(Ordering::SeqCst) >= 1);
        assert!(!handle.is_running());
    });
}

#[derive(Clone, Copy)]
enum WrongLoadEvent {
    Loaded,
    LoadedWrongGeneration,
    Progress,
}

struct WrongLoadDeployment {
    event: WrongLoadEvent,
}

impl Adapter for WrongLoadDeployment {
    fn distribution(&self) -> Distribution {
        Distribution::Internal
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        let Work::Load(load) = work else { return };
        match self.event {
            WrongLoadEvent::Loaded => events.raise(Event::Loaded {
                deployment: "other-deployment".into(),
                generation: 2,
                allocations: Vec::new(),
            }),
            WrongLoadEvent::LoadedWrongGeneration => events.raise(Event::Loaded {
                deployment: load.deployment,
                generation: 2,
                allocations: Vec::new(),
            }),
            WrongLoadEvent::Progress => events.raise(Event::LoadProgress {
                deployment: "other-deployment".into(),
                stage: 0,
                percent: 50,
                detail: format!("loading {}", load.artifact),
            }),
        }
    }
}

struct LoadBodies;

impl Payload for LoadBodies {
    fn sequence(&self, _frame: &Frame) -> Option<p4_adapter::Sequence> {
        None
    }

    fn lifecycle(&self, frame: &Frame) -> Option<Work> {
        (frame.body == b"load").then(|| {
            Work::Load(p4_adapter::Load {
                deployment: "deployment".into(),
                plan: "plan".into(),
                artifact: "artifact".into(),
                capability_snapshot_id: "snapshot".into(),
                capability_expires_at: u64::MAX,
            })
        })
    }
}

fn load_work(route: &str) -> Frame {
    let mut frame = work(route, 1);
    frame.body = b"load".to_vec();
    frame
}

#[test]
fn a_loaded_event_for_the_wrong_deployment_is_rejected() {
    assert_wrong_load_event(WrongLoadEvent::Loaded);
}

#[test]
fn a_loaded_event_for_a_stale_generation_is_rejected() {
    assert_wrong_load_event(WrongLoadEvent::LoadedWrongGeneration);
}

#[test]
fn load_progress_for_the_wrong_deployment_is_rejected() {
    assert_wrong_load_event(WrongLoadEvent::Progress);
}

fn assert_wrong_load_event(event: WrongLoadEvent) {
    runtime().block_on(async {
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        let handle = Node::spawn(
            Arc::new(WrongLoadDeployment { event }),
            Arc::new(LoadBodies),
            sender,
            1,
        );
        handle.offer(load_work("wrong-load-deployment")).unwrap();
        settle().await;

        let responses = std::iter::from_fn(|| receiver.take().now_or_never_ok().ok())
            .filter(|frame| frame.envelope.lane == QueueClass::Response)
            .count();
        assert_eq!(responses, 1);
        assert!(handle.counts().invalid_events.load(Ordering::SeqCst) >= 1);
        assert!(!handle.is_running());
    });
}

/// Keeps the first adapter invocation alive after it has reported success.
/// This models a restarted backend whose old callback arrives after the node
/// has already admitted a replacement load for the same deployment.
struct LateLoadEvent {
    starts: Arc<AtomicUsize>,
    release_stale: Mutex<SyncReceiver<()>>,
}

impl Adapter for LateLoadEvent {
    fn distribution(&self) -> Distribution {
        Distribution::Internal
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        let Work::Load(load) = work else { return };
        let invocation = self.starts.fetch_add(1, Ordering::SeqCst);
        if invocation == 0 {
            events.raise(Event::Loaded {
                deployment: load.deployment.clone(),
                generation: 1,
                allocations: Vec::new(),
            });
            self.release_stale.lock().unwrap().recv().unwrap();
            events.raise(Event::Loaded {
                deployment: load.deployment,
                generation: 1,
                allocations: Vec::new(),
            });
        }
    }
}

#[test]
fn a_callback_from_a_previous_load_cannot_complete_a_replacement_load() {
    runtime().block_on(async {
        let (release_stale, wait_stale) = std::sync::mpsc::channel();
        let starts = Arc::new(AtomicUsize::new(0));
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        let handle = Node::spawn(
            Arc::new(LateLoadEvent {
                starts: Arc::clone(&starts),
                release_stale: Mutex::new(wait_stale),
            }),
            Arc::new(LoadBodies),
            sender,
            1,
        );

        handle.offer(load_work("first-load")).unwrap();
        for _ in 0..50 {
            if starts.load(Ordering::SeqCst) >= 1 && !handle.is_running() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        handle.offer(load_work("replacement-load")).unwrap();
        for _ in 0..50 {
            if starts.load(Ordering::SeqCst) >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(starts.load(Ordering::SeqCst), 2);

        release_stale.send(()).unwrap();
        settle().await;

        let responses = std::iter::from_fn(|| receiver.take().now_or_never_ok().ok())
            .filter(|frame| frame.envelope.lane == QueueClass::Response)
            .count();
        assert_eq!(responses, 1, "only the first load may have completed");
        assert!(
            handle.is_running(),
            "a stale callback must not complete the replacement load"
        );
        assert!(
            handle.counts().orphaned.load(Ordering::SeqCst) >= 1,
            "the stale callback must be observable as orphaned"
        );
        handle.shutdown().await;
    });
}

struct LateTimedOutHop {
    starts: Arc<AtomicUsize>,
    release_late: Mutex<SyncReceiver<()>>,
}

impl Adapter for LateTimedOutHop {
    fn distribution(&self) -> Distribution {
        Distribution::Staged
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        let Work::Hop(hop) = work else { return };
        let invocation = self.starts.fetch_add(1, Ordering::SeqCst);
        if invocation == 0 {
            self.release_late.lock().unwrap().recv().unwrap();
            let sequence = hop.sequences[0].sequence.clone();
            events.raise(Event::SequenceAcquired {
                deployment: hop.deployment.clone(),
                sequence: sequence.clone(),
            });
            events.raise(Event::SequenceReleased {
                deployment: hop.deployment.clone(),
                sequence: sequence.clone(),
            });
            events.raise(Event::LoadProgress {
                deployment: hop.deployment.clone(),
                stage: 0,
                percent: 90,
                detail: "late".into(),
            });
        }
        let sequence = hop.sequences[0].sequence.clone();
        events.raise(Event::HopComplete {
            hop_id: hop.id,
            deployment: hop.deployment,
            expected: vec![sequence.clone()],
            outcomes: vec![Outcome {
                sequence,
                forward: None,
                text: String::new(),
                stop: Some("done".into()),
            }],
        });
    }
}

#[test]
fn timeout_then_late_events_cannot_change_replacement_admission() {
    runtime().block_on(async {
        let (release_late, wait_late) = std::sync::mpsc::channel();
        let starts = Arc::new(AtomicUsize::new(0));
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        let handle = Node::spawn(
            Arc::new(LateTimedOutHop {
                starts: Arc::clone(&starts),
                release_late: Mutex::new(wait_late),
            }),
            Arc::new(Bodies),
            sender,
            1,
        );

        let mut timed_out = work("timed-out", 1);
        timed_out.envelope.deadline_unix_ms = now_unix_ms() + 40;
        handle.offer(timed_out).unwrap();
        for _ in 0..100 {
            if receiver
                .take()
                .now_or_never_ok()
                .map(|frame| frame.envelope.route == "timed-out")
                .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(starts.load(Ordering::SeqCst), 1);

        handle.offer(work("replacement", 1)).unwrap();
        release_late.send(()).unwrap();
        settle().await;

        assert_eq!(starts.load(Ordering::SeqCst), 2);
        assert_eq!(handle.active_hop(), None);
        assert_eq!(handle.counts().completions.load(Ordering::SeqCst), 1);
        assert!(handle.counts().orphaned.load(Ordering::SeqCst) >= 1);
        handle.shutdown().await;
    });
}

struct LifecycleRecording {
    starts: Arc<Mutex<Vec<String>>>,
    release_first: Mutex<SyncReceiver<()>>,
}

impl Adapter for LifecycleRecording {
    fn distribution(&self) -> Distribution {
        Distribution::Staged
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        match work {
            Work::Hop(hop) => {
                let sequence = hop.sequences[0].sequence.clone();
                self.starts.lock().unwrap().push(format!("hop:{sequence}"));
                if sequence == "first" {
                    self.release_first.lock().unwrap().recv().unwrap();
                }
                events.raise(Event::HopComplete {
                    hop_id: hop.id,
                    deployment: hop.deployment,
                    expected: vec![sequence.clone()],
                    outcomes: vec![Outcome {
                        sequence,
                        forward: None,
                        text: String::new(),
                        stop: Some("done".into()),
                    }],
                });
            }
            Work::Unload(unload) => {
                self.starts.lock().unwrap().push("unload".into());
                events.raise(Event::Unloaded {
                    deployment: unload.deployment,
                });
            }
            _ => {}
        }
    }
}

struct LifecycleBodies;

impl Payload for LifecycleBodies {
    fn sequence(&self, frame: &Frame) -> Option<p4_adapter::Sequence> {
        if frame.body == b"unload" {
            return None;
        }
        Some(p4_adapter::Sequence {
            sequence: frame.envelope.route.clone(),
            state: None,
            prompt: Some(String::new()),
            remaining: 1,
            options: "{}".into(),
        })
    }

    fn lifecycle(&self, frame: &Frame) -> Option<Work> {
        (frame.body == b"unload").then(|| {
            Work::Unload(p4_adapter::Unload {
                deployment: "deployment".into(),
            })
        })
    }
}

fn unload_work(route: &str) -> Frame {
    let mut frame = work(route, 1);
    frame.body = b"unload".to_vec();
    frame
}

#[test]
fn unload_waits_for_queued_hops_before_adapter_release() {
    runtime().block_on(async {
        let (release, wait): (SyncSender<()>, SyncReceiver<()>) = std::sync::mpsc::channel();
        let starts = Arc::new(Mutex::new(Vec::new()));
        let adapter = Arc::new(LifecycleRecording {
            starts: Arc::clone(&starts),
            release_first: Mutex::new(wait),
        });
        let (sender, _receiver, _) = channel(Lanes::default(), Budget::default());
        let handle = Node::spawn(adapter, Arc::new(LifecycleBodies), sender, 1);

        handle.offer(work("first", 1)).unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        handle.offer(work("second", 1)).unwrap();
        handle.offer(unload_work("unload")).unwrap();
        release.send(()).unwrap();
        settle().await;

        assert_eq!(
            *starts.lock().unwrap(),
            vec!["hop:first", "hop:second", "unload"]
        );
    });
}

#[test]
fn a_middle_node_hands_work_on_instead_of_replying() {
    runtime().block_on(async {
        let mut fixture = fixture(4, 1);
        let _ = fixture.handle.offer(work("r0", 3));
        settle().await;

        let frame = fixture
            .receiver
            .take()
            .now_or_never_ok()
            .expect("the node emitted something");
        assert_eq!(frame.envelope.recipient, Recipient::node("n1"));
        assert_eq!(frame.body, b"prompt");
    });
}

/// Small helper: takes what is already queued without waiting for more.
trait NowOrNever {
    type Output;
    fn now_or_never_ok(self) -> Result<Self::Output, ()>;
}

impl<F: std::future::Future<Output = Option<Frame>>> NowOrNever for F {
    type Output = Frame;

    fn now_or_never_ok(self) -> Result<Frame, ()> {
        let mut future = Box::pin(self);
        let waker = futures_waker();
        let mut context = std::task::Context::from_waker(&waker);
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(Some(frame)) => Ok(frame),
            _ => Err(()),
        }
    }
}

fn futures_waker() -> std::task::Waker {
    use std::task::{RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}

/// A load event that arrives with no lifecycle carrier must not release the
/// node's running permit. `finished()` is the node's only mutual exclusion
/// over a hop, so freeing it here starts a second hop beside the first.
struct StrayProgressDuringHop;

impl Adapter for StrayProgressDuringHop {
    fn distribution(&self) -> Distribution {
        Distribution::Internal
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        let Work::Hop(hop) = work else { return };
        // The hop is deliberately left in flight.
        events.raise(Event::LoadProgress {
            deployment: hop.deployment,
            stage: 0,
            percent: 10,
            detail: "stray".into(),
        });
    }
}

#[test]
fn a_stray_load_progress_does_not_release_a_running_hop() {
    runtime().block_on(async {
        let (sender, mut _receiver, _) = channel(Lanes::default(), Budget::default());
        let handle = Node::spawn(
            Arc::new(StrayProgressDuringHop),
            Arc::new(Bodies),
            sender,
            1,
        );
        handle.offer(work("stray-progress", 1)).unwrap();
        settle().await;
        assert!(
            handle.is_running(),
            "a hop still in flight lost its running permit to a stray load event"
        );
    });
}

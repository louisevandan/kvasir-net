use super::*;
use crate::queue::lane::{Budget, Lanes};
use crate::queue::main::channel;
use p4_adapter::{Distribution, EventSink, Outcome};
use p4_protocol::{Address, Chain, Envelope, Link, Recipient};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Records the width of every hop it is given and answers each one.
struct Recording {
    widths: Arc<Mutex<Vec<usize>>>,
    peak: Arc<AtomicUsize>,
    running: Arc<AtomicUsize>,
    finish_after: usize,
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
        let outcomes = hop
            .sequences
            .iter()
            .map(|sequence| Outcome {
                sequence: sequence.sequence.clone(),
                text: "t".into(),
                position: sequence.position + 1,
                stop: (sequence.position + 1 >= self.finish_after as u32)
                    .then(|| "stop".to_string()),
            })
            .collect();
        self.running.fetch_sub(1, Ordering::SeqCst);
        events.raise(Event::HopComplete {
            deployment: hop.deployment,
            outcomes,
        });
    }
}

struct Bodies;

impl Payload for Bodies {
    fn sequence(&self, frame: &Frame) -> Option<p4_adapter::Sequence> {
        Some(p4_adapter::Sequence {
            sequence: frame.envelope.route.clone(),
            position: 0,
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

struct Fixture {
    handle: Handle,
    widths: Arc<Mutex<Vec<usize>>>,
    peak: Arc<AtomicUsize>,
    receiver: crate::queue::main::Receiver,
}

fn fixture(ceiling: usize, finish_after: usize) -> Fixture {
    let (sender, receiver, _) = channel(Lanes::default(), Budget::default());
    let widths = Arc::new(Mutex::new(Vec::new()));
    let peak = Arc::new(AtomicUsize::new(0));
    let adapter = Arc::new(Recording {
        widths: Arc::clone(&widths),
        peak: Arc::clone(&peak),
        running: Arc::new(AtomicUsize::new(0)),
        finish_after,
    });
    let handle = Node::spawn(adapter, Arc::new(Bodies), sender, ceiling);
    Fixture {
        handle,
        widths,
        peak,
        receiver,
    }
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

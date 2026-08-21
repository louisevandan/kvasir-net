use super::Node;
use crate::node::outcome;
use crate::node::payload::Payload;
use crate::queue::lane::{Budget, Lanes};
use crate::queue::main::channel;
use p4_adapter::{Adapter, Distribution, Event, EventSink, Outcome, Sequence, Work};
use p4_protocol::frame::Frame;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

struct Sequences;

impl Payload for Sequences {
    fn sequence(&self, frame: &Frame) -> Option<Sequence> {
        Some(Sequence {
            sequence: frame.envelope.route.clone(),
            state: None,
            prompt: Some("prompt".into()),
            remaining: 4,
            options: "{}".into(),
        })
    }
}

fn carrier(route: &str, event_seq: u64) -> Frame {
    let chain = Chain::new(vec![Link {
        address: Address::tcp("127.0.0.1", 52001),
        node: "n0".into(),
        binding: "deployment".into(),
        generation: 1,
    }])
    .unwrap();
    Frame {
        envelope: Envelope {
            target: chain.current().address.clone(),
            recipient: Recipient::node("n0"),
            lane: QueueClass::Decode,
            route: route.into(),
            request_id: route.into(),
            stream_id: route.into(),
            origin_agent: None,
            return_channel: None,
            ingress_generation: 0,
            event_seq,
            deadline_unix_ms: 0,
            reply_to: Some(Address::tcp("127.0.0.1", 52002)),
            chain: Some(chain),
        },
        body: b"prompt".to_vec(),
    }
}

#[test]
fn a_normal_terminal_is_the_first_observable_response() {
    let frame = carrier("normal", 0);
    let next = outcome::next(
        &frame,
        &Outcome {
            sequence: "normal".into(),
            forward: None,
            text: String::new(),
            stop: Some("eos".into()),
            terminal_generated: None,
        },
        &Sequences,
    );
    let frames = next.frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].envelope.event_seq, 1);
}

#[test]
fn an_error_after_a_silent_lap_is_still_the_first_response() {
    // A muted/silent lap carries zero because it has not made an observable
    // response. Its later failure must be response one, not a gap at two.
    let reply = Node::response_frame(&carrier("silent", 0), b"failed".to_vec()).unwrap();
    assert_eq!(reply.envelope.event_seq, 1);
}

struct TokenThenTombstone {
    starts: AtomicUsize,
}

struct LifecyclePayload;

impl Payload for LifecyclePayload {
    fn sequence(&self, _frame: &Frame) -> Option<Sequence> {
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

struct ProgressThenLoaded;

impl Adapter for ProgressThenLoaded {
    fn distribution(&self) -> Distribution {
        Distribution::Internal
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        let Work::Load(load) = work else { return };
        events.raise(Event::LoadProgress {
            deployment: load.deployment.clone(),
            stage: 0,
            percent: 25,
            detail: "opening".into(),
        });
        events.raise(Event::LoadProgress {
            deployment: load.deployment.clone(),
            stage: 0,
            percent: 75,
            detail: "loading".into(),
        });
        events.raise(Event::Loaded {
            deployment: load.deployment,
            generation: 1,
            allocations: Vec::new(),
        });
    }
}

impl Adapter for TokenThenTombstone {
    fn distribution(&self) -> Distribution {
        Distribution::Staged
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        let Work::Hop(hop) = work else { return };
        let sequence = hop.sequences[0].sequence.clone();
        if self.starts.fetch_add(1, Ordering::SeqCst) == 0 {
            events.raise(Event::HopComplete {
                hop_id: hop.id,
                deployment: hop.deployment,
                expected: vec![sequence.clone()],
                outcomes: vec![Outcome {
                    sequence,
                    forward: Some(vec![1]),
                    text: "토큰".into(),
                    stop: None,
                    terminal_generated: None,
                }],
            });
        } else {
            // This is the staged SequenceLedger's observable shape for a
            // redelivered continuation after the sequence was released.
            events.raise(Event::Failed {
                deployment: hop.deployment,
                sequence: Some(sequence),
                hop_id: Some(hop.id),
                detail: "released sequence tombstone".into(),
            });
        }
    }
}

async fn take(receiver: &mut crate::queue::main::Receiver) -> Frame {
    tokio::time::timeout(Duration::from_secs(1), receiver.take())
        .await
        .expect("a node response before timeout")
        .expect("an open node output queue")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tombstoned_continuation_fails_at_the_next_sequence_without_stranding_the_route() {
    let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
    let handle = Node::spawn(
        Arc::new(TokenThenTombstone {
            starts: AtomicUsize::new(0),
        }),
        Arc::new(Sequences),
        sender,
        1,
    );
    handle.offer(carrier("replayed", 0)).unwrap();

    let token = take(&mut receiver).await;
    let lap = take(&mut receiver).await;
    assert_eq!(token.envelope.lane, QueueClass::Response);
    assert_eq!(token.envelope.event_seq, 1);
    assert_eq!(lap.envelope.lane, QueueClass::Decode);
    assert_eq!(lap.envelope.event_seq, 1);

    handle.offer(lap).unwrap();
    let failed = take(&mut receiver).await;
    assert_eq!(failed.envelope.lane, QueueClass::Response);
    assert_eq!(failed.envelope.event_seq, 2);
    assert!(String::from_utf8_lossy(&failed.body).contains("tombstone"));
    assert_eq!(handle.depth(), 0, "the rejected continuation is terminal");
    assert!(!handle.is_running(), "the route has no in-flight hop left");
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_progress_and_its_terminal_consume_consecutive_sequences() {
    let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
    let handle = Node::spawn(
        Arc::new(ProgressThenLoaded),
        Arc::new(LifecyclePayload),
        sender,
        1,
    );
    let mut load = carrier("load", 0);
    load.body = b"load".to_vec();
    handle.offer(load).unwrap();

    let sequences = [
        take(&mut receiver).await,
        take(&mut receiver).await,
        take(&mut receiver).await,
    ]
    .map(|frame| frame.envelope.event_seq);
    assert_eq!(sequences, [1, 2, 3]);
    handle.shutdown().await;
}

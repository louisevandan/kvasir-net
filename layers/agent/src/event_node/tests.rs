use super::*;
use crate::event_broker::{Delivery, DispatchFailure, DispatchOutcome, bounded_queue};
use p4_adapter::node_adapter::{
    CompletionMailbox, CompletionPublisher, OfferError, Poll, completion_mailbox,
};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, Event, EventClass};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll as TaskPoll};

struct CompletingAdapter {
    publisher: CompletionPublisher,
    mailbox: Arc<CompletionMailbox>,
    source: Endpoint,
    target: Endpoint,
}

struct ClosedAdapter;

impl NodeAdapter for ClosedAdapter {
    fn kind(&self) -> &str {
        "closed-test"
    }
    fn try_offer(&self, event: Event) -> Result<(), OfferError> {
        Err(OfferError::Closed(event))
    }
    fn try_take(&self) -> Poll {
        Poll::Closed
    }
    fn poll_take(&self, _context: &mut Context<'_>) -> TaskPoll<Poll> {
        TaskPoll::Ready(Poll::Closed)
    }
    fn snapshot(&self) -> String {
        "logical_batch_failed:native exited 17".into()
    }
}

impl NodeAdapter for CompletingAdapter {
    fn kind(&self) -> &str {
        "test"
    }
    fn try_offer(&self, event: Event) -> Result<(), OfferError> {
        // The completion this stands in for is derived from the event, so a
        // full publisher hands the original back the way a real adapter does.
        let offered = event.clone();
        // A completion derives its id from the event it answers: two
        // completions sharing one id are a conflicting duplicate to the
        // broker ledger, which is a property of this stand-in and not of the
        // node under test.
        let completion_id = format!("complete-{}", event.envelope.event_id);
        let envelope = event.envelope.next(
            &completion_id,
            self.source.clone(),
            self.target.clone(),
            EventClass::Telemetry,
            2,
            "application/test-complete",
        );
        self.publisher
            .try_publish(Event {
                envelope,
                payload: vec![9],
            })
            .map_err(|_| OfferError::Full(offered))
    }
    fn try_take(&self) -> Poll {
        self.mailbox.try_take()
    }
    fn poll_take(&self, context: &mut Context<'_>) -> TaskPoll<Poll> {
        self.mailbox.poll_take(context)
    }
}

fn event(own: &Address) -> Event {
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: "start".into(),
            correlation_id: "request".into(),
            causation_id: None,
            source: Endpoint::outer(own.clone(), "outer", 1),
            target: Endpoint::node(own.clone(), "n1", 1),
            return_route: None,
            class: EventClass::Control,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: Some("test".into()),
            payload_content_type: "application/test".into(),
        },
        payload: vec![1],
    }
}

#[tokio::test]
async fn node_moves_adapter_completion_back_to_agent_without_callback_reentry() {
    let own = Address::tcp("127.0.0.1", 52001);
    let (agent_tx, mut agent_rx) = bounded_queue(4);
    let (outer_tx, _outer_rx) = bounded_queue(4);
    let (outbound_tx, _outbound_rx) = bounded_queue(4);
    let (node_tx, node_rx) = bounded_queue(4);
    let broker = Arc::new(EventBroker::new(
        own.clone(),
        agent_tx,
        outer_tx,
        outbound_tx,
        8,
    ));
    broker.register_node("n1", 1, node_tx).unwrap();
    let (publisher, mailbox) = completion_mailbox(4);
    let adapter = Arc::new(CompletingAdapter {
        publisher,
        mailbox,
        source: Endpoint::node(own.clone(), "n1", 1),
        target: Endpoint::agent(own.clone()),
    });
    let task = tokio::spawn(EventNode::new(adapter, node_rx, Arc::clone(&broker)).run());
    assert_eq!(
        broker.dispatch(event(&own)).unwrap(),
        DispatchOutcome::Enqueued(Delivery::Node {
            node: "n1".into(),
            generation: 1,
        })
    );
    let completed = tokio::time::timeout(std::time::Duration::from_secs(1), agent_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.payload, vec![9]);
    task.abort();
}

#[tokio::test]
async fn closed_completion_reports_the_adapter_failure_snapshot() {
    let own = Address::tcp("127.0.0.1", 52001);
    let (agent_tx, _agent_rx) = bounded_queue(1);
    let (outer_tx, _outer_rx) = bounded_queue(1);
    let (outbound_tx, _outbound_rx) = bounded_queue(1);
    let (_node_tx, node_rx) = bounded_queue(1);
    let broker = Arc::new(EventBroker::new(own, agent_tx, outer_tx, outbound_tx, 1));
    let error = EventNode::new(Arc::new(ClosedAdapter), node_rx, broker)
        .run()
        .await
        .unwrap_err();
    assert_eq!(
        error.error,
        EventNodeError::CompletionClosed("logical_batch_failed:native exited 17".into())
    );
    assert!(error.held_input.is_none() && error.held_output.is_none());
}

/// An adapter with no room until it is given some.
///
/// `refusals` counts how many times it turned an event away, so a test can
/// tell "the node retried" from "the node gave up", and `accepted` holds what
/// it finally took so the test can prove the event was not dropped on the way.
struct FullAdapter {
    room: Arc<AtomicBool>,
    refusals: Arc<AtomicUsize>,
    accepted: Arc<Mutex<Vec<Event>>>,
}

impl NodeAdapter for FullAdapter {
    fn kind(&self) -> &str {
        "full-test"
    }
    fn try_offer(&self, event: Event) -> Result<(), OfferError> {
        if self.room.load(Ordering::SeqCst) {
            self.accepted.lock().unwrap().push(event);
            Ok(())
        } else {
            self.refusals.fetch_add(1, Ordering::SeqCst);
            Err(OfferError::Full(event))
        }
    }
    fn try_take(&self) -> Poll {
        Poll::Empty
    }
    fn poll_take(&self, _context: &mut Context<'_>) -> TaskPoll<Poll> {
        // An empty mailbox is pending, not ready-with-nothing: a real one
        // registers the waker and says nothing until a completion arrives.
        // Returning `Ready(Empty)` here would make the node's select spin.
        TaskPoll::Pending
    }
    fn snapshot(&self) -> String {
        "full".into()
    }
}

/// A full adapter is backpressure, not a dead node.
///
/// `OfferError::Full` used to end the task with `AdapterFull`: one burst that
/// outran the adapter stopped the pipeline for good. The node now keeps the
/// event, stops reading inbound, and retries - so this asserts three things
/// the old code failed: the task is still running while the adapter is full,
/// the event is delivered once room appears, and it is delivered whole rather
/// than dropped.
#[tokio::test]
async fn a_full_adapter_holds_the_event_instead_of_failing_the_node() {
    let own = Address::tcp("127.0.0.1", 52001);
    let (agent_tx, _agent_rx) = bounded_queue(4);
    let (outer_tx, _outer_rx) = bounded_queue(4);
    let (outbound_tx, _outbound_rx) = bounded_queue(4);
    let (node_tx, node_rx) = bounded_queue(4);
    let broker = Arc::new(EventBroker::new(
        own.clone(),
        agent_tx,
        outer_tx,
        outbound_tx,
        8,
    ));
    broker.register_node("n1", 1, node_tx).unwrap();

    let room = Arc::new(AtomicBool::new(false));
    let refusals = Arc::new(AtomicUsize::new(0));
    let accepted = Arc::new(Mutex::new(Vec::new()));
    let adapter = Arc::new(FullAdapter {
        room: Arc::clone(&room),
        refusals: Arc::clone(&refusals),
        accepted: Arc::clone(&accepted),
    });
    let task = tokio::spawn(EventNode::new(adapter, node_rx, Arc::clone(&broker)).run());

    broker.dispatch(event(&own)).unwrap();

    // Let it turn the event away several times over. The node waits a
    // millisecond between attempts, so this is time rather than yields.
    let window = std::time::Duration::from_millis(200);
    tokio::time::sleep(window).await;
    let attempts = refusals.load(Ordering::SeqCst);
    assert!(
        attempts > 1,
        "the node should retry a full adapter, not offer once"
    );
    // And an upper bound, because retrying is not the same as spinning.
    //
    // An earlier version of this fix retried with `yield_now`, which passed
    // the lower bound above and burned a core flat - 739 seconds of CPU in
    // 12.5 minutes of wall clock, found by looking at the process rather than
    // by this test. At one attempt per millisecond a 200 ms window admits
    // about 200; ten times that is loose enough for a slow machine and tight
    // enough that a spin, which manages hundreds of thousands, cannot pass.
    let ceiling = 10 * (window.as_millis() as usize);
    assert!(
        attempts < ceiling,
        "retrying a full adapter should wait between attempts: {attempts} in {window:?}",
    );
    assert!(!task.is_finished(), "a full adapter must not end the node");
    assert!(accepted.lock().unwrap().is_empty());

    room.store(true, Ordering::SeqCst);
    let delivered = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if !accepted.lock().unwrap().is_empty() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await;
    assert!(
        delivered.is_ok(),
        "the held event should go through once there is room"
    );
    let taken = accepted.lock().unwrap();
    assert_eq!(taken.len(), 1, "held once, delivered once");
    assert_eq!(taken[0].payload, event(&own).payload);
    assert!(!task.is_finished());
    task.abort();
}

/// A full destination holds the completion; it does not end the node.
///
/// The other tests here cover one hop each. This one runs the chain the
/// review asked for - adapter completion, mailbox, node, broker destination -
/// with the destination at capacity one, because that is where a computed
/// token was being dropped and the node ended for a queue about to drain.
#[tokio::test]
async fn a_full_destination_holds_the_completion_and_the_node_survives() {
    let own = Address::tcp("127.0.0.1", 52001);
    // One slot, and nothing takes from it until this test says so.
    let (agent_tx, mut agent_rx) = bounded_queue(1);
    let (outer_tx, _outer_rx) = bounded_queue(4);
    let (outbound_tx, _outbound_rx) = bounded_queue(4);
    let (node_tx, node_rx) = bounded_queue(4);
    let broker = Arc::new(EventBroker::new(
        own.clone(),
        agent_tx,
        outer_tx,
        outbound_tx,
        8,
    ));
    broker.register_node("n1", 1, node_tx).unwrap();
    let (publisher, mailbox) = completion_mailbox(4);
    let adapter = Arc::new(CompletingAdapter {
        publisher,
        mailbox,
        source: Endpoint::node(own.clone(), "n1", 1),
        target: Endpoint::agent(own.clone()),
    });
    let task = tokio::spawn(EventNode::new(adapter, node_rx, Arc::clone(&broker)).run());

    // Two requests, so the adapter answers with two completions and the
    // second finds the one slot taken.
    let mut first = event(&own);
    first.envelope.event_id = "first".into();
    let mut second = event(&own);
    second.envelope.event_id = "second".into();
    second.envelope.correlation_id = "second".into();
    second.envelope.sequence = 2;
    broker.dispatch(first).unwrap();
    broker.dispatch(second).unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !task.is_finished(),
        "a full destination must not end the node",
    );

    // Draining the slot lets the held completion through.
    let delivered = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let mut seen = Vec::new();
        while seen.len() < 2 {
            match agent_rx.recv().await {
                Some(event) => seen.push(event),
                None => break,
            }
        }
        seen
    })
    .await
    .expect("both completions should arrive once there is room");
    assert_eq!(delivered.len(), 2, "held once, delivered once");
    assert!(delivered.iter().all(|event| event.payload == vec![9]));
    assert!(!task.is_finished());

    // And a node that is waiting can still be torn down promptly.
    task.abort();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .is_ok(),
        "a node waiting on a destination must still stop",
    );
}

/// No backend semantics or echo/dedup implementation live in this adapter.
/// Every accepted event is retained verbatim; the completion mailbox is a
/// separate bounded queue that the test populates with already-computed work.
struct DuplexProbeAdapter {
    mailbox: Arc<CompletionMailbox>,
    accepted: Arc<Mutex<Vec<Event>>>,
    taken: AtomicUsize,
}

impl DuplexProbeAdapter {
    fn observed(&self, value: Poll) -> Poll {
        if matches!(&value, Poll::Event(_)) {
            self.taken.fetch_add(1, Ordering::SeqCst);
        }
        value
    }
}

impl NodeAdapter for DuplexProbeAdapter {
    fn kind(&self) -> &str {
        "opaque-duplex-test"
    }

    fn try_offer(&self, event: Event) -> Result<(), OfferError> {
        self.accepted.lock().unwrap().push(event);
        Ok(())
    }

    fn try_take(&self) -> Poll {
        self.observed(self.mailbox.try_take())
    }

    fn poll_take(&self, context: &mut Context<'_>) -> TaskPoll<Poll> {
        self.mailbox
            .poll_take(context)
            .map(|value| self.observed(value))
    }
}

fn duplex_event(id: &str, source: Endpoint, target: Endpoint, payload: &[u8]) -> Event {
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: id.into(),
            correlation_id: format!("correlation-{id}"),
            causation_id: None,
            source,
            target,
            return_route: None,
            class: EventClass::Data,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: Some("opaque-duplex-test".into()),
            payload_content_type: "application/octet-stream".into(),
        },
        payload: payload.to_vec(),
    }
}

/// This is a specific duplex liveness obligation, not a proof that arbitrary
/// completely-full cyclic networks are deadlock-free. Both adapters here can
/// accept their inbound events, so draining input creates real destination
/// capacity. General cyclic credit/reserved-control capacity is a later gate.
#[tokio::test]
async fn bidirectional_full_completions_do_not_block_input_that_frees_the_ring() {
    use std::future::Future;

    let own = Address::tcp("127.0.0.1", 52041);
    let (agent_tx, _agent_rx) = bounded_queue(1);
    let (outer_tx, _outer_rx) = bounded_queue(1);
    let (outbound_tx, _outbound_rx) = bounded_queue(1);
    let (a_tx, a_rx) = bounded_queue(1);
    let (b_tx, b_rx) = bounded_queue(1);
    let broker = Arc::new(EventBroker::new(
        own.clone(),
        agent_tx,
        outer_tx,
        outbound_tx,
        32,
    ));
    broker.register_node("a", 3, a_tx.clone()).unwrap();
    broker.register_node("b", 7, b_tx.clone()).unwrap();
    let a = Endpoint::node(own.clone(), "a", 3);
    let b = Endpoint::node(own.clone(), "b", 7);
    let into_a = duplex_event(
        "input-a",
        Endpoint::outer(own.clone(), "client-a", 1),
        a.clone(),
        &[0, 0xff, 1, 0],
    );
    let into_b = duplex_event(
        "input-b",
        Endpoint::outer(own.clone(), "client-b", 1),
        b.clone(),
        &[0xfe, 2, 0, 3],
    );
    let completion_a = duplex_event("a-to-b", a.clone(), b.clone(), &[7, 0, 0xf8, 9]);
    let completion_b = duplex_event("b-to-a", b, a, &[0x80, 0, 6, 5]);
    let (a_publisher, a_mailbox) = completion_mailbox(1);
    let (b_publisher, b_mailbox) = completion_mailbox(1);
    a_publisher.try_publish(completion_a.clone()).unwrap();
    b_publisher.try_publish(completion_b.clone()).unwrap();
    let accepted_a = Arc::new(Mutex::new(Vec::new()));
    let accepted_b = Arc::new(Mutex::new(Vec::new()));
    let adapter_a = Arc::new(DuplexProbeAdapter {
        mailbox: a_mailbox,
        accepted: Arc::clone(&accepted_a),
        taken: AtomicUsize::new(0),
    });
    let adapter_b = Arc::new(DuplexProbeAdapter {
        mailbox: b_mailbox,
        accepted: Arc::clone(&accepted_b),
        taken: AtomicUsize::new(0),
    });

    // Reserve only capacity, not an event: recv is Pending while dispatch is
    // Full. This makes completion the only ready select arm on the first poll,
    // regardless of Tokio's randomized branch order. No event uses permit.send.
    let a_reservation = a_tx.try_reserve().unwrap();
    let b_reservation = b_tx.try_reserve().unwrap();
    for (input, expected_node, generation) in [(&into_a, "a", 3), (&into_b, "b", 7)] {
        let Err(DispatchFailure {
            error: DispatchError::Full(delivery),
            event: returned,
        }) = broker.dispatch(input.clone())
        else {
            panic!("the initial destination must actually be full");
        };
        assert_eq!(
            delivery,
            Delivery::Node {
                node: expected_node.into(),
                generation
            }
        );
        assert_eq!(
            *returned, *input,
            "Full must preserve the complete event bytes"
        );
    }
    let mut run_a = Box::pin(EventNode::new(adapter_a.clone(), a_rx, Arc::clone(&broker)).run());
    let mut run_b = Box::pin(EventNode::new(adapter_b.clone(), b_rx, Arc::clone(&broker)).run());
    assert!(
        poll_fn(|cx| TaskPoll::Ready(run_a.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(
        poll_fn(|cx| TaskPoll::Ready(run_b.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert_eq!(adapter_a.taken.load(Ordering::SeqCst), 1);
    assert_eq!(adapter_b.taken.load(Ordering::SeqCst), 1);
    assert!(accepted_a.lock().unwrap().is_empty());
    assert!(accepted_b.lock().unwrap().is_empty());

    // Neither future runs between freeing the reservations and broker routing
    // these inputs. All payloads enter through normal target/ledger validation.
    drop(a_reservation);
    drop(b_reservation);
    assert!(matches!(
        broker.dispatch(into_a.clone()),
        Ok(DispatchOutcome::Enqueued(_))
    ));
    assert!(matches!(
        broker.dispatch(into_b.clone()),
        Ok(DispatchOutcome::Enqueued(_))
    ));
    let progress = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            assert!(
                poll_fn(|cx| TaskPoll::Ready(run_a.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            assert!(
                poll_fn(|cx| TaskPoll::Ready(run_b.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            if accepted_a.lock().unwrap().len() >= 2 && accepted_b.lock().unwrap().len() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await;
    assert!(
        progress.is_ok(),
        "pending outbound completions must not prevent each node from accepting the input that frees its peer: a={}, b={}",
        accepted_a.lock().unwrap().len(),
        accepted_b.lock().unwrap().len()
    );
    assert_eq!(
        *accepted_a.lock().unwrap(),
        vec![into_a, completion_b.clone()]
    );
    assert_eq!(
        *accepted_b.lock().unwrap(),
        vec![into_b, completion_a.clone()]
    );
    assert_eq!(adapter_a.taken.load(Ordering::SeqCst), 1);
    assert_eq!(adapter_b.taken.load(Ordering::SeqCst), 1);
    assert_eq!(
        broker.dispatch(completion_a),
        Ok(DispatchOutcome::Duplicate)
    );
    assert_eq!(
        broker.dispatch(completion_b),
        Ok(DispatchOutcome::Duplicate)
    );
    assert!(
        poll_fn(|cx| TaskPoll::Ready(run_a.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(
        poll_fn(|cx| TaskPoll::Ready(run_b.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert_eq!(
        accepted_a.lock().unwrap().len(),
        2,
        "no duplicate acceptance"
    );
    assert_eq!(
        accepted_b.lock().unwrap().len(),
        2,
        "no duplicate acceptance"
    );
}

#[tokio::test]
async fn a_held_full_completion_reports_a_destination_that_closes() {
    use std::future::Future;

    let own = Address::tcp("127.0.0.1", 52042);
    let (agent_tx, _agent_rx) = bounded_queue(1);
    let (outer_tx, _outer_rx) = bounded_queue(1);
    let (outbound_tx, _outbound_rx) = bounded_queue(1);
    let (a_tx, a_rx) = bounded_queue(1);
    let (b_tx, b_rx) = bounded_queue(1);
    let broker = Arc::new(EventBroker::new(
        own.clone(),
        agent_tx,
        outer_tx,
        outbound_tx,
        8,
    ));
    broker.register_node("a", 3, a_tx).unwrap();
    broker.register_node("b", 7, b_tx.clone()).unwrap();
    let completion = duplex_event(
        "closed-destination",
        Endpoint::node(own.clone(), "a", 3),
        Endpoint::node(own, "b", 7),
        &[0, 0xff, 4, 0],
    );
    let (publisher, mailbox) = completion_mailbox(1);
    publisher.try_publish(completion.clone()).unwrap();
    let accepted = Arc::new(Mutex::new(Vec::new()));
    let adapter = Arc::new(DuplexProbeAdapter {
        mailbox,
        accepted: Arc::clone(&accepted),
        taken: AtomicUsize::new(0),
    });
    let reservation = b_tx.try_reserve().unwrap();
    let mut run = Box::pin(EventNode::new(adapter.clone(), a_rx, Arc::clone(&broker)).run());
    assert!(
        poll_fn(|cx| TaskPoll::Ready(run.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert_eq!(adapter.taken.load(Ordering::SeqCst), 1);
    assert!(accepted.lock().unwrap().is_empty());
    drop(b_rx);
    drop(reservation);
    let result = tokio::time::timeout(Duration::from_secs(1), run)
        .await
        .expect("a closed destination must not remain in the Full retry loop");
    let closed = DispatchError::Closed(Delivery::Node {
        node: "b".into(),
        generation: 7,
    });
    let failure = result.unwrap_err();
    assert_eq!(failure.error, EventNodeError::Broker(closed.clone()));
    assert!(failure.held_input.is_none());
    assert_eq!(failure.held_output.as_deref(), Some(&completion));
    let rejected = broker.dispatch(completion.clone()).unwrap_err();
    assert_eq!(
        rejected.error, closed,
        "the rejected completion must not have been committed as a duplicate"
    );
    assert_eq!(*rejected.event, completion);
    assert!(accepted.lock().unwrap().is_empty());
}

struct BoundedDuplexProbe {
    inner: DuplexProbeAdapter,
    room: AtomicBool,
    refusals: AtomicUsize,
}

impl NodeAdapter for BoundedDuplexProbe {
    fn kind(&self) -> &str {
        self.inner.kind()
    }
    fn try_offer(&self, event: Event) -> Result<(), OfferError> {
        if !self.room.load(Ordering::SeqCst) {
            self.refusals.fetch_add(1, Ordering::SeqCst);
            return Err(OfferError::Full(event));
        }
        self.inner.try_offer(event)
    }
    fn try_take(&self) -> Poll {
        self.inner.try_take()
    }
    fn poll_take(&self, context: &mut Context<'_>) -> TaskPoll<Poll> {
        self.inner.poll_take(context)
    }
}

#[tokio::test]
async fn held_input_and_output_leave_the_second_event_in_each_bounded_queue() {
    use std::future::Future;

    let own = Address::tcp("127.0.0.1", 52043);
    let (agent_tx, _agent_rx) = bounded_queue(1);
    let (outer_tx, _outer_rx) = bounded_queue(1);
    let (outbound_tx, _outbound_rx) = bounded_queue(1);
    let (a_tx, a_rx) = bounded_queue(1);
    let (b_tx, mut b_rx) = bounded_queue(1);
    let broker = Arc::new(EventBroker::new(
        own.clone(),
        agent_tx,
        outer_tx,
        outbound_tx,
        16,
    ));
    broker.register_node("a", 3, a_tx).unwrap();
    broker.register_node("b", 7, b_tx.clone()).unwrap();
    let a = Endpoint::node(own.clone(), "a", 3);
    let b = Endpoint::node(own.clone(), "b", 7);
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    for sequence in 1..=3 {
        let mut input = duplex_event(
            &format!("bounded-input-{sequence}"),
            Endpoint::outer(own.clone(), "client", 1),
            a.clone(),
            &[0, 0xff, sequence as u8],
        );
        input.envelope.sequence = sequence;
        inputs.push(input);
        let mut output = duplex_event(
            &format!("bounded-output-{sequence}"),
            a.clone(),
            b.clone(),
            &[sequence as u8, 0x80, 0],
        );
        output.envelope.sequence = sequence;
        outputs.push(output);
    }
    let (publisher, mailbox) = completion_mailbox(1);
    publisher.try_publish(outputs[0].clone()).unwrap();
    broker.dispatch(inputs[0].clone()).unwrap();
    let accepted = Arc::new(Mutex::new(Vec::new()));
    let adapter = Arc::new(BoundedDuplexProbe {
        inner: DuplexProbeAdapter {
            mailbox,
            accepted: Arc::clone(&accepted),
            taken: AtomicUsize::new(0),
        },
        room: AtomicBool::new(false),
        refusals: AtomicUsize::new(0),
    });
    let destination_reservation = b_tx.try_reserve().unwrap();
    let mut run = Box::pin(EventNode::new(adapter.clone(), a_rx, Arc::clone(&broker)).run());
    assert!(
        poll_fn(|cx| TaskPoll::Ready(run.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(
        adapter.refusals.load(Ordering::SeqCst) > 0,
        "the first input is held"
    );
    assert_eq!(
        adapter.inner.taken.load(Ordering::SeqCst),
        1,
        "the first completion is held"
    );

    broker.dispatch(inputs[1].clone()).unwrap();
    publisher.try_publish(outputs[1].clone()).unwrap();
    for _ in 0..16 {
        assert!(
            poll_fn(|cx| TaskPoll::Ready(run.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(accepted.lock().unwrap().is_empty());
    assert_eq!(
        adapter.inner.taken.load(Ordering::SeqCst),
        1,
        "a held output must prevent consuming a second completion"
    );
    let Err(DispatchFailure {
        error: DispatchError::Full(delivery),
        event: returned,
    }) = broker.dispatch(inputs[2].clone())
    else {
        panic!("the second input must still occupy the capacity-one inbound queue");
    };
    assert_eq!(
        delivery,
        Delivery::Node {
            node: "a".into(),
            generation: 3
        }
    );
    assert_eq!(*returned, inputs[2]);
    assert_eq!(
        publisher.try_publish(outputs[2].clone()),
        Err(p4_adapter::node_adapter::PublishError::Full(
            outputs[2].clone()
        )),
        "the second completion must still occupy the capacity-one mailbox"
    );

    // This test creates room explicitly. It does not assert that a completely
    // saturated cyclic network can always create its own spare capacity.
    drop(destination_reservation);
    adapter.room.store(true, Ordering::SeqCst);
    let delivered = tokio::time::timeout(Duration::from_secs(1), async {
        let mut delivered = Vec::new();
        loop {
            assert!(
                poll_fn(|cx| TaskPoll::Ready(run.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            while let Ok(event) = b_rx.try_recv() {
                delivered.push(event);
            }
            if accepted.lock().unwrap().len() >= 2 && delivered.len() >= 2 {
                return delivered;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("each held event and its queued successor must progress when room returns");
    assert_eq!(*accepted.lock().unwrap(), inputs[..2]);
    assert_eq!(delivered, outputs[..2]);
    assert_eq!(adapter.inner.taken.load(Ordering::SeqCst), 2);
}

/// Completion is permanently Pending so the input-Closed branch is the only
/// terminal cause. ClosedAdapter above deliberately tests completion closure.
struct InputClosedAdapter;

impl NodeAdapter for InputClosedAdapter {
    fn kind(&self) -> &str {
        "input-closed-test"
    }
    fn try_offer(&self, event: Event) -> Result<(), OfferError> {
        Err(OfferError::Closed(event))
    }
    fn try_take(&self) -> Poll {
        Poll::Empty
    }
}

fn with_spare_allocation(mut event: Event) -> Event {
    event.payload.reserve(31);
    event.envelope.event_id.reserve(47);
    event.validate().unwrap();
    event
}

fn allocation(event: &Event) -> (usize, usize, usize, usize) {
    (
        event.payload.as_ptr() as usize,
        event.payload.capacity(),
        event.envelope.event_id.as_ptr() as usize,
        event.envelope.event_id.capacity(),
    )
}

#[tokio::test]
async fn adapter_closed_returns_the_original_inbound_allocation() {
    let own = Address::tcp("127.0.0.1", 52044);
    let (agent_tx, _agent_rx) = bounded_queue(1);
    let (outer_tx, _outer_rx) = bounded_queue(1);
    let (outbound_tx, _outbound_rx) = bounded_queue(1);
    let (node_tx, node_rx) = bounded_queue(1);
    let broker = Arc::new(EventBroker::new(
        own.clone(),
        agent_tx,
        outer_tx,
        outbound_tx,
        8,
    ));
    broker.register_node("n1", 1, node_tx.clone()).unwrap();
    let input = with_spare_allocation(event(&own));
    let expected = input.clone();
    let original_allocation = allocation(&input);
    // Move into the real EventReceiver. A successful broker dispatch makes
    // its own queue/ledger copies; this test starts at EventNode ownership.
    node_tx.try_send(input).unwrap();
    let failure = tokio::time::timeout(
        Duration::from_secs(1),
        EventNode::new(Arc::new(InputClosedAdapter), node_rx, broker).run(),
    )
    .await
    .expect("an input-Closed adapter must terminate its node")
    .unwrap_err();
    assert_eq!(failure.error, EventNodeError::AdapterClosed);
    let returned = failure
        .held_input
        .as_deref()
        .expect("rejected input is owned");
    assert_eq!(returned, &expected);
    assert_eq!(allocation(returned), original_allocation);
    assert!(failure.held_output.is_none());
}

#[tokio::test]
async fn broker_closed_returns_both_already_held_allocations() {
    use std::future::Future;

    let own = Address::tcp("127.0.0.1", 52045);
    let (agent_tx, _agent_rx) = bounded_queue(1);
    let (outer_tx, _outer_rx) = bounded_queue(1);
    let (outbound_tx, _outbound_rx) = bounded_queue(1);
    let (a_tx, a_rx) = bounded_queue(1);
    let (b_tx, b_rx) = bounded_queue(1);
    let broker = Arc::new(EventBroker::new(
        own.clone(),
        agent_tx,
        outer_tx,
        outbound_tx,
        8,
    ));
    broker.register_node("a", 3, a_tx.clone()).unwrap();
    broker.register_node("b", 7, b_tx.clone()).unwrap();
    let a = Endpoint::node(own.clone(), "a", 3);
    let input = with_spare_allocation(duplex_event(
        "terminal-input",
        Endpoint::outer(own.clone(), "client", 1),
        a.clone(),
        &[0xff, 0, 3],
    ));
    let expected_input = input.clone();
    let input_allocation = allocation(&input);
    let output = with_spare_allocation(duplex_event(
        "terminal-output",
        a,
        Endpoint::node(own, "b", 7),
        &[7, 0x80, 0],
    ));
    let expected_output = output.clone();
    let output_allocation = allocation(&output);
    let (publisher, mailbox) = completion_mailbox(1);
    let accepted = Arc::new(Mutex::new(Vec::new()));
    let adapter = Arc::new(BoundedDuplexProbe {
        inner: DuplexProbeAdapter {
            mailbox,
            accepted: Arc::clone(&accepted),
            taken: AtomicUsize::new(0),
        },
        room: AtomicBool::new(false),
        refusals: AtomicUsize::new(0),
    });
    a_tx.try_send(input).unwrap();
    let mut run = Box::pin(EventNode::new(adapter.clone(), a_rx, Arc::clone(&broker)).run());
    // Input is the only ready branch. Establish ownership before making the
    // completion available, independently of select's randomized branch order.
    assert!(
        poll_fn(|cx| TaskPoll::Ready(run.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(adapter.refusals.load(Ordering::SeqCst) > 0);
    assert_eq!(adapter.inner.taken.load(Ordering::SeqCst), 0);
    let reservation = b_tx.try_reserve().unwrap();
    publisher.try_publish(output).unwrap();
    assert!(
        poll_fn(|cx| TaskPoll::Ready(run.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert_eq!(adapter.inner.taken.load(Ordering::SeqCst), 1);
    assert_eq!(b_tx.capacity(), 0);
    assert!(accepted.lock().unwrap().is_empty());

    drop(b_rx);
    drop(reservation);
    let failure = tokio::time::timeout(Duration::from_secs(1), run)
        .await
        .expect("Closed must terminate the established Full retry")
        .unwrap_err();
    let closed = DispatchError::Closed(Delivery::Node {
        node: "b".into(),
        generation: 7,
    });
    assert_eq!(failure.error, EventNodeError::Broker(closed.clone()));
    let input = failure
        .held_input
        .as_deref()
        .expect("prior held input is owned");
    assert_eq!(input, &expected_input);
    assert_eq!(allocation(input), input_allocation);
    let output = failure
        .held_output
        .as_deref()
        .expect("rejected output is owned");
    assert_eq!(output, &expected_output);
    assert_eq!(allocation(output), output_allocation);
    assert!(accepted.lock().unwrap().is_empty());
    let rejected = broker.dispatch(*failure.held_output.unwrap()).unwrap_err();
    assert_eq!(
        rejected.error, closed,
        "terminal failure did not commit a duplicate"
    );
    assert_eq!(*rejected.event, expected_output);
    assert_eq!(allocation(&rejected.event), output_allocation);
}

#[tokio::test]
async fn completion_closed_returns_the_already_held_input_allocation() {
    use std::future::Future;

    let own = Address::tcp("127.0.0.1", 52046);
    let (agent_tx, _agent_rx) = bounded_queue(1);
    let (outer_tx, _outer_rx) = bounded_queue(1);
    let (outbound_tx, _outbound_rx) = bounded_queue(1);
    let (node_tx, node_rx) = bounded_queue(1);
    let broker = Arc::new(EventBroker::new(
        own.clone(),
        agent_tx,
        outer_tx,
        outbound_tx,
        8,
    ));
    broker.register_node("n1", 1, node_tx.clone()).unwrap();
    let input = with_spare_allocation(event(&own));
    let expected = input.clone();
    let original_allocation = allocation(&input);
    let (publisher, mailbox) = completion_mailbox(1);
    let accepted = Arc::new(Mutex::new(Vec::new()));
    let adapter = Arc::new(BoundedDuplexProbe {
        inner: DuplexProbeAdapter {
            mailbox,
            accepted: Arc::clone(&accepted),
            taken: AtomicUsize::new(0),
        },
        room: AtomicBool::new(false),
        refusals: AtomicUsize::new(0),
    });
    node_tx.try_send(input).unwrap();
    let mut run = Box::pin(EventNode::new(adapter.clone(), node_rx, broker).run());
    assert!(
        poll_fn(|cx| TaskPoll::Ready(run.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(adapter.refusals.load(Ordering::SeqCst) > 0);
    assert_eq!(adapter.inner.taken.load(Ordering::SeqCst), 0);
    assert_eq!(adapter.snapshot(), "");
    drop(publisher);
    let failure = tokio::time::timeout(Duration::from_secs(1), run)
        .await
        .expect("completion closure must wake the actual node loop")
        .unwrap_err();
    assert_eq!(
        failure.error,
        EventNodeError::CompletionClosed(String::new())
    );
    let returned = failure
        .held_input
        .as_deref()
        .expect("prior held input is owned");
    assert_eq!(returned, &expected);
    assert_eq!(allocation(returned), original_allocation);
    assert!(failure.held_output.is_none());
    assert!(accepted.lock().unwrap().is_empty());
}

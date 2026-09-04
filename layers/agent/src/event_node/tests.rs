use super::*;
use crate::event_broker::{Delivery, DispatchOutcome, bounded_queue};
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
    fn try_offer(&self, _event: Event) -> Result<(), OfferError> {
        Err(OfferError::Closed)
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
        let envelope = event.envelope.next(
            "complete",
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
        error,
        EventNodeError::CompletionClosed("logical_batch_failed:native exited 17".into())
    );
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
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert!(
        refusals.load(Ordering::SeqCst) > 1,
        "the node should retry a full adapter, not offer once",
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
    assert!(delivered.is_ok(), "the held event should go through once there is room");
    let taken = accepted.lock().unwrap();
    assert_eq!(taken.len(), 1, "held once, delivered once");
    assert_eq!(taken[0].payload, event(&own).payload);
    assert!(!task.is_finished());
    task.abort();
}

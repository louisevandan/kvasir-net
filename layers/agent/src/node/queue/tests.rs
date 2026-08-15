use super::*;
use p4_protocol::{Address, Envelope, QueueClass, Recipient};

fn frame(route: &str, lane: QueueClass) -> Frame {
    Frame {
        envelope: Envelope {
            target: Address::tcp("127.0.0.1", 19001),
            recipient: Recipient::node("n"),
            lane,
            route: route.into(),
            deadline_unix_ms: 0,
            reply_to: None,
            chain: None,
        },
        body: Vec::new(),
    }
}

#[test]
fn a_new_node_is_empty_and_idle() {
    let queue = NodeQueue::default();
    assert_eq!(queue.depth(), 0);
    assert!(!queue.is_running());
}

#[test]
fn depth_grows_with_what_the_agent_moved_in() {
    let queue = NodeQueue::default();
    queue.push(frame("a", QueueClass::Prefill));
    queue.push(frame("b", QueueClass::Prefill));
    assert_eq!(queue.depth(), 2);
}

#[test]
fn claiming_takes_the_named_work_and_marks_a_hop_running() {
    // One call, because taking work without marking would let a second hop
    // start beside the first.
    let queue = NodeQueue::default();
    queue.push(frame("a", QueueClass::Prefill));
    queue.push(frame("b", QueueClass::Prefill));
    queue.push(frame("c", QueueClass::Prefill));

    let claimed = queue.claim(&["a".into(), "c".into()]);
    assert_eq!(claimed.len(), 2);
    assert_eq!(queue.depth(), 1);
    assert!(queue.is_running());
}

#[test]
fn claiming_nothing_leaves_the_node_idle() {
    let queue = NodeQueue::default();
    queue.push(frame("a", QueueClass::Prefill));
    assert!(queue.claim(&[]).is_empty());
    assert!(!queue.is_running());
}

#[test]
fn a_node_returns_to_idle_only_when_the_hop_is_reported_finished() {
    let queue = NodeQueue::default();
    queue.push(frame("a", QueueClass::Decode));
    queue.claim(&["a".into()]);
    assert!(queue.is_running());

    queue.finished();
    assert!(!queue.is_running());
}

#[test]
fn the_scheduling_view_carries_lane_and_deadline() {
    let queue = NodeQueue::default();
    let mut expiring = frame("late", QueueClass::Decode);
    expiring.envelope.deadline_unix_ms = 500;
    queue.push(expiring);

    let waiting = queue.waiting();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].lane, QueueClass::Decode);
    assert_eq!(waiting[0].deadline_unix_ms, 500);
}

#[test]
fn removing_a_route_takes_it_out_for_cancellation() {
    let queue = NodeQueue::default();
    queue.push(frame("a", QueueClass::Prefill));
    queue.push(frame("b", QueueClass::Prefill));

    assert!(queue.remove("a").is_some());
    assert_eq!(queue.depth(), 1);
    assert!(queue.remove("a").is_none());
}

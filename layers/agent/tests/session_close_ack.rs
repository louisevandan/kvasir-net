//! The acknowledged half of the `SessionClose`/`SessionClosed` contract:
//! convergence after either message is lost once, and a stale answer fenced
//! rather than misapplied.
//!
//! `session_close.rs` (Stage 1) already proves delivery closes the measured
//! leak when every frame arrives. These tests are what that one could not
//! claim: that losing the *first* attempt of either message still
//! converges, because the sender keeps it pending and resends -- and that an
//! acknowledgement naming an entry nobody is waiting on is dropped rather
//! than acted on.
mod common;

use common::lossy::Drop;
use common::{
    Lifecycle, Outer, Silent, chain_over, request, start_with, start_with_behind_dropping, until,
};
use p4_agent_core::agent::Agent;
use p4_agent_core::node::payload::Payload;
use p4_mock::Mock;
use p4_mock::profile::Profile;
use p4_protocol::{Envelope, QueueClass, Recipient};
use std::sync::Arc;
use std::time::Duration;

fn eos_profile() -> Profile {
    // `eos_after_turns: Some(1)` against `remaining: 200` (see `request`
    // below) is what makes the tail stop early rather than at its length
    // bound -- the only case that ever produces a `SessionClose` at all.
    Profile {
        reserve_slots: true,
        eos_after_turns: Some(1),
        ..Profile::default()
    }
}

/// `close|` is `Lifecycle::close`'s own wire prefix (see `common/mod.rs`);
/// `closed|` is `Lifecycle::session_closed`'s. They share a leading byte on
/// purpose -- proving the drop targets the right one and not its sibling is
/// part of what these tests pin.
const SESSION_CLOSE_PREFIX: &[u8] = b"close|";
const SESSION_CLOSED_PREFIX: &[u8] = b"closed|";

/// Polls the two sides of the close handshake together: the receiver must
/// release its reservation and the sender must retire the acknowledged
/// pending close. Observing both accessors on the receiver can finish before
/// the acknowledgement has crossed back to the sender.
async fn until_close_converged(
    head: &Arc<Agent>,
    head_node: &str,
    tail: &Arc<Agent>,
    tail_node: &str,
) -> (Option<usize>, Option<usize>) {
    let mut seen = (None, None);
    for _ in 0..300 {
        seen = (
            head.node_reserved(head_node).await,
            tail.node_pending_closes(tail_node).await,
        );
        if seen == (Some(0), Some(0)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    seen
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_lost_first_session_close_still_converges() {
    let profile = eos_profile();
    let outer_duties = Outer::default();
    let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;

    // The close travels tail -> head, arriving at head's own listener, so
    // wrapping head (the receiver) is what puts it on the wire this test
    // controls. See `lossy::serve`'s own doc for why the reverse traffic
    // (head's replies, and everything this test injects locally) never
    // touches this relay at all.
    let drop = Drop::first_with_prefix(SESSION_CLOSE_PREFIX);
    let head =
        start_with_behind_dropping(Arc::new(Silent), Arc::new(Lifecycle), Arc::clone(&drop)).await;
    let tail = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
    head.create_node(
        "s0",
        Arc::new(Mock::staged(0, profile.clone())) as Arc<_>,
        2,
    )
    .await;
    tail.create_node(
        "s1",
        Arc::new(Mock::terminal(1, profile.clone())) as Arc<_>,
        2,
    )
    .await;

    let chain = chain_over(&[(&head, "s0"), (&tail, "s1")]);
    head.enqueue(request("r0", &chain, &outer, 200)).unwrap();

    until(|| {
        outer_duties
            .frames_for("r0")
            .iter()
            .any(|frame| frame.body == b"eos")
    })
    .await;

    let (head_reserved, tail_pending) = until_close_converged(&head, "s0", &tail, "s1").await;

    assert_eq!(
        drop.dropped(),
        1,
        "the first SessionClose must actually have been dropped, or convergence proves nothing"
    );
    assert_eq!(
        head_reserved,
        Some(0),
        "head's reservation must still clear even though the first close never arrived -- \
         the retry is what has to do this, not the first attempt"
    );
    assert_eq!(
        tail_pending,
        Some(0),
        "the tail must have seen its retry acknowledged and stopped holding it pending"
    );
    assert_eq!(
        tail.node_session_close_abandoned("s1").await,
        Some(0),
        "one retry was enough here -- this must converge well inside the bound, not exhaust it"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_lost_first_session_closed_still_converges_and_the_resent_close_is_handled_idempotently()
{
    let profile = eos_profile();
    let outer_duties = Outer::default();
    let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;

    let head = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
    // The acknowledgement travels head -> tail, arriving at the tail's own
    // listener, so this time the tail is the one wrapped. When the tail's
    // retry resends the close because no ack arrived, head receives
    // `SessionClose` a second time for a sequence it already released --
    // exactly the redelivery `Work::Close`'s own idempotence contract
    // exists for (see `p4_adapter::work::close::Close`'s doc) -- and answers
    // again rather than erroring or double-releasing anything.
    let drop = Drop::first_with_prefix(SESSION_CLOSED_PREFIX);
    let tail =
        start_with_behind_dropping(Arc::new(Silent), Arc::new(Lifecycle), Arc::clone(&drop)).await;
    head.create_node(
        "s0",
        Arc::new(Mock::staged(0, profile.clone())) as Arc<_>,
        2,
    )
    .await;
    tail.create_node(
        "s1",
        Arc::new(Mock::terminal(1, profile.clone())) as Arc<_>,
        2,
    )
    .await;

    let chain = chain_over(&[(&head, "s0"), (&tail, "s1")]);
    head.enqueue(request("r0", &chain, &outer, 200)).unwrap();

    until(|| {
        outer_duties
            .frames_for("r0")
            .iter()
            .any(|frame| frame.body == b"eos")
    })
    .await;

    // Waited for here, not for head's own reservation: head clears its
    // reservation the moment it processes the *first* close, before the
    // (about to be dropped) ack for it is even built -- that would converge
    // trivially and prove nothing about the retry. What only a successful
    // second attempt can produce is the tail's own pending entry actually
    // clearing.
    let mut tail_pending = tail.node_pending_closes("s1").await;
    for _ in 0..300 {
        if tail_pending == Some(0) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        tail_pending = tail.node_pending_closes("s1").await;
    }
    let head_reserved = head.node_reserved("s0").await;

    assert_eq!(
        drop.dropped(),
        1,
        "the first SessionClosed must actually have been dropped, or convergence proves nothing"
    );
    // The dropped ack was itself an answer to a close head genuinely
    // processed once already -- this is not double counting, it is proof a
    // second, distinct close (the tail's retry) also reached head and was
    // also handled, and answered again.
    assert!(
        drop.seen() >= 2,
        "head must have answered at least twice for this relay to see a second ack to drop-count against, \
         seen={}",
        drop.seen()
    );
    assert_eq!(
        tail_pending,
        Some(0),
        "the tail must eventually see an acknowledgement land, even though the first one never arrived"
    );
    assert_eq!(
        head_reserved,
        Some(0),
        "head's own reservation must be clear regardless of which attempt's ack made it through"
    );
    assert_eq!(
        tail.node_session_close_abandoned("s1").await,
        Some(0),
        "one retry was enough here -- this must converge well inside the bound, not exhaust it"
    );
}

/// Not a chain scenario at all: a bare acknowledgement, naming a `close_id`
/// this node never sent a close under, delivered straight at a node that has
/// nothing pending. Proves the fence in `Node::observe_session_closed`
/// without needing a real close to have happened first -- an empty pending
/// table is the simplest case that "matches nothing" can mean.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_session_closed_naming_nothing_pending_is_dropped_not_acted_on() {
    let agent = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
    let profile = eos_profile();
    agent
        .create_node("s1", Arc::new(Mock::terminal(0, profile)) as Arc<_>, 2)
        .await;

    assert_eq!(agent.node_pending_closes("s1").await, Some(0));

    let ghost = Envelope {
        target: agent.address().clone(),
        recipient: Recipient::node("s1"),
        lane: QueueClass::Control,
        route: "ghost-route".into(),
        request_id: "ghost-request".into(),
        stream_id: "ghost-stream".into(),
        origin_agent: None,
        return_channel: None,
        ingress_generation: 0,
        event_seq: 0,
        deadline_unix_ms: 0,
        reply_to: None,
        chain: None,
    };
    agent
        .enqueue(p4_protocol::frame::Frame {
            envelope: ghost,
            body: Lifecycle.session_closed("ghost-sequence", 999),
        })
        .unwrap();

    let mut stale = agent.node_session_closed_stale("s1").await;
    for _ in 0..100 {
        if stale == Some(1) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        stale = agent.node_session_closed_stale("s1").await;
    }

    assert_eq!(
        stale,
        Some(1),
        "an acknowledgement matching nothing pending must be counted as stale"
    );
    assert_eq!(
        agent.node_session_closed_acked("s1").await,
        Some(0),
        "and must never be counted as having retired anything"
    );
    assert_eq!(
        agent.node_pending_closes("s1").await,
        Some(0),
        "there was nothing pending before, and there must be nothing pending after"
    );
    // Confirmed alive rather than left in some half-handled state: the node
    // is still there, still at rest.
    assert_eq!(agent.node_depth("s1").await, Some(0));
}

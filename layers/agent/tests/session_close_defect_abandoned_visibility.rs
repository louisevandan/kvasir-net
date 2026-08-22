//! Round 2 of the `SessionClose`/`SessionClosed` contract, defect 3 of 3
//! (brief section 7.3): giving up after the bounded retry schedule must be
//! visible on the one line an operator actually reads (`Agent::node_counts`),
//! not only through a dedicated accessor.
//!
//! Split out of `session_close_defects.rs` on line count alone, once round 3
//! pushed that file past 400 lines -- `eos_profile` and `epoch_request`
//! duplicated here rather than shared, the same tradeoff `common::Lifecycle`'s
//! own doc and `tests_hop.inc.rs`'s header describe: a handful of tiny
//! fixtures are cheaper to repeat per binary than a shared-fixture module
//! across three of them.
mod common;

use common::lossy::Drop;
use common::{Lifecycle, Outer, Silent, chain_over, start_with, start_with_behind_dropping, until};
use p4_agent_core::agent::Agent;
use p4_mock::Mock;
use p4_mock::profile::Profile;
use p4_protocol::frame::Frame;
use p4_protocol::{Chain, Envelope, QueueClass, Recipient};
use std::sync::Arc;
use std::time::Duration;

fn eos_profile() -> Profile {
    Profile {
        reserve_slots: true,
        eos_after_turns: Some(1),
        // Slow enough that a test can observe an intermediate state (a new
        // session's reservation, mid-flight) before the sequence races to
        // its own natural completion -- fast enough that the whole suite
        // still runs in well under a second.
        leading_hop: Duration::from_millis(60),
        trailing_hop: Duration::from_millis(60),
        ..Profile::default()
    }
}

const SESSION_CLOSE_PREFIX: &[u8] = b"close|";

/// A hop-shaped request naming its own session identity, read back by
/// `Lifecycle::session_epoch` -- see `common::Lifecycle`'s own doc for the
/// `"{epoch}|{prompt}|{remaining}"` encoding.
fn epoch_request(route: &str, chain: &Chain, outer: &Arc<Agent>, tokens: u32, epoch: u64) -> Frame {
    Frame {
        envelope: Envelope {
            target: chain.current().address.clone(),
            recipient: Recipient::node(chain.current().node.clone()),
            lane: QueueClass::Prefill,
            route: route.into(),
            request_id: route.into(),
            stream_id: route.into(),
            origin_agent: Some(outer.address().clone()),
            return_channel: Some(outer.address().to_string()),
            ingress_generation: 0,
            event_seq: 0,
            deadline_unix_ms: 0,
            reply_to: Some(outer.address().clone()),
            chain: Some(chain.clone()),
        },
        body: format!("{epoch}|prompt|{tokens}").into_bytes(),
    }
}

/// Defect 3 (brief section 7.3): giving up after the bounded retry schedule
/// must show up on the operator-facing line (`Agent::node_counts`), not
/// only through `Handle::counts()`'s own accessor. Unfixed, `node_counts`
/// never mentions `reserved`, `pending_closes`, `session_close_abandoned`,
/// `session_closed_acked` or `session_closed_stale` at all, so an operator
/// reading that line during an incident sees nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn an_abandoned_close_is_visible_on_the_operator_facing_line() {
    let profile = eos_profile();
    let outer_duties = Outer::default();
    let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;

    // Every SessionClose, not only the first, is dropped in this direction
    // -- a permanently unreachable head, as opposed to one lost frame.
    let drop = Drop::always_with_prefix(SESSION_CLOSE_PREFIX);
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

    head.enqueue(epoch_request("shared", &chain, &outer, 200, 1))
        .unwrap();
    until(|| {
        outer_duties
            .frames_for("shared")
            .iter()
            .any(|frame| frame.body == b"eos")
    })
    .await;

    let mut abandoned = tail.node_session_close_abandoned("s1").await;
    for _ in 0..100 {
        if abandoned == Some(1) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        abandoned = tail.node_session_close_abandoned("s1").await;
    }
    assert_eq!(
        abandoned,
        Some(1),
        "every attempt was dropped, so the tail's own bound must have been exhausted"
    );
    assert!(
        drop.dropped() >= 5,
        "all five attempts (the bound) must actually have been sent and dropped, saw {}",
        drop.dropped()
    );

    let lines = tail.node_counts().await;
    let s1_line = lines
        .iter()
        .find(|line| line.contains("node=s1"))
        .expect("s1 must report a counts line");
    assert!(
        s1_line.contains("session_close_abandoned=1"),
        "the abandonment must be visible on the operator-facing line: {s1_line}"
    );
    assert!(
        s1_line.contains("pending_closes=0"),
        "the abandoned entry must no longer be pending either: {s1_line}"
    );
    assert!(
        s1_line.contains("session_closed_acked=0"),
        "nothing was ever actually acknowledged: {s1_line}"
    );
    assert!(
        s1_line.contains("reserved="),
        "the reservation gauge itself must be on the same line an operator reads: {s1_line}"
    );
}

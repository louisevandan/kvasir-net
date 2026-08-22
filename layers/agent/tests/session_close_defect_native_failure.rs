//! Round 2 of the `SessionClose`/`SessionClosed` contract, defect 2 of 3
//! (brief section 7.2): a native close failure must not free P4's own
//! admission for a slot the backend never confirmed releasing
//! (`close_fence::fail_session_close` / `session_epoch_conflict`).
//!
//! Split out of `session_close_defects.rs` on line count alone, once round 3
//! pushed that file past 400 lines -- `eos_profile`, `epoch_request` and
//! `until_reply_containing` duplicated here rather than shared, the same
//! tradeoff `common::Lifecycle`'s own doc and `tests_hop.inc.rs`'s header
//! describe: a handful of tiny fixtures are cheaper to repeat per binary
//! than a shared-fixture module across three of them.
mod common;

use common::{Lifecycle, Outer, Silent, chain_over, start_with, until};
use p4_agent_core::agent::Agent;
use p4_mock::Mock;
use p4_mock::profile::{Fault, Profile};
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

/// A same-route reply body containing `needle`, once one shows up. Bounded
/// so a defect that never produces the reply fails the test instead of
/// hanging it.
async fn until_reply_containing(outer: &Outer, route: &str, needle: &str) -> bool {
    for _ in 0..300 {
        if outer
            .frames_for(route)
            .iter()
            .any(|frame| String::from_utf8_lossy(&frame.body).contains(needle))
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

/// Defect 2 (brief section 7.2): a native close failure must not free P4's
/// own admission for a slot the backend never confirmed releasing.
/// Unfixed, `Event::Failed` for a `Work::Close` falls into the generic
/// hop-failure path, which removes the sequence from `active_sequences`
/// regardless -- reopening admission over a slot that may still be held.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_native_close_failure_keeps_the_reservation_and_refuses_new_admission() {
    let mut head_profile = eos_profile();
    head_profile.fault = Fault::Close;
    let tail_profile = eos_profile();

    let outer_duties = Outer::default();
    let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;

    let head = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
    let tail = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
    head.create_node("s0", Arc::new(Mock::staged(0, head_profile)) as Arc<_>, 2)
        .await;
    tail.create_node("s1", Arc::new(Mock::terminal(1, tail_profile)) as Arc<_>, 2)
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

    // head's adapter always fails Work::Close -- the reservation must never
    // clear, checked repeatedly across the tail's whole retry/abandon window
    // (five attempts at 250ms) so a transient "still 1 for a moment" cannot
    // be mistaken for the real fix.
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(
            head.node_reserved("s0").await,
            Some(1),
            "a native close failure must leave the reservation held, not free it"
        );
    }

    // No acknowledgement was ever produced: the tail exhausted its bound and
    // abandoned rather than having anything to retire. Polled rather than
    // checked once: the reservation loop above already spent well over the
    // five-attempt bound, but this waits its own bounded window independent
    // of that timing so the assertion cannot be sensitive to exactly when
    // the first close was sent relative to `until`'s own return.
    let mut abandoned = tail.node_session_close_abandoned("s1").await;
    for _ in 0..100 {
        if abandoned == Some(1) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        abandoned = tail.node_session_close_abandoned("s1").await;
    }
    assert_eq!(
        tail.node_session_closed_acked("s1").await,
        Some(0),
        "a failed native close must never be acknowledged"
    );
    assert_eq!(
        abandoned,
        Some(1),
        "the tail's bounded retry must have given up after its own five attempts"
    );

    // A new admission naming the same sequence id must be refused, not
    // raced: the reservation is still epoch 1's, so an epoch-2 attempt must
    // never be admitted onto it.
    head.enqueue(epoch_request("shared", &chain, &outer, 200, 2))
        .unwrap();
    assert!(
        until_reply_containing(&outer_duties, "shared", "different session").await,
        "a second session naming the same sequence id must be refused while \
         the first close's own release is unconfirmed"
    );
    assert_eq!(
        head.node_reserved("s0").await,
        Some(1),
        "the refused admission must not have touched the held reservation either"
    );
}

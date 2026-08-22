//! Round 2 of the `SessionClose`/`SessionClosed` contract, defect 1 of 3
//! (brief section 7.1): a retransmitted close, arriving after its own
//! session already released and a *new* session took the same sequence id,
//! must never touch that new session's reservation
//! (`close_fence::stale_session_close`).
//!
//! Split out of `session_close_defects.rs` on line count alone, once round 3
//! pushed that file past 400 lines -- each of the three defects it pinned
//! has its own file now, `epoch_request` and `close_retransmit` duplicated
//! here rather than shared, the same tradeoff `common::Lifecycle`'s own doc
//! and `tests_hop.inc.rs`'s header describe: a handful of tiny fixtures are
//! cheaper to repeat per binary than a shared-fixture module across three of
//! them.
mod common;

use common::{Lifecycle, Outer, Silent, chain_over, start_with, until};
use p4_agent_core::agent::Agent;
use p4_agent_core::node::payload::Payload;
use p4_mock::Mock;
use p4_mock::profile::Profile;
use p4_protocol::frame::Frame;
use p4_protocol::{Chain, Envelope, QueueClass, Recipient};
use std::sync::Arc;
use std::time::Duration;

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

/// Fabricates exactly the frame a `SessionClose` retransmit carries: the
/// same sequence, `close_id` and `session_epoch` its original transmission
/// did. Addressed at the chain's first link the way a real close is
/// (`outcome::close::session_close_frames` positions it there), with the
/// chain's last link as who the acknowledgement answers.
fn close_retransmit(sequence: &str, chain: &Chain, close_id: u64, session_epoch: u64) -> Frame {
    let positioned = Chain::at(chain.links().to_vec(), 0).expect("chain has at least one link");
    let target = positioned.current().address.clone();
    let node = positioned.current().node.clone();
    Frame {
        envelope: Envelope {
            target,
            recipient: Recipient::node(node),
            lane: QueueClass::Control,
            route: format!("close:{sequence}"),
            request_id: sequence.into(),
            stream_id: sequence.into(),
            origin_agent: None,
            return_channel: None,
            ingress_generation: 0,
            event_seq: 0,
            deadline_unix_ms: 0,
            reply_to: None,
            chain: Some(positioned),
        },
        body: Lifecycle.close(sequence, close_id, session_epoch),
    }
}

/// Defect 1 (brief section 7.1): `close_id` alone does not fence a
/// retransmitted close against a *new* session that reused the same
/// sequence id after the old one's reservation was genuinely released.
/// Unfixed, `stale_session_close` never runs, the retransmit reaches
/// `Work::Close` unchanged, and the adapter releases whatever this node
/// currently holds for the sequence -- session B's live reservation, not
/// session A's already-gone one.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_stale_retransmitted_close_does_not_touch_a_new_sessions_reservation() {
    // No `eos_after_turns` here on purpose: both sessions below finish by
    // hitting their own requested token bound, which lets each one's
    // lifetime be controlled by its own `max_tokens` instead of a turn count
    // shared by both. Session A asks for one token (converges almost
    // immediately, to set up the "already released" state); session B asks
    // for several (stays reserved for multiple decode laps -- comfortably
    // longer than the single lap the injected retransmit needs to be
    // wrongly processed in, if it is ever going to be).
    let profile = Profile {
        reserve_slots: true,
        leading_hop: Duration::from_millis(30),
        trailing_hop: Duration::from_millis(30),
        ..Profile::default()
    };
    let outer_duties = Outer::default();
    let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;

    let head = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
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

    // Session A, epoch 1, on sequence "shared" -- one requested token, so it
    // reaches its own length terminal after a single round trip and runs to
    // a genuine, fully acknowledged release. Both sides converge to zero
    // before B ever starts, so B's own reservation is the only one live
    // when the retransmit lands.
    head.enqueue(epoch_request("shared", &chain, &outer, 1, 1))
        .unwrap();
    // Waited for explicitly: without this, the convergence poll below can
    // be satisfied vacuously at t=0, before A's hop has even been claimed
    // -- "nothing reserved yet" and "reserved, then released" both read as
    // `Some(0)`, and only the reply proves which one actually happened.
    // p4_mock's own remaining-exhausted stop reason is "stop", not P4's
    // length-terminal string -- the mock decides `Outcome::stop` itself
    // once a sequence's own `remaining` is spent, ahead of `outcome::next`'s
    // separate length-bound check ever running.
    until(|| {
        outer_duties
            .frames_for("shared")
            .iter()
            .any(|frame| frame.body == b"stop")
    })
    .await;
    let mut head_reserved = head.node_reserved("s0").await;
    let mut tail_pending = tail.node_pending_closes("s1").await;
    for _ in 0..300 {
        if head_reserved == Some(0) && tail_pending == Some(0) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        head_reserved = head.node_reserved("s0").await;
        tail_pending = tail.node_pending_closes("s1").await;
    }
    assert_eq!(
        head_reserved,
        Some(0),
        "session A must have fully released before B starts"
    );
    assert_eq!(
        tail_pending,
        Some(0),
        "A's own close/ack round trip must have converged first"
    );

    // Session B, epoch 2, the SAME sequence id -- exactly the reuse
    // `tools/drive`'s own `Admission::retry` performs deliberately. A large
    // token budget keeps it decoding far longer than this test's own
    // observation window below: p4_mock's turn counter for a sequence id is
    // its own resident state, keyed by that id alone (`execution.rs`'s
    // `produced` map) and is never reset between two sessions that reuse
    // one -- so B's own turn count starts wherever A's left off, and a
    // small budget here would let B reach its own genuine terminal inside
    // the observation window, which would prove nothing about the
    // retransmit.
    head.enqueue(epoch_request("shared", &chain, &outer, 500, 2))
        .unwrap();
    let mut reserved_for_b = None;
    for _ in 0..50 {
        reserved_for_b = head.node_reserved("s0").await;
        if reserved_for_b == Some(1) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        reserved_for_b,
        Some(1),
        "session B must have reserved head's slot for the same sequence id A just freed"
    );

    // The retransmit: same sequence, same close_id (the tail's first-ever
    // close, minted at 1) and the same session_epoch A's real close carried.
    // head is mid-lap for B when this arrives, so it cannot even be
    // dispatched to the adapter until that lap's own hop completes --
    // giving it up to one hop's worth of head start before this test's own
    // checks below would ever see an effect.
    head.enqueue(close_retransmit("shared", &chain, 1, 1))
        .unwrap();

    // Checked repeatedly across several of B's own decode laps (comfortably
    // inside its ~600ms natural lifetime), not once: a single early check
    // could pass merely because the retransmit had not been dispatched to
    // the adapter yet, proving nothing about whether the fence actually
    // ran.
    for _ in 0..15 {
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(
            head.node_reserved("s0").await,
            Some(1),
            "the bug this pins: an unfenced node calls its adapter's Close \
             for 'shared', releasing session B's own still-live reservation \
             instead of leaving it alone"
        );
    }
}

//! `ActiveHopLane`'s narrowing, driven through the real production path.
//!
//! `status::typed_snapshot()` is the one place `QueueClass` (four-valued,
//! in-process telemetry) crosses onto the wire as `ActiveHopLane`
//! (two-valued, schema 6's historical domain). Every other test that touches
//! `ActiveHopLane` constructs an `ActiveHopSnapshot` directly and never runs
//! the conversion — see `status::mod`'s `From<QueueClass> for ActiveHopLane`.
//! This file closes that gap: a real `Decode`-lane hop, composed by the real
//! node scheduler, read back through a real status reply that crossed a real
//! TCP socket and was wire-decoded on the other end.

mod common;

use common::deployment::place;
use common::{Outer, backends, chain_over, runtime, start, to_agent, to_node, until};
use p4_protocol::QueueClass;
use p4_service::Standard;
use p4_service::message::{Reply, ToAgent, ToNode};
use p4_service::status::ActiveHopLane;
use std::sync::Arc;

/// A hop composed entirely from queued `Decode` work must be reported as
/// `ActiveHopLane::Decode`, not narrowed to `Prefill` by the conversion that
/// exists to keep this field's wire domain at two values.
///
/// `mock-slow` holds its hop long enough (60ms leading, 60ms trailing) for a
/// status request sent immediately after enqueue to land while the hop is
/// still active; nothing here races the backend to observe it.
#[test]
fn a_decode_lane_hop_reports_decode_through_the_wire_status_reply() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(
            &agent,
            &outer,
            &seen,
            "slow",
            "mock-slow",
            r#"{"l":"0-9"}"#,
            4,
        )
        .await;

        let single = chain_over(&[(&agent, "slow")]);
        agent
            .enqueue(to_node(
                &single,
                &outer,
                "decode-req",
                QueueClass::Decode,
                ToNode::Execute {
                    prompt: "디코드 랩".into(),
                    max_tokens: 6,
                    options: "{}".into(),
                },
            ))
            .unwrap();

        // Poll status until a reply shows the hop still in flight. Each
        // status request is answered over the same socket a real OUTER
        // would use, so the snapshot this test inspects went through
        // `typed_snapshot()`, `encode_reply`, a TCP write/read, and
        // `decode_reply` -- not an in-process shortcut.
        until(|| {
            agent
                .enqueue(to_agent(&agent, &outer, "decode-status", ToAgent::Status))
                .unwrap();
            seen.replies("decode-status").iter().any(|reply| {
                matches!(reply, Reply::StatusSnapshot { snapshot }
                    if snapshot.nodes.iter().any(|node| node.active_hop.is_some()))
            })
        })
        .await;

        let active_lane = seen
            .replies("decode-status")
            .into_iter()
            .rev()
            .find_map(|reply| match reply {
                Reply::StatusSnapshot { snapshot } => snapshot
                    .nodes
                    .iter()
                    .find_map(|node| node.active_hop.as_ref().map(|hop| hop.lane)),
                _ => None,
            })
            .expect("a status snapshot with the decode hop still active");

        assert_eq!(
            active_lane,
            ActiveHopLane::Decode,
            "a hop composed entirely from the decode lane arrived over the \
             wire as something other than Decode -- From<QueueClass> for \
             ActiveHopLane is inverted or otherwise wrong"
        );
    });
}

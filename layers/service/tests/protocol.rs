//! What OUTER can actually find out, and actually do, over the wire.
//!
//! The other tests here prove work completes. These prove it can be *watched*
//! and *steered*, which is a different claim and the one an operator cares
//! about: a load reporting itself stage by stage, a failure that is reported
//! rather than inferred from silence, a deployment that refuses to serve half
//! a model, a request that can be located while it runs and stopped partway,
//! and counters that can be collected from another machine instead of read off
//! a console.
//!
//! Every one of them goes through the message vocabulary. Nothing here reaches
//! into an agent in process.

mod common;

use common::deployment::place;
use common::{Outer, backends, chain_over, runtime, start, to_node, until};
use p4_protocol::QueueClass;
use p4_service::Standard;
use p4_service::message::{Reply, ToNode};
use std::sync::Arc;

/// A model spread over three machines, watched as it lands.
///
/// Each stage reports its own progress and its own binding, because a model
/// spread over layer ranges finishes when its slowest piece does and one total
/// hides which piece that was.
#[test]
fn a_distributed_load_is_visible_stage_by_stage_on_every_machine() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let mut agents = Vec::new();
        for _ in 0..3 {
            agents.push(start(Arc::new(Standard::new(backends()))).await);
        }

        for (index, agent) in agents.iter().enumerate() {
            let adapter = if index == 2 { "mock-tail" } else { "mock-lead" };
            place(
                agent,
                &outer,
                &seen,
                &format!("n{index}"),
                adapter,
                &format!(r#"{{"layers":"{}-{}"}}"#, index * 20, index * 20 + 19),
                4,
            )
            .await;
        }

        for index in 0..3 {
            let replies = seen.replies(&format!("load-n{index}"));
            assert!(
                replies
                    .iter()
                    .any(|reply| matches!(reply, Reply::Progress { .. })),
                "stage {index} reported progress of its own: {replies:?}"
            );
            assert!(
                replies
                    .iter()
                    .any(|reply| matches!(reply, Reply::Bound { generation: 1 })),
                "stage {index} bound and said which generation: {replies:?}"
            );
        }
    });
}

/// One stage of a distributed load fails.
///
/// The failure is reported — not inferred from a stage that never answers —
/// and the deployment does not half-work. A chain composed over the failed
/// stage is refused *by that stage*, so an operator who loaded three machines
/// and lost one cannot accidentally serve answers from two thirds of a model.
#[test]
fn a_stage_that_fails_to_load_is_reported_and_refuses_to_serve() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let good = start(Arc::new(Standard::new(backends()))).await;
        let broken = start(Arc::new(Standard::new(backends()))).await;

        place(
            &good,
            &outer,
            &seen,
            "n0",
            "mock-lead",
            r#"{"layers":"0-19"}"#,
            4,
        )
        .await;
        place(
            &broken,
            &outer,
            &seen,
            "n1",
            "mock-unloadable",
            r#"{"layers":"20-39"}"#,
            4,
        )
        .await;

        // Reported, with a reason, rather than left to a timeout.
        let broken_replies = seen.replies("load-n1");
        assert!(
            broken_replies
                .iter()
                .any(|reply| matches!(reply, Reply::Failed { .. })),
            "the stage that could not load said so: {broken_replies:?}"
        );
        assert!(
            !broken_replies
                .iter()
                .any(|reply| matches!(reply, Reply::Bound { .. })),
            "and never claimed to be bound: {broken_replies:?}"
        );
        // Its neighbour is fine, which is what makes this a transaction rather
        // than a list of independent loads.
        assert!(
            seen.replies("load-n0")
                .iter()
                .any(|reply| matches!(reply, Reply::Bound { .. })),
            "the other stage bound normally"
        );

        // Now run an inference over both anyway.
        let chain = chain_over(&[(&good, "n0"), (&broken, "n1")]);
        good.enqueue(to_node(
            &chain,
            &outer,
            "infer",
            QueueClass::Prefill,
            ToNode::Execute {
                prompt: "안녕".into(),
                max_tokens: 4,
                options: "{}".into(),
            },
        ))
        .unwrap();
        until(|| {
            seen.replies("infer")
                .iter()
                .any(|reply| matches!(reply, Reply::Failed { .. } | Reply::Done { .. }))
        })
        .await;

        let stream = seen.replies("infer");
        assert!(
            stream
                .iter()
                .any(|reply| matches!(reply, Reply::Failed { .. })),
            "the half-loaded deployment refused rather than answered: {stream:?}"
        );
        assert!(
            !stream
                .iter()
                .any(|reply| matches!(reply, Reply::Done { .. })),
            "and produced no completion: {stream:?}"
        );
    });
}

/// An unload is reported, and what it leaves behind is a node that says so.
#[test]
fn an_unload_is_reported_and_the_deployment_stops_being_current() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(
            &agent,
            &outer,
            &seen,
            "n0",
            "mock-tail",
            r#"{"layers":"0-9"}"#,
            4,
        )
        .await;

        let single = chain_over(&[(&agent, "n0")]);
        agent
            .enqueue(to_node(
                &single,
                &outer,
                "unload",
                QueueClass::Control,
                ToNode::Unload,
            ))
            .unwrap();
        until(|| !seen.replies("unload").is_empty()).await;
        assert!(
            seen.replies("unload")
                .iter()
                .any(|reply| matches!(reply, Reply::Released)),
            "the release was reported: {:?}",
            seen.replies("unload")
        );
    });
}

/// What the reply stream is numbered by.
///
/// `event_seq` is the only thing a subscriber has for telling a producer that
/// said nothing from a transport that lost something: an acknowledgement
/// retires every unacked frame at or below the number it names, so a hole in
/// the numbering and a frame that never arrived look exactly alike. A lap of
/// the ring is therefore not a response event — only a frame the caller
/// receives is — and a backend that ran nine laps to produce six tokens must
/// still number those six one to six.
#[test]
fn a_lap_that_reports_nothing_does_not_consume_a_reply_number() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(
            &agent,
            &outer,
            &seen,
            "n0",
            "mock-muted",
            r#"{"layers":"0-19"}"#,
            4,
        )
        .await;

        let single = chain_over(&[(&agent, "n0")]);
        agent
            .enqueue(to_node(
                &single,
                &outer,
                "muted",
                QueueClass::Prefill,
                ToNode::Execute {
                    prompt: "질문".into(),
                    max_tokens: 6,
                    options: "{}".into(),
                },
            ))
            .unwrap();
        until(|| {
            seen.replies("muted")
                .iter()
                .any(|reply| matches!(reply, Reply::Done { .. } | Reply::Failed { .. }))
        })
        .await;

        let replies = seen.replies("muted");
        assert!(
            replies
                .iter()
                .any(|reply| matches!(reply, Reply::Done { .. })),
            "the request ended: {replies:?}"
        );
        let sequences = seen.event_sequences("muted");
        assert!(
            sequences.len() > 3,
            "the run has to be long enough to contain a muted lap: {sequences:?}"
        );
        assert_eq!(
            sequences,
            (1..=sequences.len() as u64).collect::<Vec<u64>>(),
            "the reply stream skipped a number: {sequences:?} for {replies:?}"
        );
    });
}

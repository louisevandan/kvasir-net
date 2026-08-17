//! What a cache verb does to a deployment, and to a chain.
//!
//! The verbs themselves are next door in `cache.rs`. These two are about the
//! things around them: that persisting state does not disturb what a node is
//! bound to, and that a conversation living on several machines is persisted
//! and restored on every one of them rather than on whichever answered first.

mod common;

use common::conversation::{cache_op, infer, place};
use common::{Outer, backends, chain_over, runtime, start, to_agent, to_node, until};
use p4_protocol::QueueClass;
use p4_service::Standard;
use p4_service::message::{Reply, ToAgent, ToNode};
use std::sync::Arc;

/// A cache instruction does not disturb the deployment.
///
/// Persisting a conversation says nothing about which model is loaded, so the
/// node must still be bound afterwards and must still serve — the check that
/// would have caught treating a sequence instruction as a lifecycle one.
#[test]
fn cache_work_leaves_the_deployment_bound_and_serving() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(&agent, &outer, &seen, "n0").await;

        infer(&agent, &outer, &seen, "n0", "first", 3).await;
        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "persist",
            ToNode::Persist {
                sequence: "first".into(),
            },
        )
        .await;

        // A different request, after the cache work, on the same deployment.
        infer(&agent, &outer, &seen, "n0", "second", 3).await;
        assert!(
            seen.replies("second")
                .iter()
                .any(|reply| matches!(reply, Reply::Done { .. })),
            "the node still serves: {:?}",
            seen.replies("second")
        );
        assert_eq!(
            agent.node_depth("n0").await,
            Some(0),
            "and is not holding the instruction open"
        );
    });
}

/// A conversation spread over a chain.
///
/// Each stage holds its own shard of the sequence's state, so persisting a
/// conversation is not one instruction but one per stage — the same shape as a
/// load, and with the same consequence: a conversation half-restored is worse
/// than one not restored at all, because it would answer.
#[test]
fn a_conversation_spread_over_a_chain_persists_and_restores_on_every_stage() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let head = start(Arc::new(Standard::new(backends()))).await;
        let tail = start(Arc::new(Standard::new(backends()))).await;

        for (agent, node, adapter) in [(&head, "h", "mock-lead"), (&tail, "t", "mock-tail")] {
            agent
                .enqueue(to_agent(
                    agent,
                    &outer,
                    &format!("create-{node}"),
                    ToAgent::CreateNode {
                        node: node.into(),
                        adapter: adapter.into(),
                    },
                ))
                .unwrap();
            until(|| !seen.replies(&format!("create-{node}")).is_empty()).await;
            let single = chain_over(&[(agent, node)]);
            agent
                .enqueue(to_node(
                    &single,
                    &outer,
                    &format!("load-{node}"),
                    QueueClass::Control,
                    ToNode::Load {
                        plan: r#"{"layers":"0-19"}"#.into(),
                        artifact: "model.gguf".into(),
                        ceiling: 4,
                        capability_snapshot_id: String::new(),
                        capability_expires_at: 0,
                    },
                ))
                .unwrap();
            until(|| {
                seen.replies(&format!("load-{node}"))
                    .iter()
                    .any(|reply| matches!(reply, Reply::Bound { .. }))
            })
            .await;
        }

        // One inference across both, so both stages are holding state for it.
        let chain = chain_over(&[(&head, "h"), (&tail, "t")]);
        head.enqueue(to_node(
            &chain,
            &outer,
            "session",
            QueueClass::Prefill,
            ToNode::Execute {
                prompt: "긴 대화".into(),
                max_tokens: 4,
                options: "{}".into(),
            },
        ))
        .unwrap();
        until(|| {
            seen.replies("session")
                .iter()
                .any(|reply| matches!(reply, Reply::Done { .. }))
        })
        .await;

        // Persist it on each stage, addressed at that stage's own node.
        for (agent, node) in [(&head, "h"), (&tail, "t")] {
            cache_op(
                agent,
                &outer,
                &seen,
                node,
                &format!("persist-{node}"),
                ToNode::Persist {
                    sequence: "session".into(),
                },
            )
            .await;
            let replies = seen.replies(&format!("persist-{node}"));
            assert!(
                matches!(replies.first(), Some(Reply::Cached { .. })),
                "stage {node} persisted its own shard: {replies:?}"
            );
        }

        // And restore it on each, then continue the conversation across both.
        for (agent, node) in [(&head, "h"), (&tail, "t")] {
            cache_op(
                agent,
                &outer,
                &seen,
                node,
                &format!("restore-{node}"),
                ToNode::Restore {
                    sequence: "session".into(),
                },
            )
            .await;
            assert!(
                matches!(
                    seen.replies(&format!("restore-{node}")).first(),
                    Some(Reply::Cached { .. })
                ),
                "stage {node} brought its shard back"
            );
        }

        head.enqueue(to_node(
            &chain,
            &outer,
            "session",
            QueueClass::Prefill,
            ToNode::Execute {
                prompt: "이어서".into(),
                max_tokens: 3,
                options: "{}".into(),
            },
        ))
        .unwrap();
        until(|| {
            seen.replies("session")
                .iter()
                .filter(|reply| matches!(reply, Reply::Done { .. }))
                .count()
                >= 2
        })
        .await;
        assert!(
            seen.replies("session")
                .iter()
                .filter(|reply| matches!(reply, Reply::Done { .. }))
                .count()
                >= 2,
            "the resumed conversation ran across both stages again"
        );
    });
}

/// One stage restored and the other not.
///
/// The failure an operator will actually hit, because a restore is per stage
/// and any of them can fail. The claim is only that the stage that has nothing
/// says so — a chain that answered here would be answering from a head that
/// remembers the conversation and a tail that does not.
#[test]
fn a_stage_that_did_not_restore_refuses_rather_than_answering_from_half_a_history() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(&agent, &outer, &seen, "n0").await;

        infer(&agent, &outer, &seen, "n0", "half", 3).await;
        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "persist",
            ToNode::Persist {
                sequence: "half".into(),
            },
        )
        .await;

        // Restoring a different id is the shape of a stage that missed the
        // instruction: its own shard is still on disk and not in memory.
        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "wrong",
            ToNode::Restore {
                sequence: "some-other-session".into(),
            },
        )
        .await;
        assert!(
            matches!(seen.replies("wrong").first(), Some(Reply::Failed { .. })),
            "the stage said it has nothing for that conversation: {:?}",
            seen.replies("wrong")
        );

        // The real one is still on disk, untouched by the failed attempt.
        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "right",
            ToNode::Restore {
                sequence: "half".into(),
            },
        )
        .await;
        assert!(
            matches!(seen.replies("right").first(), Some(Reply::Cached { .. })),
            "and a failed restore did not disturb it: {:?}",
            seen.replies("right")
        );
    });
}

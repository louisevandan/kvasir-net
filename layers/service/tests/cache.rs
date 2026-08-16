//! Persisting, restoring and branching a request's cached state.
//!
//! An inference id already means something below the boundary: the backend
//! keeps that sequence's attention state against it. Every verb except
//! "continue it" was missing, and all three are ordinary for an agent that
//! holds a conversation open — it cannot keep hours of state resident, cannot
//! resume after the process serving it went away, and cannot explore two
//! continuations of the same history.
//!
//! What is asserted here is behaviour, not bookkeeping: state that was freed
//! comes back and the conversation continues from where it was, and a branch
//! is a copy rather than a second name for the same thing.

mod common;

use common::{Outer, backends, chain_over, runtime, start, to_agent, to_node, until};
use p4_protocol::QueueClass;
use p4_service::Standard;
use p4_service::message::{Reply, ToAgent, ToNode};
use std::sync::Arc;

type Agent = Arc<p4_agent_core::agent::Agent>;

async fn place(agent: &Agent, outer: &Agent, seen: &Outer, node: &str) {
    agent
        .enqueue(to_agent(
            agent,
            outer,
            "create",
            ToAgent::CreateNode {
                node: node.into(),
                adapter: "mock-tail".into(),
            },
        ))
        .unwrap();
    until(|| !seen.replies("create").is_empty()).await;

    let single = chain_over(&[(agent, node)]);
    agent
        .enqueue(to_node(
            &single,
            outer,
            "load",
            QueueClass::Control,
            ToNode::Load {
                plan: r#"{"layers":"0-19"}"#.into(),
                artifact: "model.gguf".into(),
                ceiling: 4,
            },
        ))
        .unwrap();
    until(|| {
        seen.replies("load")
            .iter()
            .any(|reply| matches!(reply, Reply::Bound { .. }))
    })
    .await;
}

/// Runs one inference to completion under `route`.
async fn infer(agent: &Agent, outer: &Agent, seen: &Outer, node: &str, route: &str, tokens: u32) {
    let chain = chain_over(&[(agent, node)]);
    agent
        .enqueue(to_node(
            &chain,
            outer,
            route,
            QueueClass::Prefill,
            ToNode::Execute {
                prompt: "대화".into(),
                max_tokens: tokens,
                options: "{}".into(),
            },
        ))
        .unwrap();
    until(|| {
        seen.replies(route)
            .iter()
            .any(|reply| matches!(reply, Reply::Done { .. }))
    })
    .await;
}

/// Sends one cache instruction and waits for its answer.
async fn cache_op(agent: &Agent, outer: &Agent, seen: &Outer, node: &str, route: &str, op: ToNode) {
    let chain = chain_over(&[(agent, node)]);
    agent
        .enqueue(to_node(&chain, outer, route, QueueClass::Control, op))
        .unwrap();
    until(|| !seen.replies(route).is_empty()).await;
}

/// The whole cycle: run, persist, restore, continue.
///
/// The claim is that the state survived being freed — a conversation that
/// resumed from nothing would look identical from outside except that it had
/// forgotten everything, which is why the token indices after the restore are
/// what is checked rather than the fact that a reply arrived.
#[test]
fn state_persisted_and_restored_continues_where_it_left_off() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(&agent, &outer, &seen, "n0").await;

        infer(&agent, &outer, &seen, "n0", "chat", 4).await;

        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "persist",
            ToNode::Persist {
                sequence: "chat".into(),
            },
        )
        .await;
        let persisted = seen.replies("persist");
        let Some(Reply::Cached {
            sequence, bytes, ..
        }) = persisted.first()
        else {
            panic!("persisting answered with a cache reply: {persisted:?}");
        };
        assert_eq!(sequence, "chat", "against the id it was asked about");
        assert!(*bytes > 0, "and said what the durable copy costs");

        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "restore",
            ToNode::Restore {
                sequence: "chat".into(),
            },
        )
        .await;
        let restored = seen.replies("restore");
        let Some(Reply::Cached {
            bytes: back,
            sequence: back_id,
            ..
        }) = restored.first()
        else {
            panic!("restoring answered: {restored:?}");
        };
        // The size proves it is the same state rather than a fresh empty one:
        // a conversation restored from nothing would answer just as happily
        // and have forgotten everything.
        assert_eq!(back, bytes, "the copy that came back is the one that went");
        assert_eq!(back_id, "chat");

        // Continuing under the same id picks up the progress it had, rather
        // than starting the conversation again.
        infer(&agent, &outer, &seen, "n0", "chat", 3).await;
        let all = seen.replies("chat");
        let indices: Vec<u32> = all
            .iter()
            .filter_map(|reply| match reply {
                Reply::Token { index, .. } => Some(*index),
                _ => None,
            })
            .collect();
        assert!(
            indices.windows(2).all(|pair| pair[1] > pair[0]),
            "the continuation carried on counting rather than restarting: {indices:?}"
        );
    });
}

/// A branch is a copy.
///
/// Two continuations of one history, each with its own state. Sharing would be
/// cheaper and wrong: the moment either continued it would corrupt the other,
/// and the failure would show up as one conversation's answer appearing in the
/// other's.
#[test]
fn a_fork_copies_the_state_under_a_new_id() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(&agent, &outer, &seen, "n0").await;

        infer(&agent, &outer, &seen, "n0", "history", 4).await;
        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "persist",
            ToNode::Persist {
                sequence: "history".into(),
            },
        )
        .await;

        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "fork",
            ToNode::Fork {
                sequence: "history".into(),
                into: "branch-a".into(),
            },
        )
        .await;
        let forked = seen.replies("fork");
        let Some(Reply::Cached {
            sequence, bytes, ..
        }) = forked.first()
        else {
            panic!("the fork answered: {forked:?}");
        };
        assert_eq!(
            sequence, "branch-a",
            "reported against the id the state now lives under, not the source"
        );
        assert!(*bytes > 0, "and the copy has a size");

        // The original is still there to fork again, which is the point of a
        // copy rather than a move.
        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "fork2",
            ToNode::Fork {
                sequence: "history".into(),
                into: "branch-b".into(),
            },
        )
        .await;
        assert!(
            matches!(seen.replies("fork2").first(), Some(Reply::Cached { .. })),
            "the source survived being branched: {:?}",
            seen.replies("fork2")
        );

        // Both branches restore independently.
        for branch in ["branch-a", "branch-b"] {
            cache_op(
                &agent,
                &outer,
                &seen,
                "n0",
                branch,
                ToNode::Restore {
                    sequence: branch.into(),
                },
            )
            .await;
            assert!(
                matches!(seen.replies(branch).first(), Some(Reply::Cached { .. })),
                "{branch} restored on its own: {:?}",
                seen.replies(branch)
            );
        }
    });
}

/// Deleting the durable copy, and what happens if you ask twice.
///
/// State nothing ever deletes is a disk filling up on a schedule nobody set,
/// so discard is part of the protocol rather than an operator's cron job. A
/// second discard is refused rather than treated as success, because a caller
/// that cannot tell "deleted it" from "there was nothing" cannot reconcile its
/// own records.
#[test]
fn a_discarded_copy_is_gone_and_saying_so_twice_is_refused() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(&agent, &outer, &seen, "n0").await;

        infer(&agent, &outer, &seen, "n0", "temp", 3).await;
        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "persist",
            ToNode::Persist {
                sequence: "temp".into(),
            },
        )
        .await;
        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "discard",
            ToNode::Discard {
                sequence: "temp".into(),
            },
        )
        .await;
        assert!(
            matches!(seen.replies("discard").first(), Some(Reply::Cached { .. })),
            "the copy was deleted: {:?}",
            seen.replies("discard")
        );

        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "again",
            ToNode::Discard {
                sequence: "temp".into(),
            },
        )
        .await;
        assert!(
            matches!(seen.replies("again").first(), Some(Reply::Failed { .. })),
            "and asking again says there was nothing: {:?}",
            seen.replies("again")
        );
    });
}

/// Asking for state that was never persisted.
///
/// Refused rather than answered with an empty cache, because a branch
/// continuing from a conversation that does not exist is worse than an error:
/// it produces confident output with no history behind it.
#[test]
fn restoring_or_forking_something_that_was_never_persisted_is_refused() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(&agent, &outer, &seen, "n0").await;

        for (route, op) in [
            (
                "restore",
                ToNode::Restore {
                    sequence: "never-ran".into(),
                },
            ),
            (
                "fork",
                ToNode::Fork {
                    sequence: "never-ran".into(),
                    into: "ghost".into(),
                },
            ),
        ] {
            cache_op(&agent, &outer, &seen, "n0", route, op).await;
            assert!(
                matches!(seen.replies(route).first(), Some(Reply::Failed { .. })),
                "{route} was refused: {:?}",
                seen.replies(route)
            );
        }
    });
}

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

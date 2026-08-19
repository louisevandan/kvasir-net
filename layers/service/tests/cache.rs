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

use common::conversation::{cache_op, infer, place};
use common::{Outer, backends, runtime, start};

use p4_service::Standard;
use p4_service::message::{Reply, ToNode};
use std::sync::Arc;

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
            deployment,
            stage_id,
            generation,
            operation_id,
            sequence,
            bytes,
            ..
        }) = persisted.first()
        else {
            panic!("persisting answered with a cache reply: {persisted:?}");
        };
        assert_eq!(sequence, "chat", "against the id it was asked about");
        assert_eq!(deployment, "deployment");
        assert_eq!(stage_id, "n0");
        assert_eq!(*generation, 1);
        assert_eq!(operation_id, "persist");
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
            matches!(
                seen.replies("again").first(),
                Some(Reply::CacheFailed { .. })
            ),
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
                matches!(seen.replies(route).first(), Some(Reply::CacheFailed { .. })),
                "{route} was refused: {:?}",
                seen.replies(route)
            );
        }
    });
}

use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

#[test]
fn a_reply_calls_the_handler_that_was_registered_for_it() {
    let seen = Arc::new(AtomicU32::new(0));
    let registry = Continuations::<u32>::default();
    let recorder = Arc::clone(&seen);
    registry.register(
        "route-1",
        Box::new(move |value| {
            recorder.store(value, Ordering::SeqCst);
        }),
    );

    assert!(registry.resolve("route-1", 42));
    assert_eq!(seen.load(Ordering::SeqCst), 42);
}

#[test]
fn a_handler_runs_once_and_is_gone() {
    let count = Arc::new(AtomicU32::new(0));
    let registry = Continuations::<()>::default();
    let counter = Arc::clone(&count);
    registry.register(
        "route-1",
        Box::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );

    assert!(registry.resolve("route-1", ()));
    // A second terminal on one route must not run the handler again. This is
    // the shape of the defect where a route could emit two terminals.
    assert!(!registry.resolve("route-1", ()));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn a_reply_nobody_asked_for_is_reported_rather_than_swallowed() {
    let registry = Continuations::<()>::default();
    assert!(!registry.resolve("never-registered", ()));
}

#[test]
fn forgetting_discards_the_handler_without_running_it() {
    // A cancelled request must not look like a completed one, so the handler
    // is dropped rather than called with something synthetic.
    let ran = Arc::new(AtomicU32::new(0));
    let registry = Continuations::<()>::default();
    let counter = Arc::clone(&ran);
    registry.register(
        "route-1",
        Box::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );

    assert!(registry.forget("route-1"));
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    assert!(!registry.resolve("route-1", ()));
}

#[test]
fn outstanding_counts_what_is_still_expected() {
    // A count that only grows is a route leak, which is one of the things the
    // fleet run watches.
    let registry = Continuations::<()>::default();
    assert_eq!(registry.outstanding(), 0);

    registry.register("a", Box::new(|_| {}));
    registry.register("b", Box::new(|_| {}));
    assert_eq!(registry.outstanding(), 2);

    registry.resolve("a", ());
    registry.forget("b");
    assert_eq!(registry.outstanding(), 0);
}

use super::*;
use crate::contract::{Accepted, Event, RejectReason, Rejected, Submit};
use crate::test_support::RecordingSink;
use crate::transport::fake::FakeFactory;
use p4_adapter::deployment::Client as DeploymentClientTrait;
use serde_json::json;
use std::thread;
use std::time::Duration;

fn connect(factory: &Arc<FakeFactory>) -> (Arc<DeploymentClient>, Arc<RecordingSink>) {
    connect_with_backoff(factory, Duration::from_millis(1))
}

fn connect_with_backoff(
    factory: &Arc<FakeFactory>,
    backoff: Duration,
) -> (Arc<DeploymentClient>, Arc<RecordingSink>) {
    let sink = RecordingSink::new();
    let client =
        DeploymentClient::connect(factory.clone(), sink.clone(), "dep-1".into(), 1, backoff)
            .expect("connect");
    (client, sink)
}

/// Composes a `Submit` the way a caller above this client (the P4 broker)
/// would -- stamping the client's own idea of the current generation, since
/// `try_submit` no longer fills that in itself -- and submits it.
fn submit(
    client: &DeploymentClient,
    submission_id: &str,
    request: &str,
) -> Result<(), EnqueueError> {
    client.try_submit(Submit {
        deployment_id: client.deployment_id().to_string(),
        deployment_generation: client.generation(),
        submission_id: submission_id.into(),
        deadline_unix_ms: 0,
        request: json!({ "body": request }),
    })
}

fn wait_for<F: Fn() -> bool>(condition: F) {
    for _ in 0..2000 {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("condition never became true within the test's budget");
}

#[test]
fn connecting_holds_one_connection_the_client_reuses_for_every_submission() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let (client, _sink) = connect(&factory);

    submit(&client, "s1", "req-1").expect("submit s1");
    submit(&client, "s2", "req-2").expect("submit s2");

    // Both submissions travelled over the one connection the factory handed
    // out at `connect()` -- a second `submit` never asked the factory for
    // another. `factory.connect()` was called exactly once (`remaining` was
    // seeded with only this one queued outcome and nothing else was ever
    // consumed).
    wait_for(|| handle.sent().len() == 2);
    client.close();
    handle.disconnect();
}

#[test]
fn backend_neutral_request_is_encoded_before_it_enters_the_llama_wire() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let (client, _sink) = connect(&factory);

    client
        .try_submit(Submit {
            deployment_id: client.deployment_id().to_string(),
            deployment_generation: client.generation(),
            submission_id: "neutral".into(),
            deadline_unix_ms: 0,
            request: json!({
                "prompt": "러스트를 설명하라",
                "max_tokens": 64,
                "options": r#"{"temperature":0.2}"#,
            }),
        })
        .expect("submit");
    wait_for(|| handle.sent().len() == 1);

    let crate::contract::Command::Submit(sent) = &handle.sent()[0] else {
        panic!("submit command");
    };
    assert_eq!(
        sent.request,
        json!({
            "messages": [{ "role": "user", "content": "러스트를 설명하라" }],
            "max_tokens": 64,
            "stream": true,
            "temperature": 0.2,
        })
    );
    client.close();
    handle.disconnect();
}

#[test]
fn two_submissions_are_in_flight_before_either_settles() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let (client, sink) = connect(&factory);

    submit(&client, "s1", "req-1").expect("submit s1");
    submit(&client, "s2", "req-2").expect("submit s2");
    handle.push_event(Event::Accepted(Accepted {
        submission_id: "s1".into(),
    }));
    handle.push_event(Event::Accepted(Accepted {
        submission_id: "s2".into(),
    }));
    // s1's own Produced/Settled has not arrived, and s2's Accepted still
    // reaches the sink -- the second submission was never blocked on the
    // first one settling.
    wait_for(|| sink.len() == 2);
    assert_eq!(
        sink.events(),
        vec![
            Event::Accepted(Accepted {
                submission_id: "s1".into(),
            }),
            Event::Accepted(Accepted {
                submission_id: "s2".into(),
            }),
        ]
    );
    client.close();
    handle.disconnect();
}

#[test]
fn full_is_retried_inside_the_deployment_client() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let (client, sink) = connect(&factory);

    submit(&client, "s1", "req-1").expect("submit s1");
    handle.push_event(Event::Rejected(Rejected {
        submission_id: "s1".into(),
        reason: RejectReason::Full,
    }));
    wait_for(|| handle.sent().len() == 2);
    assert_eq!(sink.len(), 0, "intermediate Full must not escape to P4");
    handle.push_event(Event::Accepted(Accepted {
        submission_id: "s1".into(),
    }));
    wait_for(|| sink.len() == 1);
    client.close();
    handle.disconnect();
}

#[test]
fn full_becomes_terminal_only_after_the_submission_deadline() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let (client, sink) = connect(&factory);
    client
        .try_submit(Submit {
            deployment_id: client.deployment_id().to_string(),
            deployment_generation: client.generation(),
            submission_id: "expired".into(),
            deadline_unix_ms: 1,
            request: json!({ "body": "request" }),
        })
        .expect("enqueue expired submission");
    wait_for(|| handle.sent().len() == 1);
    handle.push_event(Event::Rejected(Rejected {
        submission_id: "expired".into(),
        reason: RejectReason::Full,
    }));
    wait_for(|| sink.len() == 1);
    assert_eq!(
        sink.events(),
        vec![Event::Rejected(Rejected {
            submission_id: "expired".into(),
            reason: RejectReason::Full,
        })]
    );
    assert_eq!(handle.sent().len(), 1, "expired work is never retried");
    client.close();
    handle.disconnect();
}

#[test]
fn duplicate_submit_is_not_sent_twice() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let (client, _sink) = connect(&factory);

    submit(&client, "s1", "req-1").expect("first submit");
    wait_for(|| handle.sent().len() == 1);
    // Not an admission verdict: try_submit reports only whether the pump
    // could enqueue at all, so a duplicate is `Ok(())` exactly like the
    // first send -- the proof that nothing was sent twice is `handle.sent()`
    // below, not this return value.
    submit(&client, "s1", "a different payload").expect("second submit");
    // A marker submission proves the pump has already worked through the
    // duplicate by the time it lands -- the pump processes its queue in
    // order, so s2 landing means s1's duplicate was already decided.
    submit(&client, "s2", "req-2").expect("marker submit");
    wait_for(|| handle.sent().len() == 2);
    assert_eq!(handle.sent().len(), 2);
    client.close();
    handle.disconnect();
}

#[test]
fn event_from_a_superseded_generation_never_reaches_the_sink() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let (client, sink) = connect(&factory);

    submit(&client, "s1", "req-1").expect("submit s1");
    client.advance_generation(2);
    handle.push_event(Event::Accepted(Accepted {
        submission_id: "s1".into(),
    }));
    // A second, current-generation submission proves the reader loop is
    // still alive and processing -- if the stale event had been silently
    // dropped by a dead thread rather than deliberately refused, this would
    // also never arrive.
    submit(&client, "s2", "req-2").expect("submit s2");
    handle.push_event(Event::Accepted(Accepted {
        submission_id: "s2".into(),
    }));
    wait_for(|| sink.len() == 2);
    assert_eq!(
        sink.events(),
        vec![
            Event::Rejected(Rejected {
                submission_id: "s1".into(),
                reason: RejectReason::DeploymentClosed,
            }),
            Event::Accepted(Accepted {
                submission_id: "s2".into(),
            }),
        ]
    );
    client.close();
    handle.disconnect();
}

// Reconnect/replay behaviour (including the mid-reconnect exactly-once
// proof) lives in `reconnect_tests.rs`, split out to stay under this
// repository's per-file line limit.

#[test]
fn closing_stops_the_background_threads() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let (client, _sink) = connect(&factory);
    client.close();
    handle.disconnect();
    client.join_reader_for_test();
}

#[test]
fn a_closed_client_refuses_to_enqueue_further_work() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let (client, _sink) = connect(&factory);
    client.close();
    handle.disconnect();
    assert!(submit(&client, "s1", "req-1").is_err());
    // Cancel is infallible by contract -- a closed client silently no-ops
    // rather than erroring, matching `p4_adapter::deployment::Client::cancel`.
    client.cancel("s1".into());
}

/// Proof obligation from the task: `try_submit` must return promptly even
/// when the socket is not draining, because it never touches the socket at
/// all -- it only ever reaches the pump's bounded queue. The writer here is
/// gated to block forever until the test releases it, simulating TCP
/// backpressure; `try_submit` still returns well inside a budget no blocked
/// write could meet.
#[test]
fn try_submit_returns_promptly_when_the_socket_is_not_draining() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let (client, _sink) = connect(&factory);

    // Let the pump pick up the connection and go idle before gating writes,
    // so the block genuinely lands on a submission made while stuck, not
    // during connection setup.
    submit(&client, "warmup", "req").expect("warmup submit");
    wait_for(|| handle.sent().len() == 1);

    handle.block_writes();

    for index in 0..8 {
        let started = std::time::Instant::now();
        submit(&client, &format!("blocked-{index}"), "req")
            .expect("try_submit must not fail just because the writer is stuck");
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(500),
            "try_submit blocked for {elapsed:?} while the socket was not draining"
        );
    }

    // The pump is still stuck trying to flush the very first blocked write,
    // so nothing past "warmup" has reached the wire yet.
    assert_eq!(handle.sent().len(), 1);

    handle.unblock_writes();
    wait_for(|| handle.sent().len() == 9);

    client.close();
    handle.disconnect();
}

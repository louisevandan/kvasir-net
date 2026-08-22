//! Reconnect/replay behaviour, split out of `tests.rs` to keep both files
//! under this repository's per-file line limit. Everything here concerns
//! what happens to a submission across a lost and re-established
//! connection; `tests.rs` covers the client's steady-state behaviour.

use super::*;
use crate::contract::{Accepted, Command, Event, Produced, SettleReason, Settled, Submit};
use crate::test_support::RecordingSink;
use crate::transport::fake::FakeFactory;
use p4_adapter::deployment::Client as DeploymentClientTrait;
use serde_json::json;
use std::thread;
use std::time::Duration;

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

fn connect(factory: &Arc<FakeFactory>) -> (Arc<DeploymentClient>, Arc<RecordingSink>) {
    connect_with_backoff(factory, Duration::from_millis(1))
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
fn reconnect_replays_an_in_flight_submission_exactly_once() {
    let factory = FakeFactory::new();
    let handle1 = factory.queue_success();
    let handle2 = factory.queue_success();
    let (client, sink) = connect(&factory);

    submit(&client, "s1", "req-1").expect("submit s1");
    wait_for(|| handle1.sent().len() == 1);

    // Socket loss, not a clean close -- proving the client does not treat
    // this the same as a `Rejected { Full }` from the backend.
    handle1.fail_with("connection reset by peer");

    wait_for(|| !handle2.sent().is_empty());
    assert_eq!(handle2.sent().len(), 1);
    match &handle2.sent()[0] {
        Command::Submit(submit) => assert_eq!(submit.submission_id, "s1"),
        other => panic!("expected a replayed Submit, got {other:?}"),
    }
    assert_eq!(client.reconnect_count(), 1);

    // The backend now answers on the new connection. Nothing about the
    // resend fabricated a second execution client-side: a caller trying to
    // submit "s1" again still finds it already known, and nothing new
    // reaches the wire for it.
    submit(&client, "s1", "req-1").expect("resubmit s1");
    // A marker on the new connection proves the pump processed the resubmit
    // attempt (in order, after the replay) without adding a second send.
    submit(&client, "s2", "req-2").expect("marker submit");
    wait_for(|| handle2.sent().len() == 2);
    assert_eq!(handle2.sent().len(), 2);

    handle2.push_event(Event::Accepted(Accepted {
        submission_id: "s1".into(),
    }));
    handle2.push_event(Event::Settled(Settled {
        submission_id: "s1".into(),
        reason: SettleReason::Stop,
        generated_tokens: 4,
    }));
    wait_for(|| sink.len() == 2);
    assert_eq!(
        sink.events(),
        vec![
            Event::Accepted(Accepted {
                submission_id: "s1".into(),
            }),
            Event::Settled(Settled {
                submission_id: "s1".into(),
                reason: SettleReason::Stop,
                generated_tokens: 4,
            }),
        ]
    );
    client.close();
    handle2.disconnect();
}

#[test]
fn reconnect_suppresses_a_replayed_prefix_and_continues_the_response() {
    let factory = FakeFactory::new();
    let handle1 = factory.queue_success();
    let handle2 = factory.queue_success();
    let (client, sink) = connect(&factory);

    submit(&client, "s1", "req-1").expect("submit s1");
    handle1.push_event(Event::Accepted(Accepted {
        submission_id: "s1".into(),
    }));
    handle1.push_event(Event::Produced(Produced {
        submission_id: "s1".into(),
        event_ordinal: 0,
        text: "He".into(),
        generated_tokens: 1,
    }));
    handle1.push_event(Event::Produced(Produced {
        submission_id: "s1".into(),
        event_ordinal: 1,
        text: "llo".into(),
        generated_tokens: 2,
    }));
    wait_for(|| sink.len() == 3);

    handle1.fail_with("connection reset after two tokens");
    wait_for(|| client.reconnect_count() == 1 && handle2.sent().len() == 1);

    // The server journal has no client resume ordinal, so it replays its
    // retained prefix before producing new work on the replacement socket.
    handle2.push_event(Event::Accepted(Accepted {
        submission_id: "s1".into(),
    }));
    for (event_ordinal, text, generated_tokens) in [(0, "He", 1), (1, "llo", 2)] {
        handle2.push_event(Event::Produced(Produced {
            submission_id: "s1".into(),
            event_ordinal,
            text: text.into(),
            generated_tokens,
        }));
    }
    handle2.push_event(Event::Produced(Produced {
        submission_id: "s1".into(),
        event_ordinal: 2,
        text: " world".into(),
        generated_tokens: 3,
    }));
    handle2.push_event(Event::Settled(Settled {
        submission_id: "s1".into(),
        reason: SettleReason::Stop,
        generated_tokens: 3,
    }));

    wait_for(|| sink.len() == 5);
    let events = sink.events();
    let body: String = events
        .iter()
        .filter_map(|event| match event {
            Event::Produced(produced) => Some(produced.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(body, "Hello world");
    assert!(matches!(
        events.last(),
        Some(Event::Settled(Settled {
            reason: SettleReason::Stop,
            generated_tokens: 3,
            ..
        }))
    ));

    client.close();
    handle2.disconnect();
}

#[test]
fn reconnect_does_not_replay_a_submission_that_already_settled() {
    let factory = FakeFactory::new();
    let handle1 = factory.queue_success();
    let handle2 = factory.queue_success();
    let (client, sink) = connect(&factory);

    submit(&client, "s1", "req-1").expect("submit s1");
    handle1.push_event(Event::Accepted(Accepted {
        submission_id: "s1".into(),
    }));
    handle1.push_event(Event::Settled(Settled {
        submission_id: "s1".into(),
        reason: SettleReason::Stop,
        generated_tokens: 2,
    }));
    wait_for(|| sink.len() == 2);

    handle1.disconnect();
    wait_for(|| client.reconnect_count() == 1);
    // s1 had already settled before the disconnect, so the replay set was
    // empty for it: nothing about a finished submission gets resent. A
    // marker submission proves the pump has finished acting on the
    // reconnect (past any replay it was going to do) before this asserts.
    submit(&client, "s2", "req-2").expect("marker submit");
    wait_for(|| !handle2.sent().is_empty());
    assert_eq!(handle2.sent().len(), 1);
    match &handle2.sent()[0] {
        Command::Submit(submit) => assert_eq!(submit.submission_id, "s2"),
        other => panic!("expected only the marker Submit, got {other:?}"),
    }

    client.close();
    handle2.disconnect();
}

#[test]
fn socket_loss_never_manufactures_a_full_rejection_or_a_settle() {
    let factory = FakeFactory::new();
    let handle1 = factory.queue_success();
    let handle2 = factory.queue_success();
    let (client, sink) = connect(&factory);

    submit(&client, "s1", "req-1").expect("submit s1");
    handle1.fail_with("broken pipe");
    wait_for(|| client.reconnect_count() == 1);
    // The client reconnected and replayed, but produced no event of its own
    // making about s1 in the process -- only what the backend itself later
    // says (nothing, in this test) reaches the sink.
    assert_eq!(sink.len(), 0);

    client.close();
    handle2.disconnect();
}

#[test]
fn reconnect_to_a_new_generation_terminalizes_old_work_before_new_admission() {
    let factory = FakeFactory::new();
    let first = factory.queue_success_reporting(1);
    let second = factory.queue_success_reporting(2);
    let (client, sink) = connect(&factory);

    submit(&client, "old", "request").expect("submit old generation");
    first.push_event(Event::Accepted(Accepted {
        submission_id: "old".into(),
    }));
    first.push_event(Event::Produced(Produced {
        submission_id: "old".into(),
        event_ordinal: 0,
        text: "partial".into(),
        generated_tokens: 1,
    }));
    wait_for(|| sink.len() == 2);
    first.disconnect();

    wait_for(|| client.generation() == 2 && sink.len() == 3);
    assert!(matches!(
        sink.events().last(),
        Some(Event::Settled(Settled {
            submission_id,
            reason: SettleReason::Error,
            generated_tokens: 1,
        })) if submission_id == "old"
    ));
    assert!(
        second.sent().iter().all(|command| {
            !matches!(command, Command::Submit(submit) if submit.submission_id == "old")
        }),
        "old-generation work must not be replayed"
    );

    submit(&client, "new", "request").expect("submit new generation");
    wait_for(|| {
        second.sent().iter().any(|command| {
        matches!(command, Command::Submit(submit) if submit.submission_id == "new" && submit.deployment_generation == 2)
    })
    });
    client.close();
    second.disconnect();
}

/// Proof obligation from the task: a submission whose enqueue attempt lands
/// while the pump is mid-reconnect must still be delivered exactly once,
/// never lost. Before the pump owned reconnect and replay together, a
/// submission arriving between the old code's ledger snapshot and its
/// writer install had nowhere to go and vanished; here the submission just
/// waits its turn in the same queue reconnect itself was dispatched
/// through, so there is no window where it is recorded but unreachable.
#[test]
fn a_submission_arriving_during_reconnect_is_delivered_exactly_once() {
    let factory = FakeFactory::new();
    let handle1 = factory.queue_success();
    // The pump's first reconnect attempt fails and must sleep for the
    // backoff before trying again -- that sleep is the window this test
    // submits into.
    factory.queue_failure("connect refused");
    let handle2 = factory.queue_success();
    let (client, sink) = connect_with_backoff(&factory, Duration::from_millis(150));

    submit(&client, "s1", "req-1").expect("submit s1");
    wait_for(|| handle1.sent().len() == 1);

    // Kill the connection: the pump's reader notices, enters `reconnect()`,
    // fails its first attempt (the queued failure above), and is now
    // sleeping for 150ms before retrying.
    handle1.fail_with("connection reset by peer");

    // Racing into that sleep window: this submission was never part of the
    // ledger before the disconnect, so it is not a replay -- it is new work
    // arriving genuinely mid-reconnect.
    thread::sleep(Duration::from_millis(20));
    submit(&client, "s-race", "req-race").expect("submit during reconnect");

    wait_for(|| handle2.sent().len() >= 2);
    // Give any duplicate-delivery bug a fair chance to show up before
    // asserting the final count.
    thread::sleep(Duration::from_millis(100));

    let sent = handle2.sent();
    let race_sends = sent
        .iter()
        .filter(|command| matches!(command, Command::Submit(submit) if submit.submission_id == "s-race"))
        .count();
    assert_eq!(
        race_sends, 1,
        "s-race must reach the wire exactly once, got {sent:?}"
    );
    let s1_sends = sent
        .iter()
        .filter(
            |command| matches!(command, Command::Submit(submit) if submit.submission_id == "s1"),
        )
        .count();
    assert_eq!(s1_sends, 1, "s1's replay must also land exactly once");

    handle2.push_event(Event::Accepted(Accepted {
        submission_id: "s-race".into(),
    }));
    wait_for(|| sink.len() == 1);

    client.close();
    handle2.disconnect();
}

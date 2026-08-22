//! Reconnect/replay behaviour, split out of `tests.rs` to keep both files
//! under this repository's per-file line limit. Everything here concerns
//! what happens to a submission across a lost and re-established
//! connection; `tests.rs` covers the client's steady-state behaviour.

use super::*;
use crate::contract::{
    Accepted, Command, Event, RejectReason, Rejected, SettleReason, Settled, Submit,
};
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

/// A cancel that loses its socket write is still owed to the backend.
///
/// The caller has already returned by then, so nothing above this client
/// can retry it. Before the ledger remembered the intent, the reconnect
/// replayed only the submission and the request went on running against a
/// caller who had asked it to stop.
#[test]
fn a_cancel_whose_write_fails_is_resent_after_the_reconnect() {
    let factory = FakeFactory::new();
    let first = factory.queue_success();
    let second = factory.queue_success();
    let (client, _sink) = connect(&factory);

    submit(&client, "s-cancel", "body").expect("submit is accepted");
    wait_for(|| {
        first
            .sent()
            .iter()
            .any(|command| matches!(command, Command::Submit(_)))
    });

    // The link breaks in the one way the caller cannot see: the cancel's
    // own write is what fails.
    first.fail_writes();
    client.cancel("s-cancel".into());

    wait_for(|| {
        second
            .sent()
            .iter()
            .filter(|command| matches!(command, Command::Cancel(_)))
            .count()
            == 1
    });

    let resent = second.sent();
    assert_eq!(
        resent
            .iter()
            .filter(|command| matches!(command, Command::Cancel(_)))
            .count(),
        1,
        "the owed cancel is resent exactly once, not dropped and not duplicated"
    );
}

/// The submission bound is a real bound under concurrent callers.
///
/// A load followed by a separate increment lets several threads all read
/// one below the limit and all pass, so the queue overshoots.
#[test]
fn the_submission_bound_holds_when_callers_race() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    handle.block_writes();
    let (client, _sink) = connect(&factory);

    let accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut threads = Vec::new();
    for worker in 0..8 {
        let client = Arc::clone(&client);
        let accepted = Arc::clone(&accepted);
        threads.push(thread::spawn(move || {
            for index in 0..200 {
                let id = format!("s-{worker}-{index}");
                if submit(&client, &id, "body").is_ok() {
                    accepted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }));
    }
    for thread in threads {
        thread.join().expect("worker joins");
    }

    // One more than the bound is legitimate and not a race: the pump pops a
    // submission -- decrementing the count -- and then blocks on its write,
    // which frees exactly one slot. Anything beyond that is two callers
    // having read the same value and both passed.
    let total = accepted.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        total <= super::pump::COMMAND_QUEUE_BOUND + 1,
        "accepted {total} submissions against a bound of {}",
        super::pump::COMMAND_QUEUE_BOUND
    );
}

/// `Full` means "later", so the id it names has to be free again.
///
/// The backend refuses a `Full` submission before recording it, so nothing
/// started and the caller's retry is the first real attempt. While the
/// ledger kept the entry, that retry was `AlreadyKnown`: nothing reached the
/// wire, no further event ever arrived, and the request hung forever on a
/// rejection that was only backpressure.
#[test]
fn a_full_rejection_frees_the_id_so_a_resend_reaches_the_wire() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let (client, sink) = connect(&factory);

    submit(&client, "s-full", "body").expect("first submit is enqueued");
    wait_for(|| handle.sent().len() == 1);

    handle.push_event(Event::Rejected(Rejected {
        submission_id: "s-full".into(),
        reason: RejectReason::Full,
    }));
    // The rejection still reaches the caller -- that is what tells it to
    // retry at all.
    wait_for(|| sink.len() == 1);

    submit(&client, "s-full", "body").expect("the retry is enqueued");
    wait_for(|| handle.sent().len() == 2);

    let sent = handle.sent();
    assert_eq!(
        sent.iter()
            .filter(|command| matches!(command, Command::Submit(submit) if submit.submission_id == "s-full"))
            .count(),
        2,
        "the retry after Full must reach the wire, got {sent:?}"
    );

    // And the freed id behaves like a live submission again: an event about
    // it is admitted rather than refused as belonging to nothing.
    handle.push_event(Event::Accepted(Accepted {
        submission_id: "s-full".into(),
    }));
    wait_for(|| sink.len() == 2);

    client.close();
    handle.disconnect();
}

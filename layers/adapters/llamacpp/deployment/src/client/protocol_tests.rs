use super::*;
use crate::contract::{
    Accepted, Event, Produced, RejectReason, Rejected, SettleReason, Settled, Submit,
};
use crate::test_support::RecordingSink;
use crate::transport::fake::FakeFactory;
use p4_adapter::deployment::Client as DeploymentClientTrait;
use serde_json::json;
use std::thread;
use std::time::Duration;

fn connect() -> (
    Arc<FakeFactory>,
    crate::transport::fake::FakeLinkHandle,
    Arc<DeploymentClient>,
    Arc<RecordingSink>,
) {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let sink = RecordingSink::new();
    let client = DeploymentClient::connect(
        factory.clone(),
        sink.clone(),
        "dep-1".into(),
        1,
        Duration::from_millis(1),
    )
    .expect("connect");
    (factory, handle, client, sink)
}

fn submit(client: &DeploymentClient, submission_id: &str) {
    client
        .try_submit(Submit {
            deployment_id: client.deployment_id().to_owned(),
            deployment_generation: client.generation(),
            submission_id: submission_id.to_owned(),
            deadline_unix_ms: 0,
            request: json!({
                "prompt": "request",
                "max_tokens": 4,
                "options": "{}",
            }),
        })
        .expect("submit");
}

fn wait_for(condition: impl Fn() -> bool) {
    for _ in 0..2_000 {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("condition never became true within the test budget");
}

#[test]
fn resubmitting_a_tombstone_raises_its_exact_terminal_again() {
    let (_factory, handle, client, sink) = connect();
    submit(&client, "done");
    handle.push_event(Event::Accepted(Accepted {
        submission_id: "done".into(),
    }));
    let terminal = Event::Settled(Settled {
        submission_id: "done".into(),
        reason: SettleReason::Length,
        generated_tokens: 9,
    });
    handle.push_event(terminal.clone());
    wait_for(|| sink.len() == 2);

    submit(&client, "done");
    wait_for(|| sink.len() == 3);
    assert_eq!(sink.events()[2], terminal);
    assert_eq!(handle.sent().len(), 1, "a tombstone never executes twice");
    client.close();
    handle.disconnect();
}

#[test]
fn an_ordinal_gap_is_not_a_success_looking_truncated_response() {
    let (_factory, handle, client, sink) = connect();
    submit(&client, "gap");
    handle.push_event(Event::Accepted(Accepted {
        submission_id: "gap".into(),
    }));
    handle.push_event(Event::Produced(Produced {
        submission_id: "gap".into(),
        event_ordinal: 0,
        text: "first".into(),
        generated_tokens: 1,
    }));
    handle.push_event(Event::Produced(Produced {
        submission_id: "gap".into(),
        event_ordinal: 2,
        text: "missing one".into(),
        generated_tokens: 3,
    }));

    wait_for(|| sink.len() == 3);
    assert_eq!(
        sink.events()[2],
        Event::Settled(Settled {
            submission_id: "gap".into(),
            reason: SettleReason::Error,
            generated_tokens: 1,
        })
    );
    client.close();
    handle.disconnect();
}

#[test]
fn full_after_accepted_fails_closed_instead_of_entering_a_silent_retry() {
    let (_factory, handle, client, sink) = connect();
    submit(&client, "bad-full");
    handle.push_event(Event::Accepted(Accepted {
        submission_id: "bad-full".into(),
    }));
    handle.push_event(Event::Rejected(Rejected {
        submission_id: "bad-full".into(),
        reason: RejectReason::Full,
    }));

    wait_for(|| sink.len() == 2);
    assert_eq!(
        sink.events()[1],
        Event::Settled(Settled {
            submission_id: "bad-full".into(),
            reason: SettleReason::Error,
            generated_tokens: 0,
        })
    );
    assert_eq!(handle.sent().len(), 1, "invalid Full is never retried");
    client.close();
    handle.disconnect();
}

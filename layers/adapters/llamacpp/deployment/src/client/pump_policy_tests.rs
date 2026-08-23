//! Pump capacity, cancel durability, and adapter-local retry policy.

use super::*;
use crate::contract::{Accepted, Command, Event, RejectReason, Rejected, Submit};
use crate::test_support::RecordingSink;
use crate::transport::fake::FakeFactory;
use p4_adapter::deployment::Client as DeploymentClientTrait;
use serde_json::json;
use std::thread;
use std::time::Duration;

fn connect(factory: &Arc<FakeFactory>) -> (Arc<DeploymentClient>, Arc<RecordingSink>) {
    let sink = RecordingSink::new();
    let client = DeploymentClient::connect(
        factory.clone(),
        sink.clone(),
        "dep-1".into(),
        1,
        Duration::from_millis(1),
    )
    .expect("connect");
    (client, sink)
}

fn submit(client: &DeploymentClient, id: &str) -> Result<(), EnqueueError> {
    client.try_submit(Submit {
        deployment_id: client.deployment_id().to_owned(),
        deployment_generation: client.generation(),
        submission_id: id.to_owned(),
        deadline_unix_ms: 0,
        request: json!({
            "prompt": "request",
            "max_tokens": 4,
            "options": "{}",
        }),
    })
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
fn a_cancel_whose_write_fails_is_resent_after_the_reconnect() {
    let factory = FakeFactory::new();
    let first = factory.queue_success();
    let second = factory.queue_success();
    let (client, _sink) = connect(&factory);
    submit(&client, "s-cancel").expect("submit");
    wait_for(|| {
        first
            .sent()
            .iter()
            .any(|item| matches!(item, Command::Submit(_)))
    });

    first.fail_writes();
    client.cancel("s-cancel".into());
    wait_for(|| {
        second
            .sent()
            .iter()
            .filter(|item| matches!(item, Command::Cancel(_)))
            .count()
            == 1
    });
}

#[test]
fn the_submission_bound_holds_when_callers_race() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    handle.block_writes();
    let (client, _sink) = connect(&factory);
    let accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut workers = Vec::new();
    for worker in 0..8 {
        let client = Arc::clone(&client);
        let accepted = Arc::clone(&accepted);
        workers.push(thread::spawn(move || {
            for index in 0..200 {
                if submit(&client, &format!("s-{worker}-{index}")).is_ok() {
                    accepted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }));
    }
    for worker in workers {
        worker.join().expect("worker joins");
    }
    let accepted = accepted.load(std::sync::atomic::Ordering::SeqCst);
    assert!(accepted <= super::pump::COMMAND_QUEUE_BOUND);
    let stats = client.stats();
    assert_eq!(stats.outstanding_submissions, accepted);
    assert_eq!(stats.peak_outstanding_submissions, accepted);
    assert!(stats.peak_queued_submissions > 0);
    assert!(stats.queued_submissions <= stats.peak_queued_submissions);
}

#[test]
fn full_is_retried_without_a_p4_resubmit() {
    let factory = FakeFactory::new();
    let handle = factory.queue_success();
    let (client, sink) = connect(&factory);
    submit(&client, "s-full").expect("submit");
    wait_for(|| handle.sent().len() == 1);
    handle.push_event(Event::Rejected(Rejected {
        submission_id: "s-full".into(),
        reason: RejectReason::Full,
    }));
    wait_for(|| handle.sent().len() == 2);
    assert_eq!(sink.len(), 0, "intermediate Full must stay in the adapter");
    handle.push_event(Event::Accepted(Accepted {
        submission_id: "s-full".into(),
    }));
    wait_for(|| sink.len() == 1);
    client.close();
    handle.disconnect();
}

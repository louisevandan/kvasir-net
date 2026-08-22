//! Two properties the relay's happy-path tests cannot see: that a cancel
//! reaches the deployment client that is actually running the work, and that
//! a terminal which cannot be delivered right now is not thrown away.
//!
//! Both were real defects. The ingress cancel walked only the node queues,
//! so a request already diverted here was answered "already terminal" while
//! it kept generating; and the terminal dropped its reply route before
//! enqueueing, so a full output queue destroyed the one piece of state a
//! resend could have been rebuilt from.

use super::*;
use p4_adapter::deployment::Rejected as DeploymentRejected;
use p4_adapter::deployment::RejectedReason as DeploymentRejectedReason;
use p4_adapter::deployment::Settled as DeploymentSettled;
use p4_adapter::deployment::SettledReason as DeploymentSettledReason;

/// Records the `submission_id` of every cancel it is asked to forward, and
/// answers nothing on its own -- these tests are about what reaches a client,
/// not about what comes back.
struct CancelRecordingClient {
    cancelled: Arc<StdMutex<Vec<String>>>,
}

impl Client for CancelRecordingClient {
    fn try_submit(&self, _submit: Submit) -> Result<(), EnqueueError> {
        Ok(())
    }

    fn cancel(&self, submission_id: SubmissionId) {
        self.cancelled.lock().unwrap().push(submission_id);
    }
}

/// A cancel for relayed work has to reach the client running it.
///
/// The client's own cancel delivery is lossless across a reconnect, but that
/// is worth nothing while nothing calls it: the ingress scanned node queues,
/// found no carrier for a submission that was never in one, and reported the
/// request already terminal while the backend went on generating.
#[test]
fn cancelling_relayed_work_reaches_the_deployment_client() {
    runtime().block_on(async {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let agent = agent_with(Arc::clone(&seen));
        let cancelled = Arc::new(StdMutex::new(Vec::new()));
        agent.deployments().register(
            "dep-1".into(),
            Arc::new(CancelRecordingClient {
                cancelled: Arc::clone(&cancelled),
            }),
        );

        let frame = submission_frame(&agent, "dep-1", 1);
        let submit = SubmissionPayload.submission(&frame).expect("a submission");
        agent.relay_submit(frame, submit);
        assert_eq!(agent.submission_route_count(), 1);

        // The route a cancel names is the carrier's own `envelope.route`,
        // which is not the `submission_id` the relay keys its table by --
        // the reason this path has to search rather than look up.
        let carrier = agent.cancel_frame("route-1").await;

        assert!(
            carrier.is_some(),
            "the carrier must come back so the original request can be terminalized"
        );
        assert_eq!(
            cancelled.lock().unwrap().as_slice(),
            &["route-1".to_string()],
            "the cancel must reach the client that is running the submission"
        );
        assert_eq!(
            agent.submission_route_count(),
            0,
            "a cancelled submission leaves no route behind"
        );
    });
}

/// A terminal that cannot be delivered keeps its route.
///
/// The relay used to remove the route and then enqueue, so a saturated
/// output lane lost both the terminal and every trace of which request it
/// belonged to. Keeping the route is what makes the loss recoverable: the
/// backend's next word about this submission still has somewhere to land.
#[test]
fn a_terminal_that_cannot_be_enqueued_keeps_its_route() {
    runtime().block_on(async {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        // One frame per lane and a one-deep emergency channel, with no
        // workers draining either: the smallest arrangement in which an
        // enqueue genuinely has nowhere to go.
        let (agent, _receiver, _in_flight) = Agent::new(
            Address::tcp("127.0.0.1", 0),
            Arc::new(Collect(seen)),
            Arc::new(SubmissionPayload),
            // Lane capacity comes from `Lanes`, not from the budget -- the
            // budget's `depth` only sizes the emergency channel behind them.
            Lanes {
                control: 1,
                prefill: 1,
                decode: 1,
                response: 1,
            },
            Budget {
                connections: 1,
                in_flight: 1,
                depth: 1,
            },
        );
        let relay_sink: Arc<dyn DeploymentSink> =
            Arc::new(AgentDeploymentSink::new(Arc::downgrade(&agent)));
        agent.deployments().register(
            "dep-1".into(),
            Arc::new(CancelRecordingClient {
                cancelled: Arc::new(StdMutex::new(Vec::new())),
            }),
        );

        let frame = submission_frame(&agent, "dep-1", 1);
        let submit = SubmissionPayload.submission(&frame).expect("a submission");
        agent.relay_submit(frame.clone(), submit);
        assert_eq!(agent.submission_route_count(), 1);

        // Fill every lane. Which lane a reply takes is `to_reply`'s business,
        // not this test's to assume -- filling only the one a reply was
        // assumed to use left the real one empty and the terminal sailed
        // straight through a queue this test believed was saturated.
        for lane in [
            QueueClass::Control,
            QueueClass::Prefill,
            QueueClass::Decode,
            QueueClass::Response,
        ] {
            for _ in 0..8 {
                let mut filler = frame.clone();
                filler.envelope.lane = lane;
                let _ = agent.enqueue(filler);
            }
        }
        // Then close the emergency channel behind them. Its drain task keeps
        // re-offering the one frame it holds into the same full lane, so it
        // stops taking new ones only once it is stuck there -- which takes a
        // moment to settle, and is why this fills, waits, and fills again
        // rather than filling once.
        for _ in 0..16 {
            let _ = agent.emergency.try_send(frame.clone());
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        while agent.emergency.try_send(frame.clone()).is_ok() {}
        // Stated as a precondition rather than assumed: a test that quietly
        // failed to saturate would pass for the wrong reason, which is
        // exactly what happened while it filled only one lane of four.
        assert!(
            agent.emergency.try_send(frame.clone()).is_err(),
            "this test proves nothing unless both lanes really are refusing"
        );

        relay_sink.raise(DeploymentEvent::Settled(DeploymentSettled {
            submission_id: "route-1".into(),
            reason: DeploymentSettledReason::Stop,
            generated_tokens: 1,
        }));

        assert_eq!(
            agent.submission_route_count(),
            1,
            "an undeliverable terminal must not take the route down with it"
        );
    });
}

/// A client emits Full only after its adapter-local retry policy is done.
/// P4 treats that final typed refusal exactly like every other rejection.
#[test]
fn a_final_full_rejection_removes_the_route() {
    runtime().block_on(async {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let agent = agent_with(Arc::clone(&seen));
        agent.deployments().register(
            "dep-1".into(),
            Arc::new(CancelRecordingClient {
                cancelled: Arc::new(StdMutex::new(Vec::new())),
            }),
        );
        let relay_sink: Arc<dyn DeploymentSink> =
            Arc::new(AgentDeploymentSink::new(Arc::downgrade(&agent)));

        let mut frame = submission_frame(&agent, "dep-1", 1);
        frame.envelope.deadline_unix_ms = 1;
        let submit = SubmissionPayload.submission(&frame).expect("a submission");
        agent.relay_submit(frame, submit);
        assert_eq!(agent.submission_route_count(), 1);

        relay_sink.raise(DeploymentEvent::Rejected(DeploymentRejected {
            submission_id: "route-1".into(),
            reason: DeploymentRejectedReason::Full,
        }));

        assert_eq!(
            agent.submission_route_count(),
            0,
            "a final refusal must be answered and dropped, not retried by P4"
        );
    });
}

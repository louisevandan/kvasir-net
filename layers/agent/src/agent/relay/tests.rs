use super::*;
use crate::agent::{Adapter, Agent, Duties, run};
use crate::node::payload::Payload;
use crate::queue::lane::{Budget, Lanes};
use p4_adapter::deployment::{
    Accepted, Client, EnqueueError, RejectedReason, SubmissionId, Submit,
};
use p4_adapter::{Distribution, Event, EventSink, Outcome, Sequence, Work};
use p4_protocol::{Address, QueueClass};
use p4_protocol::{Chain, Envelope, Link, Recipient};
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

/// Answers every hop immediately with one token and a terminal -- the
/// hop-path stand-in these tests use to prove the relay's gate leaves it
/// alone when no deployment client is registered.
struct InstantAdapter;

impl Adapter for InstantAdapter {
    fn distribution(&self) -> Distribution {
        Distribution::Staged
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        let Work::Hop(hop) = work else { return };
        events.raise(Event::HopComplete {
            hop_id: hop.id,
            expected: hop
                .sequences
                .iter()
                .map(|sequence| sequence.sequence.clone())
                .collect(),
            outcomes: hop
                .sequences
                .iter()
                .map(|sequence| Outcome {
                    sequence: sequence.sequence.clone(),
                    forward: None,
                    text: "t".into(),
                    stop: Some("stop".into()),
                    terminal_generated: None,
                })
                .collect(),
            deployment: hop.deployment,
        });
    }
}

/// The only vocabulary these tests need: every frame is fresh, prompt-shaped
/// submission work, on both the hop-path's `sequence` seam and the relay's
/// `submission` seam. Real vocabularies (`p4-service`'s `Bodies`) draw a
/// sharper line between the two -- see that impl's own doc -- but that line
/// plays no part in what these tests prove.
struct SubmissionPayload;

impl Payload for SubmissionPayload {
    fn sequence(&self, frame: &Frame) -> Option<Sequence> {
        Some(Sequence {
            sequence: frame.envelope.route.clone(),
            session_epoch: 0,
            state: None,
            prompt: Some(String::from_utf8_lossy(&frame.body).into_owned()),
            remaining: 8,
            options: "{}".into(),
        })
    }

    fn submission(&self, frame: &Frame) -> Option<Submit> {
        if frame.body == b"not-a-submission" {
            return None;
        }
        let deployment_id = self.deployment(frame)?;
        let deployment_generation = frame.envelope.chain.as_ref()?.current().generation;
        Some(Submit {
            deployment_id,
            deployment_generation,
            submission_id: frame.envelope.route.clone(),
            deadline_unix_ms: frame.envelope.deadline_unix_ms,
            request: serde_json::json!({ "prompt": String::from_utf8_lossy(&frame.body) }),
        })
    }
}

/// Records every frame this agent's own duties see -- replies included,
/// since a relayed token or terminal comes back to the same agent that sent
/// the original submission in every test here.
#[derive(Default)]
struct Collect(Arc<StdMutex<Vec<Frame>>>);

impl Duties for Collect {
    fn handle(&self, frame: Frame, _: &Arc<Agent>) {
        self.0.lock().unwrap().push(frame);
    }
}

/// What `ScriptedClient` runs on every `try_submit`: `(call index, the
/// Submit, the sink)` in, nothing out.
type Script = Box<dyn Fn(usize, &Submit, &dyn DeploymentSink) + Send + Sync>;

/// A `Client` whose `try_submit` runs a caller-supplied script against the
/// sink it was constructed with, exactly mirroring how a real client's
/// admission thread would call back into `AgentDeploymentSink::raise` from
/// outside any lock this crate holds.
struct ScriptedClient {
    sink: Arc<dyn DeploymentSink>,
    calls: AtomicUsize,
    script: Script,
}

impl Client for ScriptedClient {
    fn try_submit(&self, submit: Submit) -> Result<(), EnqueueError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        (self.script)(call, &submit, self.sink.as_ref());
        Ok(())
    }

    fn cancel(&self, _submission_id: SubmissionId) {}
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap()
}

/// One agent, its own duties recording every frame it sees, running its own
/// worker pool -- no socket, since none of these tests cross a wire.
fn agent_with(seen: Arc<StdMutex<Vec<Frame>>>) -> Arc<Agent> {
    let (agent, receiver, in_flight) = Agent::new(
        Address::tcp("127.0.0.1", 0),
        Arc::new(Collect(seen)),
        Arc::new(SubmissionPayload),
        Lanes::default(),
        Budget::default(),
    );
    tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
    agent
}

/// One fresh submission-shaped frame targeting `node-1` under
/// `deployment_id`, addressed to and replying to `agent` itself.
fn submission_frame(agent: &Agent, deployment_id: &str, generation: u64) -> Frame {
    let chain = Chain::new(vec![Link {
        address: agent.address().clone(),
        node: "node-1".into(),
        binding: deployment_id.into(),
        generation,
    }])
    .unwrap();
    Frame {
        envelope: Envelope {
            target: agent.address().clone(),
            recipient: Recipient::node("node-1"),
            lane: QueueClass::Prefill,
            route: "route-1".into(),
            request_id: "request-1".into(),
            stream_id: "stream-1".into(),
            origin_agent: None,
            return_channel: None,
            ingress_generation: 0,
            event_seq: 0,
            deadline_unix_ms: 0,
            reply_to: Some(agent.address().clone()),
            chain: Some(chain),
        },
        body: b"hello".to_vec(),
    }
}

#[test]
fn a_registered_deployment_client_streams_tokens_and_one_terminal_over_a_real_socket_free_relay() {
    runtime().block_on(async {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let agent = agent_with(Arc::clone(&seen));
        let relay_sink: Arc<dyn DeploymentSink> =
            Arc::new(AgentDeploymentSink::new(Arc::downgrade(&agent)));
        let client = Arc::new(ScriptedClient {
            sink: relay_sink,
            calls: AtomicUsize::new(0),
            script: Box::new(|_, submit, sink| {
                let id = submit.submission_id.clone();
                sink.raise(DeploymentEvent::Accepted(Accepted {
                    submission_id: id.clone(),
                }));
                sink.raise(DeploymentEvent::Produced(Produced {
                    submission_id: id.clone(),
                    event_ordinal: 0,
                    text: "hel".into(),
                    generated_tokens: 1,
                }));
                sink.raise(DeploymentEvent::Produced(Produced {
                    submission_id: id.clone(),
                    event_ordinal: 1,
                    text: "lo".into(),
                    generated_tokens: 2,
                }));
                sink.raise(DeploymentEvent::Settled(Settled {
                    submission_id: id,
                    reason: SettledReason::Stop,
                    generated_tokens: 2,
                }));
            }),
        });
        agent.deployments().register("dep-1".into(), client);

        agent.enqueue(submission_frame(&agent, "dep-1", 7)).unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3, "two tokens and one terminal, nothing else");
        assert_eq!(seen[0].body, b"hel");
        assert_eq!(seen[1].body, b"lo");
        assert_eq!(seen[2].body, b"stop");
        for frame in seen.iter() {
            assert_eq!(frame.envelope.lane, QueueClass::Response);
            assert_eq!(frame.envelope.route, "route-1");
        }
        // `event_seq` counts events the requester received, not laps: three
        // events in, three consecutive sequence numbers out.
        assert_eq!(seen[0].envelope.event_seq, 1);
        assert_eq!(seen[1].envelope.event_seq, 2);
        assert_eq!(seen[2].envelope.event_seq, 3);
        assert_eq!(agent.to_deployment(), 1);
    });
}

#[test]
fn no_registered_client_leaves_the_hop_path_untouched() {
    runtime().block_on(async {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let agent = agent_with(Arc::clone(&seen));
        agent
            .create_node("node-1", Arc::new(InstantAdapter), 4)
            .await;
        // Deliberately nothing registered under "dep-1" -- the same frame a
        // registered client would have consumed instead must reach the node
        // exactly as it always has.

        agent.enqueue(submission_frame(&agent, "dep-1", 7)).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert_eq!(
            agent.to_deployment(),
            0,
            "the relay's gate never fired: nothing is registered for dep-1"
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "the node's own hop path answered instead");
        // `InstantAdapter` finishes on its very first hop, so `outcome/mod.rs`
        // reports it as `Next::Finish` -- `payload.finished(reason, ..)`,
        // never `payload.token`. The body itself is not this test's point;
        // that exactly one hop-path reply arrived, and the relay never fired
        // (`to_deployment() == 0` above), is.
        assert_eq!(seen[0].body, b"stop");
    });
}

#[test]
fn a_registered_deployment_never_falls_back_to_a_hop_for_unknown_work() {
    runtime().block_on(async {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let agent = agent_with(Arc::clone(&seen));
        agent
            .create_node("node-1", Arc::new(InstantAdapter), 4)
            .await;
        let relay_sink: Arc<dyn DeploymentSink> =
            Arc::new(AgentDeploymentSink::new(Arc::downgrade(&agent)));
        let client = Arc::new(ScriptedClient {
            sink: relay_sink,
            calls: AtomicUsize::new(0),
            script: Box::new(|_, _, _| {}),
        });
        agent
            .deployments()
            .register("dep-1".into(), Arc::clone(&client) as Arc<dyn Client>);

        let mut frame = submission_frame(&agent, "dep-1", 7);
        frame.body = b"not-a-submission".to_vec();
        frame.envelope.lane = QueueClass::Control;
        agent.enqueue(frame).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(client.calls.load(Ordering::SeqCst), 0);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            seen[0].body,
            b"registered deployment rejected non-submission message"
        );
    });
}

#[test]
fn a_full_that_reaches_p4_is_terminal_and_is_not_retried_by_p4() {
    runtime().block_on(async {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let agent = agent_with(Arc::clone(&seen));
        let relay_sink: Arc<dyn DeploymentSink> =
            Arc::new(AgentDeploymentSink::new(Arc::downgrade(&agent)));
        let client = Arc::new(ScriptedClient {
            sink: relay_sink,
            calls: AtomicUsize::new(0),
            script: Box::new(|_, submit, sink| {
                let id = submit.submission_id.clone();
                sink.raise(DeploymentEvent::Rejected(
                    p4_adapter::deployment::Rejected {
                        submission_id: id,
                        reason: RejectedReason::Full,
                    },
                ));
            }),
        });
        agent
            .deployments()
            .register("dep-1".into(), Arc::clone(&client) as Arc<dyn Client>);

        agent.enqueue(submission_frame(&agent, "dep-1", 7)).unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            client.calls.load(Ordering::SeqCst),
            1,
            "backend policy must not create a retry in the P4 relay"
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "the final refusal closes the route once");
        assert_eq!(seen[0].envelope.lane, QueueClass::Response);
    });
}

mod deliverability;

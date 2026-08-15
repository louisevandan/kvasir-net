//! The whole system, over sockets, with mocks behind the adapter boundary.
//!
//! Every agent here is the real one and every hop crosses a real connection.
//! What is simulated is only what sits below the interface, so anything these
//! runs catch is P4's.

use p4_adapter::{Adapter, Sequence};
use p4_agent_core::agent::{Agent, Duties, run};
use p4_agent_core::node::payload::Payload;
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_agent_core::transport::inbox;
use p4_mock::Mock;
use p4_mock::profile::Profile;
use p4_protocol::frame::Frame;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

/// Reads a body as "prompt|remaining", which is all a hop needs and keeps the
/// core free of a message catalog.
struct Bodies;

impl Payload for Bodies {
    fn sequence(&self, frame: &Frame) -> Option<Sequence> {
        let text = String::from_utf8_lossy(&frame.body).into_owned();
        let (prompt, remaining) = text.rsplit_once('|')?;
        Some(Sequence {
            sequence: frame.envelope.route.clone(),
            position: 0,
            prompt: Some(prompt.to_owned()),
            remaining: remaining.parse().ok()?,
            options: "{}".into(),
        })
    }
}

/// Stands in for OUTER: keeps whatever comes back, per route.
#[derive(Default, Clone)]
struct Outer(Arc<Mutex<HashMap<String, Vec<Frame>>>>);

impl Duties for Outer {
    fn handle(&self, frame: Frame, _: &Agent) {
        self.0
            .lock()
            .unwrap()
            .entry(frame.envelope.route.clone())
            .or_default()
            .push(frame);
    }
}

impl Outer {
    fn routes(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    fn frames_for(&self, route: &str) -> Vec<Frame> {
        self.0
            .lock()
            .unwrap()
            .get(route)
            .cloned()
            .unwrap_or_default()
    }

    fn total_frames(&self) -> usize {
        self.0.lock().unwrap().values().map(Vec::len).sum()
    }
}

/// A node that does nothing but pass messages on, which is what an agent
/// holding no node still has to do correctly.
#[derive(Default)]
struct Silent;

impl Duties for Silent {
    fn handle(&self, _: Frame, _: &Agent) {}
}

async fn start(duties: Arc<dyn Duties>) -> Arc<Agent> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (agent, receiver, in_flight) = Agent::new(
        Address::tcp("127.0.0.1", port),
        duties,
        Arc::new(Bodies),
        Lanes::default(),
        Budget::default(),
    );
    tokio::spawn(inbox::serve(listener, agent.queue(), 256));
    tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
    agent
}

fn chain_over(agents: &[(&Arc<Agent>, &str)]) -> Chain {
    Chain::new(
        agents
            .iter()
            .map(|(agent, node)| Link {
                address: (*agent).address().clone(),
                node: (*node).into(),
                binding: "deployment".into(),
                generation: 1,
            })
            .collect(),
    )
    .unwrap()
}

fn request(route: &str, chain: &Chain, outer: &Arc<Agent>, tokens: u32) -> Frame {
    Frame {
        envelope: Envelope {
            target: chain.current().address.clone(),
            recipient: Recipient::node(chain.current().node.clone()),
            lane: QueueClass::Prefill,
            route: route.into(),
            deadline_unix_ms: 0,
            reply_to: Some(outer.address().clone()),
            chain: Some(chain.clone()),
        },
        body: format!("prompt|{tokens}").into_bytes(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(8)
        .enable_all()
        .build()
        .unwrap()
}

async fn settle(millis: u64) {
    tokio::time::sleep(Duration::from_millis(millis)).await;
}

/// Waits for a condition instead of for a duration.
///
/// These runs share a machine with every other test, so a fixed sleep is a
/// guess that gets worse the busier the host is. The generous ceiling only
/// bounds a failure; a passing run leaves as soon as the condition holds.
async fn until(mut done: impl FnMut() -> bool) {
    for _ in 0..600 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[test]
fn a_three_stage_chain_generates_across_three_machines() {
    // Prefill traverses the chain once; every further token costs one lap of
    // the ring. Each stage lives on its own agent and every hop is a socket.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let b = start(Arc::new(Silent)).await;
        let c = start(Arc::new(Silent)).await;

        a.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        b.create_node("n1", Arc::new(Mock::staged(1, Profile::default())), 8)
            .await;
        c.create_node("n2", Arc::new(Mock::terminal(2, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1"), (&c, "n2")]);
        a.enqueue(request("r1", &chain, &outer, 4)).unwrap();
        until(|| outer_duties.frames_for("r1").len() >= 4).await;

        let frames = outer_duties.frames_for("r1");
        assert!(!frames.is_empty(), "the caller heard back");
        assert!(
            frames
                .iter()
                .all(|f| f.envelope.lane == QueueClass::Response),
            "everything returned on the response lane"
        );
        // Four tokens wanted: tokens reported, then one terminal.
        assert_eq!(frames.len(), 4, "one frame per token, ending in the stop");
        assert_eq!(frames.last().unwrap().body, b"stop");
    });
}

#[test]
fn a_single_node_chain_generates_the_same_way() {
    // How vLLM and SGLang participate: the model is spread inside the backend,
    // so the chain is one link and a lap is a decode step on the same node.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let only = start(Arc::new(Silent)).await;
        only.create_node("n0", Arc::new(Mock::internal(Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&only, "n0")]);
        only.enqueue(request("r1", &chain, &outer, 3)).unwrap();
        until(|| outer_duties.frames_for("r1").len() >= 3).await;

        let frames = outer_duties.frames_for("r1");
        let bodies: Vec<String> = frames
            .iter()
            .map(|f| String::from_utf8_lossy(&f.body).into_owned())
            .collect();
        assert_eq!(frames.len(), 3, "saw {bodies:?}");
        assert_eq!(
            frames.last().unwrap().body,
            b"stop",
            "the stream ended on its terminal, saw {bodies:?}"
        );
    });
}

#[test]
fn every_request_of_a_crowd_reaches_a_terminal() {
    // Far more arrivals than the ceiling admits. None may be lost, and none
    // may produce two terminals.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let b = start(Arc::new(Silent)).await;

        let first = Arc::new(Mock::staged(0, Profile::default()));
        a.create_node("n0", Arc::clone(&first) as Arc<dyn Adapter>, 4)
            .await;
        b.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 4)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1")]);
        for index in 0..40 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 1))
                .unwrap();
        }
        until(|| outer_duties.routes() >= 40).await;

        assert_eq!(outer_duties.routes(), 40, "every route answered");
        assert_eq!(outer_duties.total_frames(), 40, "exactly one terminal each");
        assert!(
            first.widths().iter().all(|width| *width <= 4),
            "no hop exceeded the declared ceiling: {:?}",
            first.widths()
        );
        assert_eq!(
            first.peak_concurrent_hops(),
            1,
            "a node never ran two hops at once"
        );
    });
}

#[test]
fn arrivals_are_batched_rather_than_serialised() {
    // A window wider than one is the whole reason a hop carries a batch. If
    // every hop were width one, the node would be serialising.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;

        let node = Arc::new(Mock::terminal(
            0,
            Profile::measured_shape(Duration::from_millis(15)),
        ));
        a.create_node("n0", Arc::clone(&node) as Arc<dyn Adapter>, 16)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        for index in 0..32 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 1))
                .unwrap();
        }
        until(|| outer_duties.routes() >= 32).await;

        let widest = node.widths().into_iter().max().unwrap_or(0);
        assert!(widest > 1, "hops carried batches, widest was {widest}");
        assert!(widest <= 16, "and never past the ceiling");
    });
}

#[test]
fn a_slow_node_shows_up_as_node_depth_not_agent_depth() {
    // The attribution that makes "is this P4's fault" answerable. With a slow
    // backend the work should pile up behind the adapter, not in front of it.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;

        let slow = Profile {
            leading_hop: Duration::from_millis(60),
            ..Profile::default()
        };
        a.create_node("n0", Arc::new(Mock::terminal(0, slow)), 2)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        for index in 0..24 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 1))
                .unwrap();
        }
        settle(200).await;

        let node_depth = a.node_depth("n0").await.unwrap_or(0);
        assert!(
            node_depth > 0,
            "work waited on the node, where the slowness is"
        );
    });
}

#[test]
fn a_chain_through_an_agent_that_owns_no_node_still_works() {
    // The entry agent is a pure entry point: it holds no node and only relays.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let entry = start(Arc::new(Silent)).await;
        let worker = start(Arc::new(Silent)).await;
        worker
            .create_node("n0", Arc::new(Mock::terminal(0, Profile::default())), 4)
            .await;

        let chain = chain_over(&[(&worker, "n0")]);
        // Sent to the entry agent, addressed at the worker's node.
        entry.enqueue(request("r1", &chain, &outer, 2)).unwrap();
        until(|| outer_duties.frames_for("r1").len() >= 2).await;

        assert_eq!(outer_duties.frames_for("r1").len(), 2);
    });
}

#[test]
fn sustained_arrivals_keep_completing_while_more_come_in() {
    // Cohort replacement: work continues to arrive while earlier work is still
    // in flight, and the node keeps admitting as it frees up.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let b = start(Arc::new(Silent)).await;

        let first = Arc::new(Mock::staged(
            0,
            Profile {
                leading_hop: Duration::from_millis(4),
                ..Profile::default()
            },
        ));
        a.create_node("n0", Arc::clone(&first) as Arc<dyn Adapter>, 6)
            .await;
        b.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 6)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1")]);
        for wave in 0..6 {
            for index in 0..8 {
                a.enqueue(request(&format!("w{wave}-r{index}"), &chain, &outer, 1))
                    .unwrap();
            }
            settle(60).await;
        }
        until(|| outer_duties.routes() >= 48).await;

        assert_eq!(outer_duties.routes(), 48, "every wave completed");
        assert_eq!(first.peak_concurrent_hops(), 1);
        assert!(first.widths().iter().all(|width| *width <= 6));
    });
}

#[test]
fn a_failing_backend_answers_the_route_instead_of_stranding_it() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node(
            "n0",
            Arc::new(Mock::terminal(
                0,
                Profile {
                    fault: p4_mock::profile::Fault::Hop,
                    ..Profile::default()
                },
            )),
            4,
        )
        .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(request("r1", &chain, &outer, 2)).unwrap();
        until(|| !outer_duties.frames_for("r1").is_empty()).await;

        let frames = outer_duties.frames_for("r1");
        assert_eq!(frames.len(), 1, "the caller was told once");
        assert!(
            String::from_utf8_lossy(&frames[0].body).contains("fail"),
            "and told what happened"
        );
    });
}

#[test]
fn concurrent_chains_on_shared_agents_do_not_mix_their_routes() {
    // Two chains crossing the same two agents. A token belonging to one route
    // must never surface under the other.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let b = start(Arc::new(Silent)).await;

        a.create_node("left", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        a.create_node("right", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        b.create_node("tail", Arc::new(Mock::terminal(1, Profile::default())), 8)
            .await;

        let left = chain_over(&[(&a, "left"), (&b, "tail")]);
        let right = chain_over(&[(&a, "right"), (&b, "tail")]);
        for index in 0..10 {
            a.enqueue(request(&format!("L{index}"), &left, &outer, 2))
                .unwrap();
            a.enqueue(request(&format!("R{index}"), &right, &outer, 3))
                .unwrap();
        }
        until(|| outer_duties.routes() >= 20).await;

        for index in 0..10 {
            assert_eq!(
                outer_duties.frames_for(&format!("L{index}")).len(),
                2,
                "left route {index} got its own token count"
            );
            assert_eq!(
                outer_duties.frames_for(&format!("R{index}")).len(),
                3,
                "right route {index} got its own token count"
            );
        }
    });
}

#[test]
fn a_token_stream_arrives_in_the_order_it_was_produced() {
    // Registration order is the only ordering guarantee there is, so it has to
    // survive the queue, the worker pool and the wire. Handing consecutive
    // frames of one route to different workers is what broke this before.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let b = start(Arc::new(Silent)).await;

        a.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        b.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1")]);
        for index in 0..6 {
            a.enqueue(request(&format!("s{index}"), &chain, &outer, 5))
                .unwrap();
        }
        until(|| (0..6).all(|i| outer_duties.frames_for(&format!("s{i}")).len() >= 5)).await;

        for index in 0..6 {
            let route = format!("s{index}");
            let bodies: Vec<String> = outer_duties
                .frames_for(&route)
                .iter()
                .map(|f| String::from_utf8_lossy(&f.body).into_owned())
                .collect();
            assert_eq!(
                bodies,
                vec![
                    format!("{route}#1 "),
                    format!("{route}#2 "),
                    format!("{route}#3 "),
                    format!("{route}#4 "),
                    "stop".to_string(),
                ],
                "route {route} arrived out of order"
            );
        }
    });
}

/// A body of "load|<ceiling>" or "unload" is lifecycle; anything else is a
/// sequence. Still no message catalog in the core — this is the seam.
struct Lifecycle;

impl Payload for Lifecycle {
    fn sequence(&self, frame: &Frame) -> Option<Sequence> {
        Bodies.sequence(frame)
    }

    fn lifecycle(&self, frame: &Frame) -> Option<p4_adapter::Work> {
        let text = String::from_utf8_lossy(&frame.body).into_owned();
        let deployment = self.deployment(frame)?;
        if text.starts_with("load|") {
            return Some(p4_adapter::Work::Load(p4_adapter::Load {
                deployment,
                plan: text,
                artifact: "model".into(),
            }));
        }
        (text == "unload").then(|| p4_adapter::Work::Unload(p4_adapter::Unload { deployment }))
    }

    fn ceiling(&self, frame: &Frame) -> Option<usize> {
        String::from_utf8_lossy(&frame.body)
            .strip_prefix("load|")?
            .parse()
            .ok()
    }
}

async fn start_with(duties: Arc<dyn Duties>, payload: Arc<dyn Payload>) -> Arc<Agent> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (agent, receiver, in_flight) = Agent::new(
        Address::tcp("127.0.0.1", port),
        duties,
        payload,
        Lanes::default(),
        Budget::default(),
    );
    tokio::spawn(inbox::serve(listener, agent.queue(), 256));
    tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
    agent
}

fn control(route: &str, chain: &Chain, outer: &Arc<Agent>, body: &str) -> Frame {
    Frame {
        envelope: Envelope {
            target: chain.current().address.clone(),
            recipient: Recipient::node(chain.current().node.clone()),
            lane: QueueClass::Control,
            route: route.into(),
            deadline_unix_ms: 0,
            reply_to: Some(outer.address().clone()),
            chain: Some(chain.clone()),
        },
        body: body.as_bytes().to_vec(),
    }
}

#[test]
fn a_distributed_load_reports_every_stage_then_binds() {
    // The load workload: a model spread over stages, each reporting on its own,
    // and the deployment executable only once the last one is in.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;
        let a = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
        a.create_node(
            "n0",
            Arc::new(Mock::terminal(
                0,
                Profile {
                    stages: 4,
                    ..Profile::default()
                },
            )),
            1,
        )
        .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(control("load-1", &chain, &outer, "load|6")).unwrap();
        until(|| outer_duties.frames_for("load-1").len() >= 5).await;

        let bodies: Vec<String> = outer_duties
            .frames_for("load-1")
            .iter()
            .map(|f| String::from_utf8_lossy(&f.body).into_owned())
            .collect();
        assert_eq!(bodies.len(), 5, "four stages then the binding: {bodies:?}");
        assert!(bodies[0].starts_with("stage 0"), "{bodies:?}");
        assert!(bodies[3].starts_with("stage 3"), "{bodies:?}");
        assert!(bodies[4].starts_with("loaded generation 1"), "{bodies:?}");
    });
}

#[test]
fn a_load_raises_the_ceiling_the_plan_declared() {
    // The ceiling is declared, never derived. Before the load the node admits
    // one at a time; after it, the declared width.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;
        let a = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
        let node = Arc::new(Mock::terminal(0, Profile::default()));
        a.create_node("n0", Arc::clone(&node) as Arc<dyn Adapter>, 1).await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(control("load-1", &chain, &outer, "load|8")).unwrap();
        until(|| !outer_duties.frames_for("load-1").is_empty()).await;

        for index in 0..24 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 1))
                .unwrap();
        }
        until(|| outer_duties.routes() >= 25).await;

        let widest = node.widths().into_iter().max().unwrap_or(0);
        assert!(widest > 1, "the declared ceiling was used, widest {widest}");
        assert!(widest <= 8, "and not exceeded, widest {widest}");
    });
}

#[test]
fn an_unload_is_reported_and_the_node_keeps_serving_afterwards() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;
        let a = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
        a.create_node("n0", Arc::new(Mock::terminal(0, Profile::default())), 4)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(control("unload-1", &chain, &outer, "unload")).unwrap();
        until(|| !outer_duties.frames_for("unload-1").is_empty()).await;

        assert_eq!(outer_duties.frames_for("unload-1")[0].body, b"unloaded");

        // The node is idle again rather than stuck holding a finished
        // instruction, so ordinary work still runs.
        a.enqueue(request("after", &chain, &outer, 1)).unwrap();
        until(|| !outer_duties.frames_for("after").is_empty()).await;
        assert_eq!(outer_duties.frames_for("after").len(), 1);
    });
}

#[test]
fn a_load_that_fails_is_reported_and_does_not_wedge_the_node() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;
        let a = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
        a.create_node(
            "n0",
            Arc::new(Mock::terminal(
                0,
                Profile {
                    fault: p4_mock::profile::Fault::Load,
                    ..Profile::default()
                },
            )),
            4,
        )
        .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(control("load-1", &chain, &outer, "load|4")).unwrap();
        until(|| !outer_duties.frames_for("load-1").is_empty()).await;

        let bodies: Vec<String> = outer_duties
            .frames_for("load-1")
            .iter()
            .map(|f| String::from_utf8_lossy(&f.body).into_owned())
            .collect();
        assert!(
            bodies.iter().any(|body| body.contains("fail")),
            "the caller was told: {bodies:?}"
        );
        assert_eq!(a.node_depth("n0").await, Some(0), "nothing left stuck");
    });
}

fn expiring(route: &str, chain: &Chain, outer: &Arc<Agent>, deadline_unix_ms: u64) -> Frame {
    let mut frame = request(route, chain, outer, 2);
    frame.envelope.deadline_unix_ms = deadline_unix_ms;
    frame
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

#[test]
fn work_whose_deadline_already_passed_is_answered_rather_than_run() {
    // Expired work must not reach a backend, and must not vanish either: a
    // caller left waiting for a terminal that never comes is what a leaked
    // route looks like from outside.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let node = Arc::new(Mock::terminal(0, Profile::default()));
        a.create_node("n0", Arc::clone(&node) as Arc<dyn Adapter>, 4).await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(expiring("late", &chain, &outer, now_unix_ms() - 1_000))
            .unwrap();
        until(|| !outer_duties.frames_for("late").is_empty()).await;

        let body = String::from_utf8_lossy(&outer_duties.frames_for("late")[0].body).into_owned();
        assert!(body.contains("deadline"), "the caller was told why: {body}");
        assert!(
            node.widths().is_empty(),
            "expired work never reached the backend"
        );
    });
}

#[test]
fn a_live_deadline_does_not_stop_ordinary_work() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node("n0", Arc::new(Mock::terminal(0, Profile::default())), 4)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(expiring("soon", &chain, &outer, now_unix_ms() + 60_000))
            .unwrap();
        until(|| outer_duties.frames_for("soon").len() >= 2).await;

        assert_eq!(outer_duties.frames_for("soon").len(), 2);
    });
}

#[test]
fn cancelling_stops_the_work_that_has_not_started() {
    // A hop already inside a backend runs to its boundary; cancelling means
    // the next one never starts. Here the backend never answers, so everything
    // behind the first hop is still waiting and can be withdrawn.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node(
            "n0",
            Arc::new(Mock::terminal(
                0,
                Profile {
                    fault: p4_mock::profile::Fault::Silence,
                    ..Profile::default()
                },
            )),
            1,
        )
        .await;

        let chain = chain_over(&[(&a, "n0")]);
        for index in 0..6 {
            a.enqueue(request(&format!("c{index}"), &chain, &outer, 1))
                .unwrap();
        }
        // The first hop is inside the silent backend; the rest are queued.
        settle(300).await;

        let mut cancelled = 0;
        for index in 0..6 {
            if a.cancel(&format!("c{index}")).await {
                cancelled += 1;
            }
        }
        assert!(cancelled >= 5, "the queued work was withdrawn, {cancelled}");
        assert_eq!(a.node_depth("n0").await, Some(0), "nothing left queued");
    });
}

#[test]
fn cancelling_a_route_that_already_finished_says_so() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node("n0", Arc::new(Mock::terminal(0, Profile::default())), 4)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(request("done", &chain, &outer, 1)).unwrap();
        until(|| !outer_duties.frames_for("done").is_empty()).await;

        assert!(
            !a.cancel("done").await,
            "there was nothing left to cancel, which is not the same as failing"
        );
    });
}

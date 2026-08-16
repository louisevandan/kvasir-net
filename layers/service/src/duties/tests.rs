use super::*;
use crate::message::wire::{decode_reply, encode_to_agent};
use crate::payload::Bodies;
use p4_adapter::{Adapter, Distribution, EventSink, Work};
use p4_agent_core::agent::run;
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_agent_core::transport::inbox;
use p4_protocol::{Address, Envelope, QueueClass, Recipient};
use std::sync::Mutex;
use std::time::Duration;
use tokio::net::TcpListener;

struct Stub;

impl Adapter for Stub {
    fn distribution(&self) -> Distribution {
        Distribution::Staged
    }
    fn start(&self, _: Work, _: &dyn EventSink) {}
}

#[derive(Default, Clone)]
struct Caller(Arc<Mutex<Vec<Reply>>>);

impl Duties for Caller {
    fn handle(&self, frame: Frame, _: &Arc<Agent>) {
        if let Ok(reply) = decode_reply(&frame.body) {
            self.0.lock().unwrap().push(reply);
        }
    }
}

impl Caller {
    fn replies(&self) -> Vec<Reply> {
        self.0.lock().unwrap().clone()
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap()
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
    tokio::spawn(inbox::serve(listener, agent.queue(), 64));
    tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
    agent
}

fn ask(target: &Arc<Agent>, caller: &Arc<Agent>, route: &str, message: ToAgent) -> Frame {
    Frame {
        envelope: Envelope {
            target: target.address().clone(),
            recipient: Recipient::Agent,
            lane: QueueClass::Control,
            route: route.into(),
            deadline_unix_ms: 0,
            reply_to: Some(caller.address().clone()),
            chain: None,
        },
        body: encode_to_agent(&message),
    }
}

async fn until(mut done: impl FnMut() -> bool) {
    for _ in 0..300 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn with_stub() -> Registry {
    let mut registry = Registry::new();
    registry.register_fn("stub", |_| Arc::new(Stub));
    registry
}

#[test]
fn creating_a_node_names_a_registered_adapter() {
    runtime().block_on(async {
        let caller_duties = Caller::default();
        let caller = start(Arc::new(caller_duties.clone())).await;
        let agent = start(Arc::new(Standard::new(with_stub()))).await;

        agent
            .enqueue(ask(
                &agent,
                &caller,
                "r1",
                ToAgent::CreateNode {
                    node: "n0".into(),
                    adapter: "stub".into(),
                },
            ))
            .unwrap();
        until(|| !caller_duties.replies().is_empty()).await;

        assert!(matches!(
            caller_duties.replies().first(),
            Some(Reply::Accepted { .. })
        ));
        assert_eq!(agent.node_depth("n0").await, Some(0), "the node exists");
    });
}

#[test]
fn naming_an_adapter_this_process_does_not_have_is_refused() {
    // A placement mistake the caller is the only one who can fix, so it is
    // reported rather than defaulted to something that happens to be present.
    runtime().block_on(async {
        let caller_duties = Caller::default();
        let caller = start(Arc::new(caller_duties.clone())).await;
        let agent = start(Arc::new(Standard::new(with_stub()))).await;

        agent
            .enqueue(ask(
                &agent,
                &caller,
                "r1",
                ToAgent::CreateNode {
                    node: "n0".into(),
                    adapter: "llamacpp".into(),
                },
            ))
            .unwrap();
        until(|| !caller_duties.replies().is_empty()).await;

        let Some(Reply::Failed { detail }) = caller_duties.replies().pop() else {
            panic!("expected a refusal");
        };
        assert!(detail.contains("llamacpp"), "{detail}");
        assert_eq!(agent.node_depth("n0").await, None, "no node was created");
    });
}

#[test]
fn deleting_a_node_reports_whether_there_was_one() {
    runtime().block_on(async {
        let caller_duties = Caller::default();
        let caller = start(Arc::new(caller_duties.clone())).await;
        let agent = start(Arc::new(Standard::new(with_stub()))).await;

        agent
            .enqueue(ask(
                &agent,
                &caller,
                "r1",
                ToAgent::CreateNode {
                    node: "n0".into(),
                    adapter: "stub".into(),
                },
            ))
            .unwrap();
        until(|| !caller_duties.replies().is_empty()).await;

        agent
            .enqueue(ask(
                &agent,
                &caller,
                "r2",
                ToAgent::DeleteNode { node: "n0".into() },
            ))
            .unwrap();
        until(|| caller_duties.replies().len() >= 2).await;
        assert!(matches!(caller_duties.replies()[1], Reply::Released));

        agent
            .enqueue(ask(
                &agent,
                &caller,
                "r3",
                ToAgent::DeleteNode { node: "n0".into() },
            ))
            .unwrap();
        until(|| caller_duties.replies().len() >= 3).await;
        assert!(
            matches!(caller_duties.replies()[2], Reply::Failed { .. }),
            "deleting what is not there is not the same as deleting it"
        );
    });
}

#[test]
fn inspecting_reports_the_platform_and_the_adapters_it_can_serve() {
    runtime().block_on(async {
        let caller_duties = Caller::default();
        let caller = start(Arc::new(caller_duties.clone())).await;
        let agent = start(Arc::new(Standard::new(with_stub()))).await;

        agent
            .enqueue(ask(&agent, &caller, "r1", ToAgent::Inspect))
            .unwrap();
        until(|| !caller_duties.replies().is_empty()).await;

        let Some(Reply::Machine { snapshot }) = caller_duties.replies().pop() else {
            panic!("expected a snapshot");
        };
        assert!(snapshot.contains(r#""adapters":["stub"]"#), "{snapshot}");
        assert!(snapshot.contains(std::env::consts::OS), "{snapshot}");
    });
}

#[test]
fn a_body_that_is_not_an_agent_message_is_answered_rather_than_dropped() {
    runtime().block_on(async {
        let caller_duties = Caller::default();
        let caller = start(Arc::new(caller_duties.clone())).await;
        let agent = start(Arc::new(Standard::new(with_stub()))).await;

        let mut frame = ask(&agent, &caller, "r1", ToAgent::Inspect);
        frame.body = vec![200, 200];
        agent.enqueue(frame).unwrap();
        until(|| !caller_duties.replies().is_empty()).await;

        assert!(matches!(
            caller_duties.replies().first(),
            Some(Reply::Failed { .. })
        ));
    });
}

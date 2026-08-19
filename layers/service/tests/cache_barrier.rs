//! Four-stage KV transaction proof at the agent/P4 boundary.

mod common;

use common::conversation::cache_op;
use common::{Outer, chain_over, runtime, start, to_agent, to_node, until};
use p4_adapter::{Adapter, Distribution, EventSink, Work};
use p4_agent_core::agent::Agent;
use p4_mock::{
    Mock,
    profile::{Fault, Profile},
};
use p4_protocol::QueueClass;
use p4_service::cache::{
    CacheExecutionAdmission, CacheTransaction, CacheTransactionKind, CacheTransactionState,
    StageCommand,
};
use p4_service::message::{Reply, ToAgent, ToNode};
use p4_service::{Registry, Standard};
use std::sync::Arc;
use std::sync::Mutex;

struct RestoreBarrierAdapter {
    inner: Arc<Mock>,
    restore_operations: Arc<Mutex<Vec<String>>>,
    restore_active: Arc<std::sync::atomic::AtomicUsize>,
    inference_during_restore: Arc<std::sync::atomic::AtomicBool>,
    inference_active: Arc<std::sync::atomic::AtomicUsize>,
    restore_during_inference: Arc<std::sync::atomic::AtomicBool>,
}

impl Adapter for RestoreBarrierAdapter {
    fn inspect_model(&self, artifact: &str) -> Result<String, String> {
        self.inner.inspect_model(artifact)
    }

    fn distribution(&self) -> Distribution {
        self.inner.distribution()
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        let is_restore = matches!(
            &work,
            Work::Cache(cache) if matches!(cache.action, p4_adapter::CacheAction::Restore)
        );
        let is_hop = matches!(&work, Work::Hop(_));
        if is_restore
            && self
                .inference_active
                .load(std::sync::atomic::Ordering::SeqCst)
                > 0
        {
            self.restore_during_inference
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if is_hop
            && self
                .restore_active
                .load(std::sync::atomic::Ordering::SeqCst)
                > 0
        {
            self.inference_during_restore
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if is_hop {
            self.inference_active
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        if let Work::Cache(cache) = &work
            && is_restore
        {
            self.restore_operations
                .lock()
                .unwrap()
                .push(cache.operation_id.clone());
            self.restore_active
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // Keep the first restore in the node's lifecycle slot long
            // enough for the queued inference to prove it is blocked.
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        self.inner.start(work, events);
        if is_restore {
            self.restore_active
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
        if is_hop {
            self.inference_active
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn report(&self) -> String {
        self.inner.report()
    }
}

struct Fleet {
    outer: Arc<Agent>,
    seen: Outer,
    agents: Vec<Arc<Agent>>,
    mocks: Vec<Arc<Mock>>,
}

async fn fleet(fault_stage: Option<usize>) -> Fleet {
    let seen = Outer::default();
    let outer = start(Arc::new(seen.clone())).await;
    let mut registry = Registry::new();
    let mocks = (0..4)
        .map(|stage| {
            let profile = (fault_stage == Some(stage)).then_some(Fault::CacheCommit);
            let mock = Mock::internal(Profile {
                fault: profile.unwrap_or(Fault::None),
                ..Profile::default()
            });
            let mock = Arc::new(mock);
            let instance: Arc<dyn p4_adapter::Adapter> = mock.clone();
            registry.register_fn(format!("stage{stage}"), move |_| Arc::clone(&instance));
            mock
        })
        .collect::<Vec<_>>();
    let mut agents = Vec::new();
    for _ in 0..4 {
        agents.push(start(Arc::new(Standard::new(registry.clone()))).await);
    }
    for (stage, agent) in agents.iter().enumerate() {
        let node = format!("s{stage}");
        let create = format!("create-{stage}");
        agent
            .enqueue(to_agent(
                agent,
                &outer,
                &create,
                ToAgent::CreateNode {
                    node: node.clone(),
                    adapter: format!("stage{stage}"),
                },
            ))
            .unwrap();
        until(|| !seen.replies(&create).is_empty()).await;
        let load = format!("load-{stage}");
        agent
            .enqueue(to_node(
                &chain_over(&[(agent, node.as_str())]),
                &outer,
                &load,
                QueueClass::Control,
                ToNode::Load {
                    plan: "{}".into(),
                    artifact: "model.gguf".into(),
                    ceiling: 1,
                    capability_snapshot_id: "test-snapshot".into(),
                    capability_expires_at: u64::MAX,
                },
            ))
            .unwrap();
        until(|| {
            seen.replies(&load)
                .iter()
                .any(|reply| matches!(reply, Reply::Bound { .. }))
        })
        .await;
    }
    Fleet {
        outer,
        seen,
        agents,
        mocks,
    }
}

async fn seed(fleet: &Fleet) -> Vec<u32> {
    for stage in 0..4 {
        let route = format!("seed-{stage}");
        let mut frame = to_node(
            &chain_over(&[(&fleet.agents[stage], format!("s{stage}").as_str())]),
            &fleet.outer,
            &route,
            QueueClass::Prefill,
            ToNode::Execute {
                prompt: "seed".into(),
                max_tokens: 1,
                options: "{}".into(),
            },
        );
        frame.envelope.request_id = "session".into();
        fleet.agents[stage].enqueue(frame).unwrap();
        until(|| {
            fleet
                .seen
                .replies(&route)
                .iter()
                .any(|reply| matches!(reply, Reply::Done { .. }))
        })
        .await;
    }
    fleet
        .mocks
        .iter()
        .map(|mock| mock.hop_observations().last().unwrap().outcomes[0].position)
        .collect()
}

async fn command(fleet: &Fleet, txid: &str, ordinal: usize, command: &StageCommand) -> Reply {
    let route = format!("{txid}-{ordinal}-{}", command.stage);
    let stage = command.stage[1..].parse::<usize>().unwrap();
    let mut frame = to_node(
        &chain_over(&[(&fleet.agents[stage], command.stage.as_str())]),
        &fleet.outer,
        &route,
        QueueClass::Control,
        command.body.clone(),
    );
    // Every stage sees the same operation id; route remains unique for replies.
    frame.envelope.request_id = txid.into();
    fleet.agents[stage].enqueue(frame).unwrap();
    until(|| !fleet.seen.replies(&route).is_empty()).await;
    let reply = fleet.seen.replies(&route).into_iter().last().unwrap();
    if let Reply::CacheFailed {
        deployment,
        stage_id,
        generation,
        operation_id,
        sequence,
        ..
    } = &reply
    {
        assert_eq!(deployment, "deployment");
        assert_eq!(stage_id, &command.stage);
        assert_eq!(*generation, 1);
        assert_eq!(operation_id, txid);
        assert_eq!(sequence, "session");
    }
    reply
}

async fn run_transaction(
    fleet: &Fleet,
    txid: &str,
    kind: CacheTransactionKind,
) -> CacheTransaction {
    let mut tx = CacheTransaction::new(
        txid,
        "session",
        "deployment",
        1,
        ["s0", "s1", "s2", "s3"],
        kind,
    )
    .unwrap();
    let mut wave = tx.start();
    let mut ordinal = 0;
    while !wave.is_empty() {
        let current = std::mem::take(&mut wave);
        let mut next = Vec::new();
        for command_to_send in current {
            let reply = command(fleet, txid, ordinal, &command_to_send).await;
            ordinal += 1;
            next.extend(tx.observe_command(&command_to_send, &reply).unwrap());
        }
        wave = next;
    }
    tx
}

#[test]
fn four_stage_save_unload_reload_restore_is_equivalent() {
    runtime().block_on(async {
        let fleet = fleet(None).await;
        let positions = seed(&fleet).await;
        let tx = run_transaction(&fleet, "save-four", CacheTransactionKind::Persist).await;
        assert_eq!(
            tx.state(),
            CacheTransactionState::Complete,
            "save replies: {:?}; states: {:?}",
            fleet.seen.replies("save-four-0-s0"),
            fleet
                .mocks
                .iter()
                .map(|mock| mock.cache_state("session"))
                .collect::<Vec<_>>()
        );
        assert!(fleet.mocks.iter().all(|mock| {
            let state = mock.cache_state("session");
            state.persisted && !state.resident
        }));

        // Unload/reload is deliberately outside the transaction: durable KV
        // must survive the model object's lifetime, then restore atomically.
        for stage in 0..4 {
            let route = format!("unload-{stage}");
            fleet.agents[stage]
                .enqueue(to_node(
                    &chain_over(&[(&fleet.agents[stage], format!("s{stage}").as_str())]),
                    &fleet.outer,
                    &route,
                    QueueClass::Control,
                    ToNode::Unload,
                ))
                .unwrap();
            until(|| !fleet.seen.replies(&route).is_empty()).await;
        }
        let restored = run_transaction(&fleet, "restore-four", CacheTransactionKind::Restore).await;
        assert_eq!(
            restored.state(),
            CacheTransactionState::Complete,
            "restore states: {:?}",
            fleet
                .mocks
                .iter()
                .map(|mock| mock.cache_state("session"))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            restored.execution_admission(),
            CacheExecutionAdmission::Allowed
        );
        assert!(
            fleet
                .mocks
                .iter()
                .all(|mock| mock.cache_state("session").resident)
        );
        let after = seed(&fleet).await;
        assert_eq!(
            after, positions,
            "restore resumed each stage at the same KV position"
        );
    });
}

#[test]
fn a_commit_failure_rolls_back_all_four_stages() {
    runtime().block_on(async {
        let fleet = fleet(Some(2)).await;
        seed(&fleet).await;
        let tx = run_transaction(&fleet, "save-rollback", CacheTransactionKind::Persist).await;
        assert_eq!(tx.state(), CacheTransactionState::Failed);
        assert_eq!(
            tx.execution_admission(),
            CacheExecutionAdmission::TransactionFailed
        );
        assert!(
            matches!(
                fleet.seen.replies("save-rollback-6-s2").first(),
                Some(Reply::CacheFailed { .. })
            ),
            "the failing commit reached the outer as CacheFailed: {:?}",
            fleet.seen.replies("save-rollback-6-s2")
        );
        assert!(
            fleet.mocks.iter().all(|mock| {
                let state = mock.cache_state("session");
                state.resident && !state.persisted
            }),
            "rollback states: {:?}",
            fleet
                .mocks
                .iter()
                .map(|mock| mock.cache_state("session"))
                .collect::<Vec<_>>()
        );
    });
}

#[test]
fn restore_is_ordered_by_sequence_and_blocks_inference_until_complete() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let inner = Arc::new(Mock::internal(Profile {
            leading_hop: std::time::Duration::from_millis(100),
            ..Profile::default()
        }));
        let restore_operations = Arc::new(Mutex::new(Vec::new()));
        let restore_active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let inference_during_restore = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let inference_active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let restore_during_inference = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let adapter = Arc::new(RestoreBarrierAdapter {
            inner: Arc::clone(&inner),
            restore_operations: Arc::clone(&restore_operations),
            restore_active: Arc::clone(&restore_active),
            inference_during_restore: Arc::clone(&inference_during_restore),
            inference_active: Arc::clone(&inference_active),
            restore_during_inference: Arc::clone(&restore_during_inference),
        });
        let mut registry = Registry::new();
        let registered: Arc<dyn Adapter> = adapter;
        registry.register_fn("restore-barrier", move |_| Arc::clone(&registered));
        let agent = start(Arc::new(Standard::new(registry))).await;

        agent
            .enqueue(to_agent(
                &agent,
                &outer,
                "create",
                ToAgent::CreateNode {
                    node: "n0".into(),
                    adapter: "restore-barrier".into(),
                },
            ))
            .unwrap();
        until(|| !seen.replies("create").is_empty()).await;
        let chain = chain_over(&[(&agent, "n0")]);
        agent
            .enqueue(to_node(
                &chain,
                &outer,
                "load",
                QueueClass::Control,
                ToNode::Load {
                    plan: "{}".into(),
                    artifact: "model.gguf".into(),
                    ceiling: 1,
                    capability_snapshot_id: "test-snapshot".into(),
                    capability_expires_at: u64::MAX,
                },
            ))
            .unwrap();
        until(|| {
            seen.replies("load")
                .iter()
                .any(|reply| matches!(reply, Reply::Bound { .. }))
        })
        .await;

        // Create the durable KV copy that both restore requests address.
        agent
            .enqueue(to_node(
                &chain,
                &outer,
                "session",
                QueueClass::Prefill,
                ToNode::Execute {
                    prompt: "seed".into(),
                    max_tokens: 1,
                    options: "{}".into(),
                },
            ))
            .unwrap();
        until(|| {
            seen.replies("session")
                .iter()
                .any(|reply| matches!(reply, Reply::Done { .. }))
        })
        .await;
        cache_op(
            &agent,
            &outer,
            &seen,
            "n0",
            "persist-session",
            ToNode::Persist {
                sequence: "session".into(),
            },
        )
        .await;

        agent
            .enqueue(to_node(
                &chain,
                &outer,
                "active-inference",
                QueueClass::Prefill,
                ToNode::Execute {
                    prompt: "boundary".into(),
                    max_tokens: 1,
                    options: "{}".into(),
                },
            ))
            .unwrap();
        until(|| inner.hop_observations().len() == 2).await;

        for route in ["restore-first", "restore-second"] {
            agent
                .enqueue(to_node(
                    &chain,
                    &outer,
                    route,
                    QueueClass::Control,
                    ToNode::Restore {
                        sequence: "session".into(),
                    },
                ))
                .unwrap();
        }
        agent
            .enqueue(to_node(
                &chain,
                &outer,
                "after-restore",
                QueueClass::Prefill,
                ToNode::Execute {
                    prompt: "after restore".into(),
                    max_tokens: 1,
                    options: "{}".into(),
                },
            ))
            .unwrap();

        until(|| {
            seen.replies("active-inference")
                .iter()
                .any(|reply| matches!(reply, Reply::Done { .. }))
        })
        .await;
        until(|| seen.replies("restore-second").len() == 1).await;
        until(|| {
            seen.replies("after-restore")
                .iter()
                .any(|reply| matches!(reply, Reply::Done { .. }))
        })
        .await;
        assert_eq!(
            restore_operations.lock().unwrap().as_slice(),
            ["restore-first", "restore-second"],
            "same sequence restores execute in queue order"
        );
        assert!(
            !inference_during_restore.load(std::sync::atomic::Ordering::SeqCst),
            "inference must remain blocked while restore is running"
        );
        assert!(
            !restore_during_inference.load(std::sync::atomic::Ordering::SeqCst),
            "restore must wait behind the active inference boundary"
        );
    });
}

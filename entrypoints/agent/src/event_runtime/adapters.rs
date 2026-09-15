//! Composition-only factories. Discovery and CREATE consume the same entries.
use p4_adapter::node_adapter::RetainedNodeAdapter;
use p4_agent_core::event_broker::RetainedEventBroker;
use p4_llamacpp_staged_adapter::v2::{
    ResourceStorageSnapshot, RuntimeResourceProbe, RuntimeResourceSnapshot,
};
use p4_protocol::event::Endpoint;
use std::sync::Arc;

type Factory = fn(
    Endpoint,
    usize,
    usize,
    usize,
    usize,
    RuntimeResourceProbe,
) -> Result<Arc<dyn RetainedNodeAdapter>, String>;

const FACTORIES: &[(&str, Factory)] = &[
    #[cfg(test)]
    ("neutral-lifecycle", neutral::create),
    (
        "llamacpp",
        |endpoint, input, output, retained, bytes, probe| {
            p4_llamacpp_staged_adapter::v2::RetainedLlamaNodeAdapter::new_with_runtime_resource_probe(
            endpoint,input,output,retained,bytes,probe)
            .map(|adapter| Arc::new(adapter) as Arc<dyn RetainedNodeAdapter>)
            .map_err(|error|format!("invalid adapter storage: {error:?}"))
        },
    ),
    #[cfg(feature = "hf-transformers")]
    (
        "hf-transformers",
        |endpoint, input, output, retained, bytes, _probe| {
            p4_hf_adapter::HfNodeAdapter::new(endpoint, input, output, retained, bytes)
                .map(|adapter| Arc::new(adapter) as Arc<dyn RetainedNodeAdapter>)
        },
    ),
];

pub(super) fn kinds() -> Vec<&'static str> {
    FACTORIES.iter().map(|(kind, _)| *kind).collect()
}

pub(super) fn supports(kind: &str) -> bool {
    FACTORIES.iter().any(|(name, _)| *name == kind)
}

pub(super) fn runtime_resource_probe(
    broker: &Arc<RetainedEventBroker>,
    transport: &super::transport::Inspector,
) -> RuntimeResourceProbe {
    let broker = Arc::clone(broker);
    let transport = transport.clone();
    RuntimeResourceProbe::new(move || {
        let edge = broker.outbound_storage_snapshot();
        let edge_bytes = edge
            .byte_limit
            .ok_or("outbound edge retained byte limit is not configured")?;
        let receipt = transport.receipt_capacity_snapshot();
        Ok(RuntimeResourceSnapshot {
            edge: ResourceStorageSnapshot {
                count_limit: edge.capacity,
                retained_count: edge.retained_count,
                byte_limit: edge_bytes,
                retained_bytes: edge.retained_bytes,
            },
            receipt: ResourceStorageSnapshot {
                count_limit: receipt.limit_count,
                retained_count: receipt.retained_count,
                byte_limit: receipt.limit_bytes,
                retained_bytes: receipt.retained_bytes,
            },
        })
    })
}

pub(super) fn create(
    kind: &str,
    endpoint: Endpoint,
    input: usize,
    output: usize,
    retained: usize,
    bytes: usize,
    probe: RuntimeResourceProbe,
) -> Result<Arc<dyn RetainedNodeAdapter>, String> {
    let factory = FACTORIES
        .iter()
        .find(|(name, _)| *name == kind)
        .ok_or_else(|| format!("unsupported adapter kind {kind}"))?
        .1;
    factory(endpoint, input, output, retained, bytes, probe)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn probe() -> RuntimeResourceProbe {
        RuntimeResourceProbe::new(|| {
            let store = ResourceStorageSnapshot {
                count_limit: 8,
                retained_count: 0,
                byte_limit: 4096,
                retained_bytes: 0,
            };
            Ok(RuntimeResourceSnapshot {
                edge: store,
                receipt: store,
            })
        })
    }
    #[tokio::test]
    async fn advertised_factories_construct_the_real_retained_adapters() {
        let mut expected = vec!["neutral-lifecycle", "llamacpp"];
        if cfg!(feature = "hf-transformers") {
            expected.push("hf-transformers");
        }
        assert_eq!(kinds(), expected);
        for kind in expected {
            let endpoint = Endpoint::node("tcp://127.0.0.1:41990".parse().unwrap(), kind, 1);
            let adapter = create(kind, endpoint, 1, 1, 2, 4096, probe()).unwrap();
            assert_eq!(
                adapter
                    .completion_storage_snapshot()
                    .unwrap()
                    .retained_count,
                0
            );
            assert!(adapter.peek_retained_completion().is_none());
        }
        assert!(
            create(
                "unknown",
                Endpoint::node("tcp://127.0.0.1:41990".parse().unwrap(), "invalid", 1),
                1,
                1,
                2,
                4096,
                probe()
            )
            .is_err()
        );
        #[cfg(not(feature = "hf-transformers"))]
        assert!(
            create(
                "hf-transformers",
                Endpoint::node("tcp://127.0.0.1:41990".parse().unwrap(), "disabled", 1),
                1,
                1,
                2,
                4096,
                probe()
            )
            .is_err()
        );
    }
}

#[cfg(test)]
pub(super) mod neutral {
    use super::*;
    use p4_adapter::node_adapter::{
        AdapterLifecycleCompletion, CompletionFront, CompletionMailbox, CompletionPublisher,
        OwnedPoll, RetainedCompletion, RetainedOfferError, completion_mailbox_with_limits,
    };
    use p4_protocol::event::lifecycle::{LifecycleOperation, LifecycleStatus, ResourceState};
    use p4_protocol::event::{Event, EventClass};
    use serde::Deserialize;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};

    pub(crate) const LOAD: &str = "application/vnd.p4.test.neutral-load-v1+json";
    pub(crate) const UNLOAD: &str = "application/vnd.p4.test.neutral-unload-v1+json";
    pub(crate) const RESULT: &str = "application/vnd.p4.test.neutral-result-v1+json";

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Command {
        #[serde(default)]
        delay_ms: u64,
        #[serde(default)]
        fail: bool,
        #[serde(default)]
        unknown_resource: bool,
    }

    struct Adapter {
        endpoint: Endpoint,
        publisher: CompletionPublisher,
        mailbox: Arc<CompletionMailbox>,
        state: Arc<Mutex<String>>,
        busy: Arc<AtomicBool>,
        sequence: AtomicU64,
    }

    impl RetainedNodeAdapter for Adapter {
        fn try_offer_retained(
            &self,
            completion: RetainedCompletion,
        ) -> Result<(), RetainedOfferError> {
            let event = completion.event();
            if event.envelope.target != self.endpoint || event.validate().is_err() {
                return Err(RetainedOfferError::Closed(completion));
            }
            let operation = match event.envelope.payload_content_type.as_str() {
                LOAD => LifecycleOperation::Load,
                UNLOAD => LifecycleOperation::Unload,
                _ => return Err(RetainedOfferError::Closed(completion)),
            };
            let command: Command = match serde_json::from_slice(&event.payload) {
                Ok(command) => command,
                Err(_) => return Err(RetainedOfferError::Closed(completion)),
            };
            let target = match &event.envelope.source {
                Endpoint::Agent(address) => Endpoint::agent(address.clone()),
                _ => return Err(RetainedOfferError::Closed(completion)),
            };
            if self
                .busy
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return Err(RetainedOfferError::Full(completion));
            }
            *self.state.lock().unwrap_or_else(|error| error.into_inner()) = match operation {
                LifecycleOperation::Load => "loading".into(),
                LifecycleOperation::Unload => "unloading".into(),
            };
            let number = self.sequence.fetch_add(1, Ordering::Relaxed);
            let (node_id, node_generation) = match &self.endpoint {
                Endpoint::Node {
                    node, generation, ..
                } => (node.clone(), *generation),
                _ => unreachable!("neutral adapter endpoint is always a node"),
            };
            let endpoint = self.endpoint.clone();
            let publisher = self.publisher.clone();
            let state = Arc::clone(&self.state);
            let busy = Arc::clone(&self.busy);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(command.delay_ms));
                let resource_state = if command.unknown_resource {
                    ResourceState::Unknown
                } else {
                    match operation {
                        LifecycleOperation::Load => ResourceState::Present,
                        LifecycleOperation::Unload => ResourceState::Absent,
                    }
                };
                let status = if command.fail {
                    LifecycleStatus::Failed
                } else {
                    LifecycleStatus::Succeeded
                };
                let outcome = AdapterLifecycleCompletion {
                    operation,
                    status,
                    resource_state,
                    first_error: command.fail.then(|| "neutral requested failure".into()),
                    cleanup_error: None,
                };
                let mut envelope = completion.event().envelope.next(
                    format!("neutral-lifecycle:{node_id}:{node_generation}:{number}"),
                    endpoint,
                    target,
                    EventClass::Control,
                    number,
                    RESULT,
                );
                envelope.adapter_kind = Some("neutral-lifecycle".into());
                let mut output = Event {
                    envelope,
                    payload: serde_json::to_vec(&outcome).expect("neutral outcome encodes"),
                };
                *state.lock().unwrap_or_else(|error| error.into_inner()) = match operation {
                    LifecycleOperation::Load if status == LifecycleStatus::Succeeded => {
                        "loaded".into()
                    }
                    LifecycleOperation::Unload if status == LifecycleStatus::Succeeded => {
                        "unloaded".into()
                    }
                    _ => "failed".into(),
                };
                busy.store(false, Ordering::Release);
                // Terminal publication is the authority that the command is no
                // longer retained by the node-side delivery path.
                completion.retire();
                loop {
                    match publisher.try_publish_owned(output) {
                        Ok(()) => break,
                        Err(p4_adapter::node_adapter::PublishError::Full(returned)) => {
                            output = returned;
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                        Err(_) => break,
                    }
                }
            });
            Ok(())
        }

        fn peek_retained_completion(&self) -> Option<CompletionFront> {
            self.mailbox.peek_owned_front()
        }

        fn try_take_retained_matching(&self, expected: &CompletionFront) -> OwnedPoll {
            self.mailbox.try_take_owned_matching(expected)
        }

        fn poll_take_retained(&self, context: &mut Context<'_>) -> Poll<OwnedPoll> {
            self.mailbox.poll_take_owned(context)
        }

        fn snapshot(&self) -> String {
            self.state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        }

        fn completion_storage_snapshot(
            &self,
        ) -> Option<p4_adapter::node_adapter::CompletionStorageSnapshot> {
            Some(self.mailbox.storage_snapshot())
        }

        fn decode_lifecycle_completion(
            &self,
            operation: LifecycleOperation,
            event: &Event,
        ) -> Result<AdapterLifecycleCompletion, String> {
            if event.envelope.payload_content_type != RESULT {
                return Err("neutral lifecycle result content type does not match".into());
            }
            let completion: AdapterLifecycleCompletion =
                serde_json::from_slice(&event.payload).map_err(|error| error.to_string())?;
            if completion.operation != operation {
                return Err("neutral lifecycle result operation does not match".into());
            }
            completion.validate().map_err(str::to_owned)?;
            Ok(completion)
        }
    }

    pub(super) fn create(
        endpoint: Endpoint,
        _input: usize,
        completion: usize,
        retained: usize,
        bytes: usize,
        _probe: RuntimeResourceProbe,
    ) -> Result<Arc<dyn RetainedNodeAdapter>, String> {
        let (publisher, mailbox) = completion_mailbox_with_limits(completion, retained, bytes)
            .map_err(|error| format!("invalid neutral storage: {error:?}"))?;
        Ok(Arc::new(Adapter {
            endpoint,
            publisher,
            mailbox,
            state: Arc::new(Mutex::new("empty".into())),
            busy: Arc::new(AtomicBool::new(false)),
            sequence: AtomicU64::new(1),
        }))
    }
}

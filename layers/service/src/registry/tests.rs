use super::*;
use p4_adapter::{Distribution, EventSink, Work};

struct Stub(Distribution);

impl Adapter for Stub {
    fn distribution(&self) -> Distribution {
        self.0
    }
    fn start(&self, _: Work, _: &dyn EventSink) {}
}

#[test]
fn attaching_a_backend_is_one_registration() {
    // The claim this crate exists to make: adding llama.cpp or vLLM costs a
    // name and an Adapter implementation, and touches nothing else.
    let mut registry = Registry::new();
    registry.register_fn("llamacpp", |_| Arc::new(Stub(Distribution::Staged)));
    registry.register_fn("vllm", |_| Arc::new(Stub(Distribution::Internal)));

    assert!(registry.knows("llamacpp"));
    assert!(registry.knows("vllm"));
    assert_eq!(registry.kinds(), vec!["llamacpp", "vllm"]);
}

#[test]
fn a_built_adapter_carries_its_own_distribution() {
    let mut registry = Registry::new();
    registry.register_fn("staged", |_| Arc::new(Stub(Distribution::Staged)));
    registry.register_fn("internal", |_| Arc::new(Stub(Distribution::Internal)));

    assert_eq!(
        registry.build("staged", "n0").unwrap().distribution(),
        Distribution::Staged
    );
    assert_eq!(
        registry.build("internal", "n0").unwrap().distribution(),
        Distribution::Internal
    );
}

#[test]
fn an_unknown_kind_builds_nothing() {
    // Reported rather than defaulted: a node created against a backend this
    // process does not have is a placement mistake worth hearing about.
    assert!(Registry::new().build("llamacpp", "n0").is_none());
}

#[test]
fn a_factory_is_told_which_node_it_is_building_for() {
    // A staged backend needs to know which position it plays, and the node id
    // is what names it.
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let mut registry = Registry::new();
    registry.register_fn("staged", move |node| {
        recorder.lock().unwrap().push(node.to_owned());
        Arc::new(Stub(Distribution::Staged))
    });

    registry.build("staged", "stage-2");
    assert_eq!(seen.lock().unwrap().as_slice(), ["stage-2"]);
}

#[test]
fn a_factory_may_refuse_a_node() {
    let mut registry = Registry::new();
    registry.register(
        "picky",
        Arc::new(|node| {
            (node == "allowed").then(|| Arc::new(Stub(Distribution::Staged)) as Arc<dyn Adapter>)
        }),
    );

    assert!(registry.build("picky", "allowed").is_some());
    assert!(registry.build("picky", "other").is_none());
}

#[test]
fn registering_a_name_twice_replaces_it() {
    let mut registry = Registry::new();
    registry.register_fn("backend", |_| Arc::new(Stub(Distribution::Staged)));
    registry.register_fn("backend", |_| Arc::new(Stub(Distribution::Internal)));

    assert_eq!(
        registry.build("backend", "n0").unwrap().distribution(),
        Distribution::Internal
    );
    assert_eq!(registry.kinds().len(), 1);
}

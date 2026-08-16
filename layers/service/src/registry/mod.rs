//! Where a concrete adapter is attached.
//!
//! This is the whole of what adding a backend costs. A factory is registered
//! under a name, a node names that adapter when it is created, and nothing
//! else in the system changes — not the envelope, not the queue, not the
//! routing, not the node. Attaching llama.cpp or vLLM is one registration and
//! one implementation of `Adapter`.
//!
//! Nothing here knows a backend. The names are strings the caller chooses.

use p4_adapter::Adapter;
use std::collections::HashMap;
use std::sync::Arc;

/// Builds an adapter for one node.
///
/// Takes the node's id so a backend that cares which stage it is playing can
/// be told, and returns `None` when this factory cannot serve that node — a
/// refusal the agent reports rather than papering over.
pub type Factory = Arc<dyn Fn(&str) -> Option<Arc<dyn Adapter>> + Send + Sync>;

#[derive(Default, Clone)]
pub struct Registry {
    factories: HashMap<String, Factory>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Attaches a backend under a name. Registering the same name twice
    /// replaces it, so a process can swap an implementation without a restart.
    pub fn register(&mut self, kind: impl Into<String>, factory: Factory) -> &mut Self {
        self.factories.insert(kind.into(), factory);
        self
    }

    /// Convenience for a backend that needs nothing but its node id.
    pub fn register_fn(
        &mut self,
        kind: impl Into<String>,
        build: impl Fn(&str) -> Arc<dyn Adapter> + Send + Sync + 'static,
    ) -> &mut Self {
        self.register(kind, Arc::new(move |node| Some(build(node))))
    }

    pub fn build(&self, kind: &str, node: &str) -> Option<Arc<dyn Adapter>> {
        self.factories.get(kind)?(node)
    }

    pub fn knows(&self, kind: &str) -> bool {
        self.factories.contains_key(kind)
    }

    /// Every attached backend, sorted so a report is stable between runs.
    pub fn kinds(&self) -> Vec<String> {
        let mut kinds: Vec<String> = self.factories.keys().cloned().collect();
        kinds.sort();
        kinds
    }
}

#[cfg(test)]
mod tests;

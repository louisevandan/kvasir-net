//! What the mock adapter remembers between messages.
//!
//! The same three registries a real adapter keeps -- created nodes, bound
//! deployments, and one admission gate per deployment -- so the state machine
//! P4 talks to is the production one. Only what sits under them is simulated.

use crate::domain::profile::Profile;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) agent_endpoint: String,
    pub(crate) adapter_id: String,
    pub(crate) nodes: Arc<RwLock<HashSet<String>>>,
    pub(crate) bindings: Arc<RwLock<HashMap<(String, String), Binding>>>,
    pub(crate) deployments: Arc<Deployments>,
}

#[derive(Clone, Debug)]
pub(crate) struct Binding {
    pub(crate) deployment_id: String,
    pub(crate) generation: u64,
}

/// Per-deployment profile and admission gate.
///
/// The gate is a real semaphore sized from the declared `max_sequences`, which
/// is what makes an overload run meaningful: arrivals above the declaration
/// have to wait or be refused by the same mechanism production uses, with no
/// GPU in the way to confound the result.
#[derive(Default)]
pub(crate) struct Deployments {
    profiles: RwLock<HashMap<String, Profile>>,
    gates: RwLock<HashMap<String, Arc<Semaphore>>>,
}

impl Deployments {
    pub(crate) fn declare(&self, deployment_id: &str, profile: Profile) {
        let permits = profile.max_sequences;
        self.profiles
            .write()
            .expect("profile registry lock")
            .insert(deployment_id.to_owned(), profile);
        self.gates
            .write()
            .expect("gate registry lock")
            .insert(deployment_id.to_owned(), Arc::new(Semaphore::new(permits)));
    }

    /// An execution against an unknown deployment gets the default profile
    /// rather than a panic: P4 is entitled to send it, and refusing it is the
    /// lifecycle's job, not the simulator's.
    pub(crate) fn profile(&self, deployment_id: &str) -> Profile {
        self.profiles
            .read()
            .expect("profile registry lock")
            .get(deployment_id)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn gate(&self, deployment_id: &str) -> Option<Arc<Semaphore>> {
        self.gates
            .read()
            .expect("gate registry lock")
            .get(deployment_id)
            .map(Arc::clone)
    }

    pub(crate) fn release(&self, deployment_id: &str) {
        self.profiles
            .write()
            .expect("profile registry lock")
            .remove(deployment_id);
        self.gates
            .write()
            .expect("gate registry lock")
            .remove(deployment_id);
    }
}

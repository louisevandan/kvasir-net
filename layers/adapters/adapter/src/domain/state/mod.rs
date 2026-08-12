//! Adapter registries.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) host: String,
    pub(crate) agent_endpoint: String,
    pub(crate) adapter_id: String,
    pub(crate) nodes: Arc<RwLock<HashSet<String>>>,
    pub(crate) bindings: Arc<RwLock<HashMap<(String, String), Binding>>>,
}

#[derive(Clone)]
pub(crate) struct Binding {
    pub(crate) deployment_id: String,
    pub(crate) generation: u64,
}

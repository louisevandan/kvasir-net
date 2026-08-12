//! Process-owned llama.cpp adapter configuration and bindings.

use std::collections::{HashMap, HashSet};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Clone)]
pub(crate) struct AdapterConfig {
    pub(crate) endpoint: String,
    pub(crate) model: String,
    pub(crate) agent_endpoint: String,
    pub(crate) adapter_id: String,
    pub(crate) nodes: Arc<RwLock<HashSet<String>>>,
    pub(crate) bindings: Arc<RwLock<HashMap<(String, String), u64>>>,
    pub(crate) active_upstreams: Arc<Mutex<HashMap<String, TcpStream>>>,
    pub(crate) max_inflight: usize,
    pub(crate) max_queued: usize,
    pub(crate) max_batch: usize,
    pub(crate) batch_linger_ms: usize,
}

impl AdapterConfig {
    pub(crate) fn cancel(&self, route_id: &str) {
        let stream = self
            .active_upstreams
            .lock()
            .ok()
            .and_then(|mut streams| streams.remove(route_id));
        if let Some(stream) = stream {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

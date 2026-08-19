//! Agent-local capability snapshots.
//!
//! Discovery is useful only if the load can prove that its snapshot was
//! produced by this agent for the same artifact and adapter. The registry is
//! deliberately service-owned; the agent core still carries the snapshot id
//! opaquely.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

const MAX_CAPABILITIES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capability {
    pub artifact: String,
    pub adapter: String,
    pub profile: String,
    pub expires_at: u64,
}

#[derive(Clone, Default)]
pub struct CapabilityRegistry {
    entries: Arc<RwLock<HashMap<String, Capability>>>,
}

impl CapabilityRegistry {
    pub fn insert(&self, id: String, capability: Capability) {
        let mut entries = self.entries.write().expect("capability registry lock");
        let now = unix_ms();
        entries.retain(|_, entry| entry.expires_at > now);
        if entries.len() >= MAX_CAPABILITIES
            && !entries.contains_key(&id)
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(key, _)| key.clone())
        {
            entries.remove(&oldest);
        }
        entries.insert(id, capability);
    }

    pub fn matches(&self, id: &str, artifact: &str, expires_at: u64) -> bool {
        self.entries
            .read()
            .expect("capability registry lock")
            .get(id)
            .is_some_and(|entry| {
                entry.artifact == artifact
                    && entry.expires_at == expires_at
                    && entry.expires_at > unix_ms()
            })
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

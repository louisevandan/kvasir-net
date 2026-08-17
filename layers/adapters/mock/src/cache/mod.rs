//! The four cache verbs, against state this mock is holding.
//!
//! Apart from the rest because it answers a different question: not what a
//! backend costs, but what it does with a conversation asked to be put away and
//! brought back. Each verb is refused rather than guessed at when it makes no
//! sense — restoring something never persisted, or forking from nothing, is a
//! caller error, and inventing an empty cache for it would let a branch
//! continue from a conversation that does not exist.

use super::{Mock, Progress, spin};
use p4_adapter::{Event, EventSink};

impl Mock {
    pub(super) fn cache(&self, cache: p4_adapter::Cache, events: &dyn EventSink) {
        spin(self.profile.trailing_hop);
        let refuse = |detail: String| {
            events.raise(Event::Failed {
                deployment: cache.deployment.clone(),
                sequence: Some(cache.sequence.clone()),
                detail,
            })
        };
        let (bytes, detail) = match &cache.action {
            p4_adapter::CacheAction::Persist => {
                // Progress is what there is to persist. A sequence that has
                // run further has more state, which is the whole reason an
                // operator wants it off the device.
                let Some(progress) = self
                    .produced
                    .lock()
                    .expect("produced")
                    .remove(&cache.sequence)
                else {
                    return refuse(format!("nothing resident for {}", cache.sequence));
                };
                let bytes =
                    u64::from(progress.lifetime + 1) * self.profile.reserved_per_stage.max(1_024);
                self.persisted
                    .lock()
                    .expect("persisted")
                    .insert(cache.sequence.clone(), bytes);
                (bytes, format!("persisted and freed {}", cache.sequence))
            }
            p4_adapter::CacheAction::Restore => {
                let Some(bytes) = self
                    .persisted
                    .lock()
                    .expect("persisted")
                    .remove(&cache.sequence)
                else {
                    return refuse(format!("nothing persisted for {}", cache.sequence));
                };
                // The size is what the copy was, so the progress it stood for
                // comes back with it — a restore that forgot how far the
                // conversation had got would be a restore in name only.
                let lifetime =
                    (bytes / self.profile.reserved_per_stage.max(1_024)).saturating_sub(1) as u32;
                self.produced
                    .lock()
                    .expect("produced")
                    .insert(cache.sequence.clone(), Progress { turn: 0, lifetime });
                (bytes, format!("restored {}", cache.sequence))
            }
            p4_adapter::CacheAction::Fork { into } => {
                let mut persisted = self.persisted.lock().expect("persisted");
                let Some(bytes) = persisted.get(&cache.sequence).copied() else {
                    return refuse(format!("nothing persisted for {}", cache.sequence));
                };
                // Copied, never aliased. Two branches that shared state would
                // each corrupt the other the moment either continued.
                persisted.insert(into.clone(), bytes);
                (bytes, format!("forked {} into {into}", cache.sequence))
            }
            p4_adapter::CacheAction::Discard => {
                let removed = self
                    .persisted
                    .lock()
                    .expect("persisted")
                    .remove(&cache.sequence);
                if removed.is_none() {
                    return refuse(format!("nothing persisted for {}", cache.sequence));
                }
                (0, format!("discarded {}", cache.sequence))
            }
        };
        events.raise(Event::Cached {
            deployment: cache.deployment.clone(),
            sequence: cache.subject().clone(),
            bytes,
            detail,
        });
    }
}

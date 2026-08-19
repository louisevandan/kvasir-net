//! Pure policy decisions for the OUTER connection and persisted KV cache.
//!
//! This module deliberately has no clock, I/O, scheduler, adapter, or wire
//! dependency. Callers provide the heartbeat event or current time and apply
//! the returned decision at their own boundary.

pub const DEFAULT_CACHE_RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeartbeatState {
    pub generation: u64,
    pub misses: u32,
    pub connected: bool,
}

impl HeartbeatState {
    pub const fn new() -> Self {
        Self {
            generation: 0,
            misses: 0,
            connected: false,
        }
    }
}

impl Default for HeartbeatState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeartbeatInput {
    Reconnect { generation: u64 },
    Ack { generation: u64 },
    Miss { generation: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeartbeatTransition {
    Reconnected,
    Healthy,
    Missed { misses: u32 },
    Disconnected,
    IgnoredStale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeartbeatDecision {
    pub state: HeartbeatState,
    pub transition: HeartbeatTransition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeartbeatPolicy {
    miss_threshold: u32,
}

impl HeartbeatPolicy {
    pub const fn new(miss_threshold: u32) -> Option<Self> {
        if miss_threshold == 0 {
            None
        } else {
            Some(Self { miss_threshold })
        }
    }

    pub const fn miss_threshold(self) -> u32 {
        self.miss_threshold
    }

    pub const fn apply(self, state: HeartbeatState, input: HeartbeatInput) -> HeartbeatDecision {
        match input {
            HeartbeatInput::Reconnect { generation } if generation > state.generation => {
                HeartbeatDecision {
                    state: HeartbeatState {
                        generation,
                        misses: 0,
                        connected: true,
                    },
                    transition: HeartbeatTransition::Reconnected,
                }
            }
            HeartbeatInput::Reconnect { .. } => HeartbeatDecision {
                state,
                transition: HeartbeatTransition::IgnoredStale,
            },
            HeartbeatInput::Ack { generation } if generation == state.generation => {
                HeartbeatDecision {
                    state: HeartbeatState {
                        misses: 0,
                        connected: true,
                        ..state
                    },
                    transition: HeartbeatTransition::Healthy,
                }
            }
            HeartbeatInput::Miss { generation } if generation == state.generation => {
                let misses = state.misses.saturating_add(1);
                let connected = misses < self.miss_threshold;
                HeartbeatDecision {
                    state: HeartbeatState {
                        misses,
                        connected,
                        ..state
                    },
                    transition: if connected {
                        HeartbeatTransition::Missed { misses }
                    } else {
                        HeartbeatTransition::Disconnected
                    },
                }
            }
            HeartbeatInput::Ack { .. } | HeartbeatInput::Miss { .. } => HeartbeatDecision {
                state,
                transition: HeartbeatTransition::IgnoredStale,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedCache {
    pub request_id: String,
    pub persisted_at_ms: u64,
    pub retain_until_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheDecision {
    Retain { retain_until_ms: u64 },
    Discard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheRetentionPolicy {
    retention_ms: u64,
}

impl CacheRetentionPolicy {
    pub const fn default() -> Self {
        Self {
            retention_ms: DEFAULT_CACHE_RETENTION_MS,
        }
    }

    pub const fn with_retention_ms(retention_ms: u64) -> Self {
        Self { retention_ms }
    }

    pub const fn retention_ms(self) -> u64 {
        self.retention_ms
    }

    pub const fn retain_until(self, persisted_at_ms: u64) -> u64 {
        persisted_at_ms.saturating_add(self.retention_ms)
    }

    pub const fn decide(self, cache: &PersistedCache, now_ms: u64) -> CacheDecision {
        if now_ms >= cache.retain_until_ms {
            CacheDecision::Discard
        } else {
            CacheDecision::Retain {
                retain_until_ms: cache.retain_until_ms,
            }
        }
    }

    pub fn discard_candidates(self, caches: &[PersistedCache], now_ms: u64) -> Vec<String> {
        let mut candidates = caches
            .iter()
            .filter(|cache| matches!(self.decide(cache, now_ms), CacheDecision::Discard))
            .map(|cache| cache.request_id.clone())
            .collect::<Vec<_>>();
        candidates.sort_unstable();
        candidates.dedup();
        candidates
    }
}

impl Default for CacheRetentionPolicy {
    fn default() -> Self {
        Self {
            retention_ms: DEFAULT_CACHE_RETENTION_MS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_generation_fences_old_heartbeat_events() {
        let policy = HeartbeatPolicy::new(3).unwrap();
        let connected = policy
            .apply(
                HeartbeatState::default(),
                HeartbeatInput::Reconnect { generation: 4 },
            )
            .state;
        let decision = policy.apply(connected, HeartbeatInput::Miss { generation: 3 });

        assert_eq!(decision.state, connected);
        assert_eq!(decision.transition, HeartbeatTransition::IgnoredStale);

        let future = policy.apply(connected, HeartbeatInput::Ack { generation: 5 });
        assert_eq!(future.state, connected);
        assert_eq!(future.transition, HeartbeatTransition::IgnoredStale);
    }

    #[test]
    fn miss_threshold_disconnects_only_at_the_threshold() {
        let policy = HeartbeatPolicy::new(2).unwrap();
        let first = policy.apply(
            HeartbeatState {
                generation: 8,
                misses: 0,
                connected: true,
            },
            HeartbeatInput::Miss { generation: 8 },
        );
        let second = policy.apply(first.state, HeartbeatInput::Miss { generation: 8 });

        assert_eq!(first.transition, HeartbeatTransition::Missed { misses: 1 });
        assert!(first.state.connected);
        assert_eq!(second.transition, HeartbeatTransition::Disconnected);
        assert!(!second.state.connected);
    }

    #[test]
    fn retention_uses_inclusive_retain_until_boundary() {
        let policy = CacheRetentionPolicy::default();
        let persisted_at_ms = 10_000;
        let retain_until_ms = policy.retain_until(persisted_at_ms);
        let cache = PersistedCache {
            request_id: "request-1".into(),
            persisted_at_ms,
            retain_until_ms,
        };

        assert_eq!(
            policy.decide(&cache, retain_until_ms - 1),
            CacheDecision::Retain { retain_until_ms }
        );
        assert_eq!(
            policy.decide(&cache, retain_until_ms),
            CacheDecision::Discard
        );
    }

    #[test]
    fn discard_candidates_are_deterministic_and_idempotent() {
        let policy = CacheRetentionPolicy::with_retention_ms(100);
        let caches = vec![
            PersistedCache {
                request_id: "expired-b".into(),
                persisted_at_ms: 0,
                retain_until_ms: 100,
            },
            PersistedCache {
                request_id: "live".into(),
                persisted_at_ms: 100,
                retain_until_ms: 200,
            },
            PersistedCache {
                request_id: "expired-a".into(),
                persisted_at_ms: 0,
                retain_until_ms: 100,
            },
            PersistedCache {
                request_id: "expired-a".into(),
                persisted_at_ms: 0,
                retain_until_ms: 100,
            },
        ];

        let first = policy.discard_candidates(&caches, 100);
        let second = policy.discard_candidates(&caches, 100);

        assert_eq!(first, vec!["expired-a", "expired-b"]);
        assert_eq!(second, first);
    }
}

//! `node_spec` is opaque controller data. The Agent reads exactly one field
//! from it and ignores the rest, so backend-specific placement never becomes
//! Agent policy.

use serde_json::Value;

pub(crate) const DEFAULT_MAX_INFLIGHT: u32 = 1;
pub(crate) const MAX_INFLIGHT_RANGE: std::ops::RangeInclusive<u32> = 1..=1024;

/// Concurrency sizing for a slot. Out-of-range or absent values fall back to
/// the conservative default; sizing never affects lifecycle exclusivity.
pub(crate) fn max_inflight(node_spec: &str) -> u32 {
    serde_json::from_str::<Value>(node_spec)
        .ok()
        .and_then(|value| value.get("p4_max_inflight").and_then(Value::as_u64))
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| MAX_INFLIGHT_RANGE.contains(value))
        .unwrap_or(DEFAULT_MAX_INFLIGHT)
}

#[cfg(test)]
mod tests;

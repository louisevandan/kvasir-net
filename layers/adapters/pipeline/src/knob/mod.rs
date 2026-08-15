//! Environment knob reading, with the pre-rename names still honoured.
//!
//! The crate was `p4-adapter` and its knobs were `P4_ADAPTER_*`. Those names
//! are an operational contract with whoever runs a benchmark, so dropping them
//! outright would not fail — it would silently fall back to the default and
//! quietly measure something else. Each knob therefore answers to both names
//! and says so on stderr when the retired one supplies the value.

use std::env;

/// Name resolution split from the environment so the fallback can be tested
/// without mutating process state.
fn resolve(suffix: &str, lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
    if let Some(value) = lookup(&format!("P4_PIPELINE_{suffix}")) {
        return Some(value);
    }
    let retired = format!("P4_ADAPTER_{suffix}");
    let value = lookup(&retired)?;
    eprintln!("P4_PIPELINE_RETIRED_KNOB name={retired} use=P4_PIPELINE_{suffix}");
    Some(value)
}

/// Reads `P4_PIPELINE_{suffix}`, falling back to the retired
/// `P4_ADAPTER_{suffix}`. Callers own the parsing and the range they accept.
pub(crate) fn read(suffix: &str) -> Option<String> {
    resolve(suffix, |name| env::var(name).ok())
}

/// A knob whose accepted range is `1..=4096`. Out-of-range and unparsable
/// values take the default rather than clamping, so a typo does not quietly
/// become a working configuration.
pub(crate) fn limit(suffix: &str, default: usize) -> usize {
    parse(read(suffix), default, |value| (1..=4096).contains(value))
}

/// A knob whose accepted range is `0..=4096`, for knobs where zero disables
/// the behaviour rather than being invalid.
pub(crate) fn nonnegative_limit(suffix: &str, default: usize) -> usize {
    parse(read(suffix), default, |value| *value <= 4096)
}

fn parse(raw: Option<String>, default: usize, accept: impl Fn(&usize) -> bool) -> usize {
    raw.and_then(|value| value.parse::<usize>().ok())
        .filter(accept)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests;

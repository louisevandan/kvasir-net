//! Choosing which queued sequences go into the next hop.
//!
//! This is the batching decision, and it sits here — next to the node, under
//! the agent's queue — rather than upstream. A gate further from the work was
//! measured reporting a limit the arrivals disagreed with, so nothing above
//! this point decides a window.
//!
//! Pure: queued items and a ceiling in, a window out.

use p4_protocol::QueueClass;

/// One queued piece of work waiting for a hop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    pub route: String,
    pub lane: QueueClass,
    pub deadline_unix_ms: u64,
}

/// What the next hop will carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub lane: QueueClass,
    pub items: Vec<Waiting>,
}

impl Window {
    pub fn width(&self) -> usize {
        self.items.len()
    }
}

/// Composes the next hop from what is waiting.
///
/// Decode goes first when anything decode is ready. A decode lap belongs to a
/// request that already holds KV on every node of its chain, and prefill is
/// the long phase — letting a fresh prefill in ahead of a ready lap makes an
/// in-flight request wait behind work that has not started.
///
/// A window never mixes lanes. Prefill and decode are different passes over
/// the model and a backend batches them separately.
pub fn compose(waiting: &[Waiting], ceiling: usize, now_unix_ms: u64) -> Option<Window> {
    if ceiling == 0 {
        return None;
    }
    let live: Vec<&Waiting> = waiting
        .iter()
        .filter(|item| !expired(item, now_unix_ms))
        .collect();
    let lane = if live.iter().any(|item| item.lane == QueueClass::Decode) {
        QueueClass::Decode
    } else {
        live.first()?.lane
    };
    let items: Vec<Waiting> = live
        .into_iter()
        .filter(|item| item.lane == lane)
        .take(ceiling)
        .cloned()
        .collect();
    (!items.is_empty()).then_some(Window { lane, items })
}

/// Which queued items the deadline has already passed for.
///
/// Separate from composing because they are answered separately: expired work
/// gets an error rather than being silently dropped, and that reply is a
/// different message from the hop.
pub fn expired_items(waiting: &[Waiting], now_unix_ms: u64) -> Vec<Waiting> {
    waiting
        .iter()
        .filter(|item| expired(item, now_unix_ms))
        .cloned()
        .collect()
}

fn expired(item: &Waiting, now_unix_ms: u64) -> bool {
    item.deadline_unix_ms != 0 && now_unix_ms > item.deadline_unix_ms
}

#[cfg(test)]
mod tests;

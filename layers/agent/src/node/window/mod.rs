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
/// Decode goes first **once the deployment is full**. A decode lap belongs to a
/// request that already holds KV on every node of its chain, and prefill is the
/// long phase, so letting a fresh prefill in ahead of a ready lap makes an
/// in-flight request wait behind work that has not started.
///
/// While there is still room, prefill goes first instead. Preferring decode
/// unconditionally was a veto rather than a preference: one sequence decoding
/// always has a lap ready, so a burst of arrivals behind it never got in, and a
/// node with a ceiling of sixty-four admitted them roughly one every six
/// seconds — never reaching the width it had declared, with both cards mostly
/// idle. The main queue had already learned this and bounds its own preference
/// every sixteenth take; this one had not.
///
/// Room is counted from the decode items themselves. Between hops every live
/// sequence has exactly one lap waiting, so the number of them *is* how many
/// the deployment is currently carrying.
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
    let carrying = live
        .iter()
        .filter(|item| item.lane == QueueClass::Decode)
        .count();
    let admits = live.iter().any(|item| item.lane == QueueClass::Prefill);
    let lane = if admits && carrying < ceiling {
        QueueClass::Prefill
    } else if carrying > 0 {
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

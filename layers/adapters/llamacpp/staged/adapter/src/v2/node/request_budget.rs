//! Admission storage, independent of resident KV and transport credits.
//! The claim follows immutable request provenance, including a late effect's
//! shared input, and retires only after the last input owner drops its data.
use super::state::RequestInput;
use crate::v2::InferenceCommand;
use p4_adapter::node_adapter::retained_event_bytes;
use p4_protocol::event::Event;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct RequestCost {
    pub requests: usize,
    pub bytes: usize,
    pub prompt_tokens: usize,
    pub output_tokens: usize,
}

impl RequestCost {
    fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            requests: self.requests.checked_add(other.requests)?,
            bytes: self.bytes.checked_add(other.bytes)?,
            prompt_tokens: self.prompt_tokens.checked_add(other.prompt_tokens)?,
            output_tokens: self.output_tokens.checked_add(other.output_tokens)?,
        })
    }

    fn fits(self, limit: Self) -> bool {
        self.requests <= limit.requests && self.bytes <= limit.bytes
            && self.prompt_tokens <= limit.prompt_tokens
            && self.output_tokens <= limit.output_tokens
    }

    /// Conservative retained footprint: count distinct input allocations and
    /// their capacities. Include inline/Arc and request-map/pending bookkeeping.
    /// Native tokenizer temporaries, KV, subsequent effects and broker receipts
    /// are different stores; this is not a process RSS or B3 return-byte bound.
    pub(crate) fn input(command: &InferenceCommand, event: &Event, reply: &String)
        -> Result<Self, String>
    {
        let mut bytes = retained_event_bytes(event).map_err(|_| "request storage cost overflow")?;
        let token_bytes = command.tokens.capacity().checked_mul(std::mem::size_of::<i32>())
            .ok_or("request storage cost overflow")?;
        let key_bytes = command.session_id.len().checked_add(command.request_id.len())
            .and_then(|n| n.checked_add(64)).and_then(|n| n.checked_mul(4))
            .ok_or("request storage cost overflow")?;
        for value in [std::mem::size_of::<RequestInput>(), 256, key_bytes, token_bytes,
            command.session_id.capacity(), command.request_id.capacity(),
            command.options.capacity(), command.session_key.as_ref().map_or(0, String::capacity),
            command.prompt.as_ref().map_or(0, String::capacity), reply.capacity()]
        {
            bytes = bytes.checked_add(value).ok_or("request storage cost overflow")?;
        }
        Ok(Self { requests: 1, bytes, prompt_tokens: command.tokens.len(),
            output_tokens: command.max_tokens as usize })
    }
}

#[derive(Debug)]
struct Account {
    limit: RequestCost,
    used: RequestCost,
}

#[derive(Clone, Debug)]
pub(crate) struct RequestBudget(Arc<Mutex<Account>>);

impl Default for RequestBudget {
    fn default() -> Self {
        // Pending + active + input retained by effects, not just resident slots.
        // These ceilings do not increase the sequence or fragment window.
        Self::new(RequestCost {
            requests: 4096, bytes: 512 * 1024 * 1024,
            prompt_tokens: 16 * 1024 * 1024, output_tokens: 16 * 1024 * 1024,
        })
    }
}

impl RequestBudget {
    pub(crate) fn new(limit: RequestCost) -> Self {
        Self(Arc::new(Mutex::new(Account { limit, used: RequestCost::default() })))
    }

    pub(crate) fn used(&self) -> RequestCost {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).used
    }

    pub(crate) fn reserve(&self, cost: RequestCost) -> Result<RequestReservation, String> {
        let mut account = self.0.lock().map_err(|_| "request storage budget poisoned")?;
        let next = account.used.checked_add(cost).ok_or("request storage budget overflow")?;
        if !next.fits(account.limit) {
            return Err(format!("request storage budget exhausted: held={:?}, additional={cost:?}, limit={:?}",
                account.used, account.limit));
        }
        account.used = next;
        Ok(RequestReservation(Arc::new(Claim { account: self.clone(), cost })))
    }
}

/// Clone shares one claim, just as RequestState shares one immutable input.
/// It cannot authorize a second production allocation.
#[derive(Clone, Debug)]
pub(crate) struct RequestReservation(Arc<Claim>);

impl PartialEq for RequestReservation {
    fn eq(&self, other: &Self) -> bool { Arc::ptr_eq(&self.0, &other.0) }
}
impl Eq for RequestReservation {}

#[derive(Debug)]
struct Claim { account: RequestBudget, cost: RequestCost }

impl Drop for Claim {
    fn drop(&mut self) {
        let mut account = self.account.0.lock().unwrap_or_else(|p| p.into_inner());
        account.used.requests -= self.cost.requests;
        account.used.bytes -= self.cost.bytes;
        account.used.prompt_tokens -= self.cost.prompt_tokens;
        account.used.output_tokens -= self.cost.output_tokens;
    }
}

//! Per-native-slot ownership and the latest control receipt, without native or
//! transport calls. A caller installs a prepared rows candidate only after
//! successful execution. This is NOT physical execution deduplication, row
//! ordering, or a durable receipt store; those require separate contracts.

use crate::v2::{Phase, RowOwner};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(crate) const MAX_CONTROL_BYTES: usize = 1024 * 1024;
const MAX_RECEIPT_BYTES: usize = 64 * 1024 * 1024;
const MAX_WATERMARKS: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub load_generation: u64,
    pub session_id: String,
    pub sequence_key: String,
    pub sequence_id: u32,
    pub incarnation: u64,
}

impl Identity {
    pub fn from_owner(owner: &RowOwner) -> Self {
        Self {
            load_generation: owner.load_generation,
            session_id: owner.session_id.clone(),
            sequence_key: owner.sequence_key.clone(),
            sequence_id: owner.sequence_id,
            incarnation: owner.incarnation,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.load_generation == 0
            || self.incarnation == 0
            || self.session_id.is_empty()
            || self.sequence_key.is_empty()
        {
            return Err("stage ownership requires a nonzero load/incarnation and identity".into());
        }
        Ok(())
    }

    fn watermark_key(&self) -> (u64, String, u32) {
        (
            self.load_generation,
            self.session_id.clone(),
            self.sequence_id,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Receipt {
    operation_id: u64,
    request: Vec<u8>,
    response: Vec<u8>,
}

impl Receipt {
    fn bytes(&self) -> usize {
        self.request.len() + self.response.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Limits {
    control_bytes: usize,
    receipt_bytes: usize,
    watermarks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Active,
    Released,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Slot {
    identity: Identity,
    status: Status,
    // Preparing row ownership must not copy retained control payloads.
    receipt: Option<Arc<Receipt>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ControlCheck {
    New,
    Replay(Vec<u8>),
}

/// One control of a batch together with the ceiling its own contract can
/// prove for the response. The budget reserves this bound before the native
/// effect; `MAX_CONTROL_BYTES` is only the ceiling a bound may not exceed.
/// A RELEASE echoes its request, so its bound is the request length; a
/// SETTLE answers an identity echo plus at most one token per physical row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ControlBudget {
    pub identity: Identity,
    pub operation_id: u64,
    pub request: Vec<u8>,
    pub response_bound: usize,
}

enum BorrowedControlCheck<'a> {
    New,
    Replay(&'a [u8]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StageOwners {
    slots: BTreeMap<u32, Slot>,
    // Remember the last admitted incarnation for every load/session/slot even
    // when a different session temporarily occupies the same physical slot.
    // Admission is a high-water mark; release never lowers it.
    watermarks: BTreeMap<(u64, String, u32), u64>,
    receipt_bytes: usize,
    limits: Limits,
}

impl Default for StageOwners {
    fn default() -> Self {
        Self {
            slots: BTreeMap::new(),
            watermarks: BTreeMap::new(),
            receipt_bytes: 0,
            limits: Limits {
                control_bytes: MAX_CONTROL_BYTES,
                receipt_bytes: MAX_RECEIPT_BYTES,
                watermarks: MAX_WATERMARKS,
            },
        }
    }
}

impl StageOwners {
    /// Released slots retain receipts and watermarks, but no live native KV.
    pub fn active_slots(&self) -> usize {
        self.slots
            .values()
            .filter(|slot| slot.status == Status::Active)
            .count()
    }

    pub fn prepare_rows(
        &self,
        generation: u64,
        capacity: u32,
        rows: &[&RowOwner],
    ) -> Result<Self, String> {
        if generation == 0 || capacity == 0 || rows.is_empty() {
            return Err("stage rows require a loaded capacity and nonempty ownership".into());
        }
        let mut groups = BTreeMap::<u32, Identity>::new();
        let mut new_slots = BTreeMap::<u32, Identity>::new();
        for row in rows {
            let identity = Identity::from_owner(row);
            identity.validate()?;
            if identity.load_generation != generation || identity.sequence_id >= capacity {
                return Err("stage row belongs to a stale load or an out-of-range slot".into());
            }
            if let Some(first) = groups.get(&identity.sequence_id) {
                if first != &identity {
                    return Err("one physical slot has multiple row owners in a batch".into());
                }
                if new_slots.contains_key(&identity.sequence_id) && row.phase != Phase::Prefill {
                    return Err("new stage ownership must start with a prefill fragment".into());
                }
                continue;
            }
            match self.slots.get(&identity.sequence_id) {
                Some(slot) if slot.status == Status::Active => {
                    if slot.identity != identity {
                        return Err("stage slot is still held by a different owner".into());
                    }
                }
                _ => {
                    if row.phase != Phase::Prefill || row.position != 0 {
                        return Err(
                            "new stage ownership must begin at prefill position zero".into()
                        );
                    }
                    if self
                        .watermarks
                        .get(&identity.watermark_key())
                        .is_some_and(|latest| identity.incarnation <= *latest)
                    {
                        return Err("stage row reuses a retired incarnation".into());
                    }
                    new_slots.insert(identity.sequence_id, identity.clone());
                }
            }
            groups.insert(identity.sequence_id, identity);
        }
        // Every row is checked before constructing the externally installable
        // candidate. An invalid later owner cannot claim an earlier slot.
        let mut candidate = self.clone();
        for (id, identity) in new_slots {
            if !candidate.watermarks.contains_key(&identity.watermark_key())
                && candidate.watermarks.len() >= candidate.limits.watermarks
            {
                return Err("stage ownership watermark budget is exhausted".into());
            }
            candidate
                .watermarks
                .insert(identity.watermark_key(), identity.incarnation);
            if let Some(receipt) = candidate
                .slots
                .get(&id)
                .and_then(|slot| slot.receipt.as_ref())
            {
                candidate.receipt_bytes -= receipt.bytes();
            }
            candidate.slots.insert(
                id,
                Slot {
                    identity,
                    status: Status::Active,
                    receipt: None,
                },
            );
        }
        Ok(candidate)
    }

    /// Can be called before native execution with the expected response bound.
    /// If the actual response later exceeds it, the caller must fence the
    /// already-executed native operation; an error is not rollback evidence.
    pub fn validate_control_sizes(request_len: usize, response_len: usize) -> Result<(), String> {
        if request_len > MAX_CONTROL_BYTES || response_len > MAX_CONTROL_BYTES {
            return Err("stage control request or response exceeds the receipt limit".into());
        }
        Ok(())
    }

    fn validate_sizes(&self, request_len: usize, response_len: usize) -> Result<(), String> {
        Self::validate_control_sizes(request_len, response_len)?;
        if request_len > self.limits.control_bytes || response_len > self.limits.control_bytes {
            return Err("stage control exceeds its configured receipt limit".into());
        }
        Ok(())
    }

    fn replacement_bytes(
        &self,
        slot: &Slot,
        request_len: usize,
        response_len: usize,
    ) -> Result<usize, String> {
        let old = slot.receipt.as_ref().map_or(0, |receipt| receipt.bytes());
        let bytes = self
            .receipt_bytes
            .checked_sub(old)
            .and_then(|retained| retained.checked_add(request_len))
            .and_then(|retained| retained.checked_add(response_len))
            .ok_or_else(|| "stage control receipt accounting overflow".to_owned())?;
        if bytes > self.limits.receipt_bytes {
            return Err(format!(
                "stage control total receipt budget is exhausted: slot {}, retained {} B, \
                 reclaimed {} B, request {} B, response bound {} B, need {} B, limit {} B",
                slot.identity.sequence_id,
                self.receipt_bytes,
                old,
                request_len,
                response_len,
                bytes,
                self.limits.receipt_bytes
            ));
        }
        Ok(bytes)
    }

    /// `request` must be the canonical command including its operation kind,
    /// not merely an untagged payload shared by different native operations.
    /// `response_bound` is the ceiling this one command can prove for its own
    /// response. It is what the budget reserves before the native effect, so
    /// a command whose answer is small no longer charges the global maximum.
    pub fn check_control(
        &self,
        identity: &Identity,
        operation_id: u64,
        request: &[u8],
        response_bound: usize,
    ) -> Result<ControlCheck, String> {
        Ok(
            match self.check_control_borrowed(identity, operation_id, request, response_bound)? {
                BorrowedControlCheck::New => ControlCheck::New,
                BorrowedControlCheck::Replay(response) => ControlCheck::Replay(response.to_vec()),
            },
        )
    }

    fn check_control_borrowed(
        &self,
        identity: &Identity,
        operation_id: u64,
        request: &[u8],
        response_bound: usize,
    ) -> Result<BorrowedControlCheck<'_>, String> {
        identity.validate()?;
        self.validate_sizes(request.len(), response_bound)?;
        if operation_id == 0 {
            return Err("stage control operation identity is zero".into());
        }
        let slot = self
            .slots
            .get(&identity.sequence_id)
            .filter(|slot| slot.identity == *identity)
            .ok_or_else(|| "stage control does not own its current slot".to_owned())?;
        if let Some(receipt) = &slot.receipt {
            if operation_id == receipt.operation_id {
                if receipt.request != request {
                    return Err(
                        "stage control operation identity conflicts with its request".into(),
                    );
                }
                return Ok(BorrowedControlCheck::Replay(&receipt.response));
            }
            if operation_id < receipt.operation_id {
                return Err("stage control operation is older than its latest receipt".into());
            }
        }
        if slot.status == Status::Released {
            return Err("released stage owner can only replay its final control receipt".into());
        }
        // New controls are executed serially by the owning worker. Reserve
        // room for this command's own proven response bound BEFORE native
        // execution, replacing this slot's old receipt rather than charging
        // both forever.
        self.replacement_bytes(slot, request.len(), response_bound)?;
        Ok(BorrowedControlCheck::New)
    }

    /// Validate all controls before any native effect in one command. This is
    /// a read-only budget check, not a reservation that survives other writes:
    /// the owning worker must execute the validated command without interleaving
    /// another command. Retained receipt payloads are borrowed, never copied.
    ///
    /// Every member reserves its own `response_bound`, so the batch width a
    /// stage accepts follows the responses its commands can actually return
    /// rather than the one-megabyte ceiling. The caller owes the proof that
    /// its command cannot answer with more than the bound it declares:
    /// `commit_control` charges the response that actually arrived, so a
    /// longer answer must be fenced before it reaches commit.
    pub fn validate_control_batch(&self, controls: &[ControlBudget]) -> Result<(), String> {
        let mut slots = BTreeSet::new();
        let mut reserved = self.receipt_bytes;
        let mut new_members = 0usize;
        for control in controls {
            if !slots.insert(control.identity.sequence_id) {
                return Err("stage control batch repeats a physical slot".into());
            }
            if matches!(
                self.check_control_borrowed(
                    &control.identity,
                    control.operation_id,
                    &control.request,
                    control.response_bound,
                )?,
                BorrowedControlCheck::Replay(_)
            ) {
                continue;
            }
            new_members += 1;
            let old = self.slots[&control.identity.sequence_id]
                .receipt
                .as_ref()
                .map_or(0, |receipt| receipt.bytes());
            let replacement = control
                .request
                .len()
                .checked_add(control.response_bound)
                .ok_or_else(|| "stage control batch receipt accounting overflow".to_owned())?;
            // A later shrinking receipt cannot fund an earlier growing one.
            // Sum only positive increases so every execution prefix fits,
            // independently of command order and actual response lengths.
            let increase = replacement.saturating_sub(old);
            reserved = reserved
                .checked_add(increase)
                .ok_or_else(|| "stage control batch receipt accounting overflow".to_owned())?;
        }
        if reserved > self.limits.receipt_bytes {
            return Err(format!(
                "stage control batch total receipt budget is exhausted: {} member(s), \
                 {} new, retained {} B, reserve {} B, need {} B, limit {} B",
                controls.len(),
                new_members,
                self.receipt_bytes,
                reserved - self.receipt_bytes,
                reserved,
                self.limits.receipt_bytes
            ));
        }
        Ok(())
    }

    /// Commits only a successful native effect. A failed check or oversized
    /// response leaves ownership and every prior receipt unchanged.
    pub fn commit_control(
        &mut self,
        identity: &Identity,
        operation_id: u64,
        request: &[u8],
        response: &[u8],
        released: bool,
    ) -> Result<(), String> {
        self.validate_sizes(request.len(), response.len())?;
        // Commit charges the response that actually arrived, never a bound.
        let check = self.check_control_borrowed(identity, operation_id, request, response.len())?;
        let status = if released {
            Status::Released
        } else {
            Status::Active
        };
        if let BorrowedControlCheck::Replay(previous) = check {
            let slot = self
                .slots
                .get(&identity.sequence_id)
                .expect("checked ownership");
            if previous != response || slot.status != status {
                return Err("replayed stage control changed its result or effect".into());
            }
            return Ok(());
        }
        let bytes = self.replacement_bytes(
            self.slots
                .get(&identity.sequence_id)
                .expect("checked ownership"),
            request.len(),
            response.len(),
        )?;
        let receipt = Arc::new(Receipt {
            operation_id,
            request: request.to_vec(),
            response: response.to_vec(),
        });
        let slot = self
            .slots
            .get_mut(&identity.sequence_id)
            .expect("checked ownership");
        slot.receipt = Some(receipt);
        slot.status = status;
        self.receipt_bytes = bytes;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn with_limits(
        control_bytes: usize,
        receipt_bytes: usize,
        watermarks: usize,
    ) -> Self {
        Self {
            limits: Limits {
                control_bytes,
                receipt_bytes,
                watermarks,
            },
            ..Self::default()
        }
    }
}

/// Every control charged the whole per-control ceiling before commands
/// declared their own response bound. Tests whose subject is something else
/// keep charging it, so only the tests about bounds carry bound arithmetic.
#[cfg(test)]
impl StageOwners {
    pub(crate) fn check_at_ceiling(
        &self,
        identity: &Identity,
        operation_id: u64,
        request: &[u8],
    ) -> Result<ControlCheck, String> {
        self.check_control(identity, operation_id, request, self.limits.control_bytes)
    }

    fn validate_batch_at_ceiling(
        &self,
        controls: &[(Identity, u64, Vec<u8>)],
    ) -> Result<(), String> {
        let budgets: Vec<_> = controls
            .iter()
            .map(|(identity, operation_id, request)| ControlBudget {
                identity: identity.clone(),
                operation_id: *operation_id,
                request: request.clone(),
                response_bound: self.limits.control_bytes,
            })
            .collect();
        self.validate_control_batch(&budgets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(session: &str, request: &str, slot: u32, incarnation: u64) -> RowOwner {
        RowOwner {
            load_generation: 1,
            incarnation,
            request_id: request.into(),
            sequence_key: format!("{session}\0{request}"),
            session_id: session.into(),
            reply: "reply".into(),
            sequence_id: slot,
            phase: Phase::Prefill,
            position: 0,
            max_tokens: 16,
            generated_tokens: 0,
            output: false,
            input_token: 7,
            speculative_id: 0,
            speculative_index: 0,
            speculative_count: 0,
            options: String::new(),
        }
    }

    fn active(owner: &RowOwner) -> StageOwners {
        StageOwners::default().prepare_rows(1, 4, &[owner]).unwrap()
    }

    fn release(owners: &mut StageOwners, owner: &RowOwner, operation: u64) {
        owners
            .commit_control(
                &Identity::from_owner(owner),
                operation,
                b"release",
                b"released",
                true,
            )
            .unwrap();
    }

    #[test]
    fn shutdown_counts_active_owners_but_not_released_receipts_or_watermarks() {
        let owner = row("shutdown", "request", 0, 1);
        let original = StageOwners::default();
        assert_eq!(original.active_slots(), 0);
        let mut owners = original.prepare_rows(1, 4, &[&owner]).unwrap();
        assert_eq!(
            original.active_slots(),
            0,
            "an uncommitted candidate owns nothing"
        );
        assert_eq!(owners.active_slots(), 1);
        release(&mut owners, &owner, 1);
        assert_eq!(owners.slots.len(), 1, "release retains its slot tombstone");
        assert_eq!(owners.watermarks.len(), 1);
        assert!(owners.slots[&0].receipt.is_some());
        assert!(owners.receipt_bytes > 0);
        assert_eq!(
            owners.active_slots(),
            0,
            "retry history is not live KV ownership"
        );
        let next = row("shutdown", "next-request", 0, 2);
        let reused = owners.prepare_rows(1, 4, &[&next]).unwrap();
        assert_eq!(reused.active_slots(), 1);
        assert_eq!(owners.active_slots(), 0);
    }

    #[test]
    fn preparation_is_an_uncommitted_candidate_and_bad_later_rows_are_atomic() {
        let owners = StageOwners::default();
        let first = row("s", "a", 0, 1);
        let second = row("s", "b", 1, 2);
        let candidate = owners.prepare_rows(1, 4, &[&first, &second]).unwrap();
        assert_eq!(owners, StageOwners::default());
        assert_eq!(candidate.slots.len(), 2);
        for bad_first in [false, true] {
            let mut invalid = second.clone();
            invalid.incarnation = 0;
            let rows = if bad_first {
                vec![&invalid, &first]
            } else {
                vec![&first, &invalid]
            };
            assert!(owners.prepare_rows(1, 4, &rows).is_err());
            assert_eq!(owners, StageOwners::default());
        }
    }

    #[test]
    fn active_owner_can_continue_but_not_be_replaced() {
        let first = row("s", "a", 0, 1);
        let owners = active(&first);
        let mut continued = first.clone();
        continued.phase = Phase::Decode;
        continued.position = 32;
        assert_eq!(owners.prepare_rows(1, 4, &[&continued]).unwrap(), owners);
        for replacement in [
            row("s", "a", 0, 2),
            row("s", "b", 0, 2),
            row("other", "a", 0, 1),
        ] {
            assert!(owners.prepare_rows(1, 4, &[&replacement]).is_err());
        }
    }

    #[test]
    fn one_batch_cannot_assign_two_owners_or_phases_to_a_new_slot() {
        let owners = StageOwners::default();
        let first = row("s", "a", 0, 1);
        let second = row("s", "b", 0, 2);
        assert!(owners.prepare_rows(1, 4, &[&first, &second]).is_err());
        assert!(owners.prepare_rows(1, 4, &[&second, &first]).is_err());
        let mut decode = first.clone();
        decode.phase = Phase::Decode;
        decode.position = 1;
        assert!(owners.prepare_rows(1, 4, &[&first, &decode]).is_err());
        assert!(owners.prepare_rows(1, 4, &[&decode]).is_err());
        let mut offset = first.clone();
        offset.position = 1;
        assert!(owners.prepare_rows(1, 4, &[&offset]).is_err());
        assert_eq!(owners, StageOwners::default());
    }

    #[test]
    fn released_slots_require_a_fresh_incarnation_and_reject_old_controls() {
        let first = row("s", "same", 0, 1);
        let mut owners = active(&first);
        let old = Identity::from_owner(&first);
        release(&mut owners, &first, 7);
        assert!(owners.prepare_rows(1, 4, &[&first]).is_err());
        let second = row("s", "same", 0, 2);
        owners = owners.prepare_rows(1, 4, &[&second]).unwrap();
        let before = owners.clone();
        for body in [b"release".as_slice(), b"settle".as_slice()] {
            assert!(owners.check_at_ceiling(&old, 7, body).is_err());
            assert!(owners.commit_control(&old, 7, body, b"old", true).is_err());
        }
        assert_eq!(owners, before);
        assert_eq!(
            owners
                .check_at_ceiling(&Identity::from_owner(&second), 1, b"settle")
                .unwrap(),
            ControlCheck::New
        );
    }

    #[test]
    fn session_switches_preserve_each_sessions_retired_watermark() {
        let first = row("a", "first", 0, 10);
        let mut owners = active(&first);
        release(&mut owners, &first, 1);
        let other = row("b", "other", 0, 1);
        owners = owners.prepare_rows(1, 4, &[&other]).unwrap();
        release(&mut owners, &other, 1);
        for incarnation in [1, 9, 10] {
            assert!(
                owners
                    .prepare_rows(1, 4, &[&row("a", "new", 0, incarnation)])
                    .is_err()
            );
        }
        let fresh = row("a", "new", 0, 11);
        owners = owners.prepare_rows(1, 4, &[&fresh]).unwrap();
        assert_eq!(owners.watermarks.get(&(1, "a".into(), 0)), Some(&11));
        assert_eq!(owners.watermarks.get(&(1, "b".into(), 0)), Some(&1));
    }

    #[test]
    fn the_latest_control_is_exactly_replayed_and_older_operations_are_rejected() {
        let owner = row("s", "r", 0, 1);
        let identity = Identity::from_owner(&owner);
        let mut owners = active(&owner);
        assert_eq!(
            owners.check_at_ceiling(&identity, 2, b"settle").unwrap(),
            ControlCheck::New
        );
        owners
            .commit_control(&identity, 2, b"settle", b"proposal", false)
            .unwrap();
        let settled = owners.clone();
        assert_eq!(
            owners.check_at_ceiling(&identity, 2, b"settle").unwrap(),
            ControlCheck::Replay(b"proposal".to_vec())
        );
        owners
            .commit_control(&identity, 2, b"settle", b"proposal", false)
            .unwrap();
        assert_eq!(owners, settled);
        assert!(owners.check_at_ceiling(&identity, 1, b"settle").is_err());
        assert!(owners.check_at_ceiling(&identity, 2, b"changed").is_err());
        assert!(
            owners
                .commit_control(&identity, 2, b"settle", b"different", false)
                .is_err()
        );
        assert!(
            owners
                .commit_control(&identity, 2, b"settle", b"proposal", true)
                .is_err()
        );
        assert_eq!(owners, settled);
        release(&mut owners, &owner, 3);
        let released = owners.clone();
        assert_eq!(
            owners.check_at_ceiling(&identity, 3, b"release").unwrap(),
            ControlCheck::Replay(b"released".to_vec())
        );
        owners
            .commit_control(&identity, 3, b"release", b"released", true)
            .unwrap();
        for operation in [1, 2, 4, u64::MAX] {
            assert!(
                owners
                    .check_at_ceiling(&identity, operation, b"release")
                    .is_err()
            );
        }
        assert_eq!(owners, released);
    }

    #[test]
    fn every_identity_axis_and_zero_operation_is_checked_for_controls() {
        let owner = row("s", "r", 0, 1);
        let identity = Identity::from_owner(&owner);
        let owners = active(&owner);
        let mut variants = vec![identity.clone(); 5];
        variants[0].load_generation += 1;
        variants[1].session_id = "different".into();
        variants[2].sequence_key = "different".into();
        variants[3].sequence_id += 1;
        variants[4].incarnation += 1;
        for invalid in variants {
            assert!(owners.check_at_ceiling(&invalid, 1, b"release").is_err());
        }
        assert!(owners.check_at_ceiling(&identity, 0, b"release").is_err());
    }

    #[test]
    fn receipt_limits_are_inclusive_and_refusals_preserve_previous_effects() {
        let owner = row("s", "r", 0, 1);
        let identity = Identity::from_owner(&owner);
        let mut owners = active(&owner);
        let limit = vec![5; MAX_CONTROL_BYTES];
        let excessive = vec![5; MAX_CONTROL_BYTES + 1];
        assert!(StageOwners::validate_control_sizes(limit.len(), limit.len()).is_ok());
        assert!(StageOwners::validate_control_sizes(excessive.len(), 0).is_err());
        assert!(StageOwners::validate_control_sizes(0, excessive.len()).is_err());
        owners
            .commit_control(&identity, 1, &limit, &limit, false)
            .unwrap();
        let before = owners.clone();
        assert!(owners.check_at_ceiling(&identity, 2, &excessive).is_err());
        assert!(
            owners
                .commit_control(&identity, 2, &limit, &excessive, true)
                .is_err()
        );
        assert!(
            owners
                .commit_control(&identity, 2, &excessive, &limit, true)
                .is_err()
        );
        assert_eq!(owners, before);
        assert_eq!(
            owners.check_at_ceiling(&identity, 1, &limit).unwrap(),
            ControlCheck::Replay(limit)
        );
        let candidate = owners.prepare_rows(1, 4, &[&owner]).unwrap();
        assert!(Arc::ptr_eq(
            owners.slots[&0].receipt.as_ref().unwrap(),
            candidate.slots[&0].receipt.as_ref().unwrap()
        ));
    }

    #[test]
    fn malformed_rows_do_not_claim_a_slot() {
        let owners = StageOwners::default();
        let owner = row("s", "r", 0, 1);
        let mut variants = vec![owner.clone(); 5];
        variants[0].load_generation = 0;
        variants[1].incarnation = 0;
        variants[2].session_id.clear();
        variants[3].sequence_key.clear();
        variants[4].sequence_id = 4;
        for invalid in variants {
            assert!(owners.prepare_rows(1, 4, &[&invalid]).is_err());
        }
        assert!(owners.prepare_rows(2, 4, &[&owner]).is_err());
        assert!(owners.prepare_rows(0, 4, &[&owner]).is_err());
        assert!(owners.prepare_rows(1, 0, &[&owner]).is_err());
        assert!(owners.prepare_rows(1, 4, &[]).is_err());
        assert_eq!(owners, StageOwners::default());
    }

    #[test]
    fn total_receipt_budget_is_checked_before_new_native_controls() {
        let a = row("s", "a", 0, 1);
        let b = row("s", "b", 1, 2);
        let aid = Identity::from_owner(&a);
        let bid = Identity::from_owner(&b);
        let mut owners = StageOwners::with_limits(8, 20, 4)
            .prepare_rows(1, 4, &[&a, &b])
            .unwrap();
        // First receipt uses 16 bytes. The second needs at least 1 request
        // byte + the full 8-byte response allowance before native execution.
        owners
            .commit_control(&aid, 1, b"12345678", b"12345678", false)
            .unwrap();
        assert_eq!(owners.receipt_bytes, 16);
        let before = owners.clone();
        assert!(owners.check_at_ceiling(&bid, 1, b"b").is_err());
        // The pre-native gate charges the bound the caller must prove, so a
        // command that can answer in three bytes is admitted at exactly the
        // limit while the same command claiming the whole ceiling is not.
        assert_eq!(
            owners.check_control(&bid, 1, b"b", 3).unwrap(),
            ControlCheck::New
        );
        assert!(owners.check_control(&bid, 1, b"b", 4).is_err());
        // Commit charges the response that actually arrived, so it refuses
        // only a receipt that truly does not fit, never a bound.
        assert!(
            owners
                .commit_control(&bid, 1, b"b", b"1234", false)
                .is_err()
        );
        assert_eq!(owners, before);
        // Replacing A's receipt deducts its old bytes before reservation.
        owners.commit_control(&aid, 2, b"a", b"a", false).unwrap();
        assert_eq!(owners.receipt_bytes, 2);
        assert_eq!(
            owners.check_at_ceiling(&bid, 1, b"b").unwrap(),
            ControlCheck::New
        );
        owners
            .commit_control(&bid, 1, b"b", b"12345678", false)
            .unwrap();
        assert_eq!(owners.receipt_bytes, 11);
    }

    #[test]
    fn control_batch_rejects_the_sum_even_when_each_control_fits_alone() {
        let a = row("s", "a", 0, 1);
        let b = row("s", "b", 1, 2);
        let controls = vec![
            (Identity::from_owner(&a), 1, b"a".to_vec()),
            (Identity::from_owner(&b), 1, b"b".to_vec()),
        ];
        let owners = StageOwners::with_limits(8, 17, 4)
            .prepare_rows(1, 4, &[&a, &b])
            .unwrap();
        for (identity, operation, request) in &controls {
            assert_eq!(
                owners
                    .check_at_ceiling(identity, *operation, request)
                    .unwrap(),
                ControlCheck::New
            );
        }
        let before = owners.clone();
        assert_eq!(
            owners.validate_batch_at_ceiling(&controls).unwrap_err(),
            "stage control batch total receipt budget is exhausted: 2 member(s), \
             2 new, retained 0 B, reserve 18 B, need 18 B, limit 17 B",
            "the refusal names the width and the arithmetic that produced it"
        );
        assert_eq!(owners, before);

        // The inclusive bound is the sum of both worst-case responses, not
        // the small responses that happen to be returned in a successful run.
        let mut fitting = StageOwners::with_limits(8, 18, 4)
            .prepare_rows(1, 4, &[&a, &b])
            .unwrap();
        fitting.validate_batch_at_ceiling(&controls).unwrap();
        for (identity, operation, request) in &controls {
            fitting
                .commit_control(identity, *operation, request, b"12345678", false)
                .unwrap();
        }
        assert_eq!(fitting.receipt_bytes, 18);
    }

    /// The 2026-09-09 pressure failure, at production limits and the resident
    /// width the scenario runs: 64 MiB of receipts divided by a 1 MiB
    /// per-control ceiling is 64 members, so a stage sitting at sequence
    /// capacity 256 could never release or settle its own owners. The bodies
    /// here are the real wire commands, and the bounds are the ones their
    /// contracts prove, not values chosen to make the sum fit.
    #[test]
    fn a_full_width_control_batch_fits_when_each_command_reserves_its_own_bound() {
        const CAPACITY: u32 = 256;
        let rows: Vec<_> = (0..CAPACITY)
            .map(|slot| row("session", &format!("request-{slot}"), slot, 1))
            .collect();
        let borrowed: Vec<_> = rows.iter().collect();
        let owners = StageOwners::default()
            .prepare_rows(1, CAPACITY, &borrowed)
            .unwrap();
        assert_eq!(owners.active_slots(), CAPACITY as usize);

        // A physical release is acknowledged by echoing its request.
        let releases: Vec<_> = rows
            .iter()
            .map(|owner| {
                let request = crate::v2::control_identity::release(
                    1,
                    "session",
                    &crate::v2::ReleaseSequence {
                        key: owner.sequence_key.clone(),
                        id: owner.sequence_id,
                        incarnation: owner.incarnation,
                        operation_id: 1,
                    },
                )
                .unwrap();
                ControlBudget {
                    identity: Identity::from_owner(owner),
                    operation_id: 1,
                    response_bound: request.len(),
                    request,
                }
            })
            .collect();
        owners.validate_control_batch(&releases).unwrap();

        // A settlement answers its identity echo, a count, and at most one
        // four-byte token per physical row.
        let settlements: Vec<_> = rows
            .iter()
            .map(|owner| {
                let (prefix, request) = crate::v2::control_identity::settlement(
                    1,
                    "session",
                    &crate::v2::SettlementSequence {
                        key: owner.sequence_key.clone(),
                        id: owner.sequence_id,
                        incarnation: owner.incarnation,
                        operation_id: 1,
                        retain_from: 3,
                        replay_tokens: Vec::new(),
                        replay_position: 0,
                        proposal: Vec::new(),
                    },
                )
                .unwrap();
                ControlBudget {
                    identity: Identity::from_owner(owner),
                    operation_id: 1,
                    response_bound: CAPACITY as usize * 4 + prefix.len() + 4,
                    request,
                }
            })
            .collect();
        owners.validate_control_batch(&settlements).unwrap();

        // The same batch, with every member claiming the ceiling instead of
        // its own bound, is the refusal the pressure run hit. Its width is
        // the number the run did not record.
        let at_ceiling: Vec<_> = releases
            .iter()
            .cloned()
            .map(|mut control| {
                control.response_bound = MAX_CONTROL_BYTES;
                control
            })
            .collect();
        let widest = (1..=at_ceiling.len())
            .take_while(|width| owners.validate_control_batch(&at_ceiling[..*width]).is_ok())
            .count();
        let reserved: usize = at_ceiling[..widest]
            .iter()
            .map(|control| control.request.len() + control.response_bound)
            .sum();
        assert_eq!(
            widest, 63,
            "the ceiling admits {widest} of {CAPACITY} members ({reserved} B of {MAX_RECEIPT_BYTES} B)"
        );
        let refusal = owners.validate_control_batch(&at_ceiling).unwrap_err();
        assert!(
            refusal.contains("256 member(s)")
                && refusal.contains("256 new")
                && refusal.contains("retained 0 B")
                && refusal.contains(&format!("limit {MAX_RECEIPT_BYTES} B")),
            "the refusal must carry the width and the arithmetic: {refusal}"
        );
    }

    #[test]
    fn control_batch_replays_reserve_nothing_and_replacements_reclaim_their_own_receipt() {
        let a = row("s", "a", 0, 1);
        let b = row("s", "b", 1, 2);
        let aid = Identity::from_owner(&a);
        let bid = Identity::from_owner(&b);
        let mut owners = StageOwners::with_limits(8, 25, 4)
            .prepare_rows(1, 4, &[&a, &b])
            .unwrap();
        owners
            .commit_control(&aid, 1, b"12345678", b"abcdefgh", true)
            .unwrap();
        let controls = vec![
            (aid.clone(), 1, b"12345678".to_vec()),
            (bid.clone(), 1, b"b".to_vec()),
        ];
        let before = owners.clone();
        owners.validate_batch_at_ceiling(&controls).unwrap();
        assert_eq!(owners, before);
        let BorrowedControlCheck::Replay(response) = owners
            .check_control_borrowed(&aid, 1, b"12345678", owners.limits.control_bytes)
            .unwrap()
        else {
            panic!("the released owner's exact latest receipt must be borrowed");
        };
        assert_eq!(
            response.as_ptr(),
            owners.slots[&0].receipt.as_ref().unwrap().response.as_ptr()
        );
        owners
            .commit_control(&bid, 1, b"b", b"12345678", false)
            .unwrap();
        assert_eq!(owners.receipt_bytes, 25);
        // B replaces its nine-byte receipt with the same worst-case size.
        // Neither an exact replay nor a replacement needs another allowance.
        owners
            .validate_batch_at_ceiling(&[(aid, 1, b"12345678".to_vec()), (bid, 2, b"c".to_vec())])
            .unwrap();

        // A short stored response is replayed as-is, not expanded to another
        // maximum-response allowance. Treating Replay as New would charge
        // seven nonexistent bytes and incorrectly reject this exact bound.
        let mut short = StageOwners::with_limits(8, 11, 4)
            .prepare_rows(1, 4, &[&a, &b])
            .unwrap();
        short
            .commit_control(&Identity::from_owner(&a), 1, b"a", b"a", true)
            .unwrap();
        short
            .validate_batch_at_ceiling(&[
                (Identity::from_owner(&a), 1, b"a".to_vec()),
                (Identity::from_owner(&b), 1, b"b".to_vec()),
            ])
            .unwrap();
    }

    #[test]
    fn control_batch_does_not_spend_a_later_receipts_unrealized_shrinkage() {
        let a = row("s", "a", 0, 1);
        let b = row("s", "b", 1, 2);
        let c = row("s", "c", 2, 3);
        let aid = Identity::from_owner(&a);
        let mut owners = StageOwners::with_limits(8, 28, 4)
            .prepare_rows(1, 4, &[&a, &b, &c])
            .unwrap();
        owners
            .commit_control(&aid, 1, b"12345678", b"12345678", false)
            .unwrap();
        let mut controls = vec![
            (Identity::from_owner(&b), 1, b"b".to_vec()),
            (Identity::from_owner(&c), 1, b"c".to_vec()),
            (aid, 2, b"a".to_vec()),
        ];
        for (identity, operation, request) in &controls {
            assert_eq!(
                owners
                    .check_at_ceiling(identity, *operation, request)
                    .unwrap(),
                ControlCheck::New
            );
        }
        // Final net use would be 27 <= 28, but before A shrinks the two new
        // receipts can reach 34. Refuse before B has any native side effect.
        let before = owners.clone();
        assert!(owners.validate_batch_at_ceiling(&controls).is_err());
        controls.reverse();
        assert!(owners.validate_batch_at_ceiling(&controls).is_err());
        assert_eq!(owners, before);
    }

    #[test]
    fn control_batch_validates_later_authority_and_rejects_even_identical_duplicate_slots() {
        let a = row("s", "a", 0, 1);
        let b = row("s", "b", 1, 2);
        let owners = StageOwners::with_limits(8, 64, 4)
            .prepare_rows(1, 4, &[&a, &b])
            .unwrap();
        let first = (Identity::from_owner(&a), 1, b"a".to_vec());
        let mut second = (Identity::from_owner(&b), 1, b"b".to_vec());
        second.0.incarnation += 1;
        let before = owners.clone();
        assert!(
            owners
                .validate_batch_at_ceiling(&[first.clone(), second])
                .is_err()
        );
        assert_eq!(
            owners
                .validate_batch_at_ceiling(&[first.clone(), first])
                .unwrap_err(),
            "stage control batch repeats a physical slot"
        );
        assert_eq!(owners, before);
    }

    #[test]
    fn reusing_a_released_slot_reclaims_only_that_slots_receipt_bytes() {
        let a = row("s", "a", 0, 1);
        let b = row("s", "b", 1, 2);
        let mut owners = StageOwners::with_limits(8, 24, 4)
            .prepare_rows(1, 4, &[&a, &b])
            .unwrap();
        owners
            .commit_control(&Identity::from_owner(&a), 1, b"a", b"released", true)
            .unwrap();
        owners
            .commit_control(&Identity::from_owner(&b), 1, b"b", b"b", false)
            .unwrap();
        assert_eq!(owners.receipt_bytes, 11);
        let fresh = row("s", "next", 0, 3);
        let candidate = owners.prepare_rows(1, 4, &[&fresh]).unwrap();
        assert_eq!(owners.receipt_bytes, 11);
        assert_eq!(candidate.receipt_bytes, 2);
        assert_eq!(
            candidate
                .check_at_ceiling(&Identity::from_owner(&b), 1, b"b")
                .unwrap(),
            ControlCheck::Replay(b"b".to_vec())
        );
    }

    #[test]
    fn watermark_budget_is_bounded_without_forgetting_retired_sessions() {
        let a = row("a", "r", 0, 1);
        let b = row("b", "r", 0, 1);
        let mut owners = StageOwners::with_limits(16, 64, 2)
            .prepare_rows(1, 4, &[&a])
            .unwrap();
        release(&mut owners, &a, 1);
        owners = owners.prepare_rows(1, 4, &[&b]).unwrap();
        release(&mut owners, &b, 1);
        let before = owners.clone();
        assert!(owners.prepare_rows(1, 4, &[&row("c", "r", 0, 1)]).is_err());
        assert_eq!(owners, before);
        // Existing watermark entries can advance without consuming a new
        // entry. At capacity, deleting an old session to make room is unsafe.
        let fresh = row("a", "new", 0, 2);
        owners = owners.prepare_rows(1, 4, &[&fresh]).unwrap();
        assert_eq!(owners.watermarks.len(), 2);
        assert!(owners.prepare_rows(1, 4, &[&a]).is_err());
        // A later new owner exceeding the budget cannot partially claim the
        // earlier owner's slot in the same prepared batch.
        let empty = StageOwners::with_limits(8, 32, 1);
        assert!(
            empty
                .prepare_rows(1, 4, &[&row("s", "one", 0, 1), &row("s", "two", 1, 2)])
                .is_err()
        );
        assert_eq!(empty.slots.len(), 0);
        assert_eq!(empty.watermarks.len(), 0);
    }
}

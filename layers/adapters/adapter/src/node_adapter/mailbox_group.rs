//! Atomic storage reservation for a known fan-out, not queue-slot or remote
//! admission. Each item remains a move-only claim against the actual mailbox.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupReserveError {
    Empty,
    MissingByteLimit,
    InvalidFootprint {
        index: usize,
        bytes: usize,
        minimum: usize,
    },
    Closed,
    Full,
    CostOverflow,
    AllocationFailed,
    PoolTooSmall {
        required_bytes: usize,
        provided_bytes: usize,
    },
    PoolItemTooLarge {
        required_bytes: usize,
        assignable_bytes: usize,
    },
    TooLarge {
        required_count: usize,
        count_limit: usize,
        required_bytes: usize,
        byte_limit: Option<usize>,
    },
}

/// The real group array remains allocated even after claims leave its slots.
/// Its bytes must stay charged until the array itself has been destroyed.
struct GroupBackingClaim {
    budget: Arc<Mutex<Budget>>,
    capacity: Arc<Mutex<CapacityState>>,
    bytes: usize,
}

impl GroupBackingClaim {
    fn release_quiet(&mut self) -> bool {
        if self.bytes == 0 {
            return false;
        }
        let mut budget = self.budget.lock().unwrap_or_else(|e| e.into_inner());
        budget.used_bytes = budget
            .used_bytes
            .checked_sub(self.bytes)
            .expect("group backing charge is owned until its allocation is destroyed");
        self.bytes = 0;
        true
    }
}

impl Drop for GroupBackingClaim {
    fn drop(&mut self) {
        if self.release_quiet() && !std::thread::panicking() {
            notify_capacity(&self.capacity, false);
        }
    }
}

/// One atomic acquisition, independently transferable items. This is not a
/// distributed transaction and conveys no source, sequence, or native authority.
pub struct CompletionReservationGroup {
    items: VecDeque<CompletionReservation>,
    backing: Option<GroupBackingClaim>,
    assignable_bytes: usize,
}

impl std::fmt::Debug for CompletionReservationGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompletionReservationGroup")
            .field("remaining", &self.items.len())
            .field("backing_bytes", &self.backing_bytes())
            .field("assignable_bytes", &self.assignable_bytes)
            .finish()
    }
}

impl CompletionReservationGroup {
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn backing_bytes(&self) -> usize {
        self.backing.as_ref().map_or(0, |claim| claim.bytes)
    }
    pub fn assignable_bytes(&self) -> usize {
        self.assignable_bytes
    }
    /// Input order is preserved. A failed publication returns this same item's
    /// reservation; it does not consume a different item's capacity.
    pub fn take_next(&mut self) -> Option<CompletionReservation> {
        self.items.pop_front()
    }

    /// Assign part of an already charged aggregate pool to the next item.
    /// The mailbox's used count/bytes do not change here: ownership moves from
    /// the group's unpublished pool to one linear publication reservation.
    pub fn take_for(
        &mut self,
        footprint_bytes: usize,
    ) -> Result<CompletionReservation, GroupReserveError> {
        let required = footprint_bytes
            .checked_add(COMPLETION_ENTRY_OVERHEAD_BYTES)
            .ok_or(GroupReserveError::CostOverflow)?;
        let current = self
            .items
            .front()
            .ok_or(GroupReserveError::Empty)?
            .claim
            .bytes;
        let additional = required.saturating_sub(current);
        if additional > self.assignable_bytes {
            return Err(GroupReserveError::PoolItemTooLarge {
                required_bytes: required,
                assignable_bytes: self.assignable_bytes,
            });
        }
        let mut reservation = self.items.pop_front().expect("front was inspected");
        if additional != 0 {
            reservation.claim.bytes = reservation
                .claim
                .bytes
                .checked_add(additional)
                .ok_or(GroupReserveError::CostOverflow)?;
            let backing = self.backing.as_mut().expect("assignable pool has backing");
            backing.bytes = backing
                .bytes
                .checked_sub(additional)
                .expect("assignable bytes are part of the backing claim");
            self.assignable_bytes -= additional;
        }
        Ok(reservation)
    }
}

impl Drop for CompletionReservationGroup {
    fn drop(&mut self) {
        // Retire everything without callbacks, then notify once. A panicking
        // caller waker must not be invoked again by another field's Drop during
        // unwind. Popped claims are elsewhere and retain their own lifetimes.
        for item in &mut self.items {
            item.claim.release_quiet();
        }
        drop(std::mem::take(&mut self.items));
        if let Some(mut backing) = self.backing.take() {
            // The actual array is gone before its bytes become available.
            if backing.release_quiet() && !std::thread::panicking() {
                notify_capacity(&backing.capacity, false);
            }
        }
    }
}

fn check_group_budget(
    budget: &Budget,
    count: usize,
    bytes: usize,
) -> Result<(usize, usize), GroupReserveError> {
    if budget.closed {
        return Err(GroupReserveError::Closed);
    }
    if budget.byte_limit.is_none() {
        return Err(GroupReserveError::MissingByteLimit);
    }
    if count > budget.capacity || budget.byte_limit.is_some_and(|limit| bytes > limit) {
        return Err(GroupReserveError::TooLarge {
            required_count: count,
            count_limit: budget.capacity,
            required_bytes: bytes,
            byte_limit: budget.byte_limit,
        });
    }
    let next_count = budget
        .used_count
        .checked_add(count)
        .ok_or(GroupReserveError::Full)?;
    let next_bytes = budget
        .used_bytes
        .checked_add(bytes)
        .ok_or(GroupReserveError::Full)?;
    if next_count > budget.capacity || budget.byte_limit.is_some_and(|limit| next_bytes > limit) {
        return Err(GroupReserveError::Full);
    }
    Ok((next_count, next_bytes))
}

impl CompletionPublisher {
    /// Atomically reserve a known set of Event-footprint bounds. This reserves
    /// actual retained storage, NOT simultaneous queue slots. Queue Full still
    /// returns Event + reservation; a one-slot queue can drain the group serially.
    ///
    /// Array backing is charged separately from item claims, including while
    /// popped items are queued/owned elsewhere. No Event is cloned or serialized.
    /// Requires a declared byte bound: emptying every group item must not allow
    /// unlimited retained group arrays under a count-only compatibility budget.
    pub fn try_reserve_group(
        &self,
        footprints: &[usize],
    ) -> Result<CompletionReservationGroup, GroupReserveError> {
        self.reserve_group_before_commit(footprints, || {})
    }

    fn reserve_group_before_commit(
        &self,
        footprints: &[usize],
        before_commit: impl FnOnce(),
    ) -> Result<CompletionReservationGroup, GroupReserveError> {
        if footprints.is_empty() {
            return Err(GroupReserveError::Empty);
        }
        let minimum = std::mem::size_of::<Event>();
        let mut item_bytes = 0usize;
        for (index, &bytes) in footprints.iter().enumerate() {
            if bytes < minimum {
                return Err(GroupReserveError::InvalidFootprint {
                    index,
                    bytes,
                    minimum,
                });
            }
            item_bytes = item_bytes
                .checked_add(
                    bytes
                        .checked_add(COMPLETION_ENTRY_OVERHEAD_BYTES)
                        .ok_or(GroupReserveError::CostOverflow)?,
                )
                .ok_or(GroupReserveError::CostOverflow)?;
        }
        let count = footprints.len();
        let minimum_backing = count
            .checked_mul(std::mem::size_of::<CompletionReservation>())
            .ok_or(GroupReserveError::CostOverflow)?;
        let minimum_total = item_bytes
            .checked_add(minimum_backing)
            .ok_or(GroupReserveError::CostOverflow)?;
        {
            let budget = self.budget.lock().map_err(|_| GroupReserveError::Closed)?;
            check_group_budget(&budget, count, minimum_total)?;
        }

        // Empty, uncharged backing only. No live Claim may exist before the
        // entire acquisition commits, including when allocation or close fails.
        // Concurrent preparations can own uncharged temporary arrays here:
        // this API bounds successful retained storage, not transient RSS.
        let mut items = VecDeque::new();
        items
            .try_reserve_exact(count)
            .map_err(|_| GroupReserveError::AllocationFailed)?;
        let backing_bytes = items
            .capacity()
            .checked_mul(std::mem::size_of::<CompletionReservation>())
            .ok_or(GroupReserveError::CostOverflow)?;
        let total = item_bytes
            .checked_add(backing_bytes)
            .ok_or(GroupReserveError::CostOverflow)?;
        before_commit();
        {
            let mut budget = self.budget.lock().map_err(|_| GroupReserveError::Closed)?;
            // Recheck actual capacity, closure and intervening claims. No
            // compare/publish gap is allowed between this check and both writes.
            let (next_count, next_bytes) = check_group_budget(&budget, count, total)?;
            budget.used_count = next_count;
            budget.used_bytes = next_bytes;
        }
        for &bytes in footprints {
            // Addition and backing allocation were proved before commit. There
            // are no fallible operations or user callbacks in this installation.
            items.push_back(CompletionReservation {
                claim: Claim {
                    budget: Arc::clone(&self.budget),
                    capacity: Arc::clone(&self.capacity),
                    bytes: bytes + COMPLETION_ENTRY_OVERHEAD_BYTES,
                    active: true,
                },
            });
        }
        Ok(CompletionReservationGroup {
            items,
            backing: Some(GroupBackingClaim {
                budget: Arc::clone(&self.budget),
                capacity: Arc::clone(&self.capacity),
                bytes: backing_bytes,
            }),
            assignable_bytes: 0,
        })
    }

    /// Reserve a count and aggregate retained-byte ceiling before the caller
    /// knows each result's exact allocation footprint. Every item starts with
    /// the minimum Event claim; `take_for` moves bytes out of the same charged
    /// pool, so native/result preparation can never increase mailbox usage.
    pub fn try_reserve_pool(
        &self,
        count: usize,
        total_retained_bytes: usize,
    ) -> Result<CompletionReservationGroup, GroupReserveError> {
        if count == 0 {
            return Err(GroupReserveError::Empty);
        }
        let item_bytes = count
            .checked_mul(
                std::mem::size_of::<Event>()
                    .checked_add(COMPLETION_ENTRY_OVERHEAD_BYTES)
                    .ok_or(GroupReserveError::CostOverflow)?,
            )
            .ok_or(GroupReserveError::CostOverflow)?;
        {
            let budget = self.budget.lock().map_err(|_| GroupReserveError::Closed)?;
            check_group_budget(&budget, count, total_retained_bytes)?;
        }
        let mut items = VecDeque::new();
        items
            .try_reserve_exact(count)
            .map_err(|_| GroupReserveError::AllocationFailed)?;
        let backing_bytes = items
            .capacity()
            .checked_mul(std::mem::size_of::<CompletionReservation>())
            .ok_or(GroupReserveError::CostOverflow)?;
        let minimum = item_bytes
            .checked_add(backing_bytes)
            .ok_or(GroupReserveError::CostOverflow)?;
        if total_retained_bytes < minimum {
            return Err(GroupReserveError::PoolTooSmall {
                required_bytes: minimum,
                provided_bytes: total_retained_bytes,
            });
        }
        {
            let mut budget = self.budget.lock().map_err(|_| GroupReserveError::Closed)?;
            let (next_count, next_bytes) =
                check_group_budget(&budget, count, total_retained_bytes)?;
            budget.used_count = next_count;
            budget.used_bytes = next_bytes;
        }
        let minimum_item = std::mem::size_of::<Event>() + COMPLETION_ENTRY_OVERHEAD_BYTES;
        for _ in 0..count {
            items.push_back(CompletionReservation {
                claim: Claim {
                    budget: Arc::clone(&self.budget),
                    capacity: Arc::clone(&self.capacity),
                    bytes: minimum_item,
                    active: true,
                },
            });
        }
        Ok(CompletionReservationGroup {
            items,
            backing: Some(GroupBackingClaim {
                budget: Arc::clone(&self.budget),
                capacity: Arc::clone(&self.capacity),
                bytes: total_retained_bytes - item_bytes,
            }),
            assignable_bytes: total_retained_bytes - minimum,
        })
    }
}

#[cfg(test)]
#[path = "mailbox_group_tests.rs"]
mod tests;

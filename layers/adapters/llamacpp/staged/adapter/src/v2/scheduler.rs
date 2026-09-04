use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Prefill,
    Decode,
    Verify,
    Replay,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Demand {
    pub request_id: String,
    pub sequence_id: u32,
    pub compatibility: String,
    pub phase: Phase,
    /// Prefill rows available in sequence order. Decode always contributes one.
    pub available_rows: usize,
    /// Verify and replay rows are one indivisible sequence transaction.
    pub atomic: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub request_id: String,
    pub sequence_id: u32,
    pub phase: Phase,
    pub rows: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchedulerError {
    ZeroCapacity,
    EmptyIdentity,
    DuplicateSequence(u32),
    MixedCompatibility,
    InvalidDemand,
    AtomicDemandExceedsCapacity,
}

/// llama.cpp-compatible mixed-batch planner.
///
/// Attention models fill one logical llama batch up to `llama_n_batch` and
/// allow llama.cpp to split it at `n_ubatch`. Recurrent/hybrid models negotiate
/// equal per-sequence widths, so one call constructs exactly one physical
/// UBATCH instead. Decode consumes one row first; ordinary attention Prefill
/// uses the remaining rows in rotating water-fill order.
pub struct Scheduler {
    cursor: usize,
}

impl Scheduler {
    pub fn new() -> Self {
        Self { cursor: 0 }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn plan(
        &mut self,
        demands: &[Demand],
        capacity: usize,
    ) -> Result<Vec<Allocation>, SchedulerError> {
        self.plan_with_physical_capacity(demands, capacity, capacity, false, usize::MAX, false)
    }

    pub fn plan_with_physical_capacity(
        &mut self,
        demands: &[Demand],
        ordinary_capacity: usize,
        physical_capacity: usize,
        equal_sequence_ubatch: bool,
        max_atomic_sequences: usize,
        atomic_batch_exclusive: bool,
    ) -> Result<Vec<Allocation>, SchedulerError> {
        if ordinary_capacity == 0 || physical_capacity == 0 || max_atomic_sequences == 0 {
            return Err(SchedulerError::ZeroCapacity);
        }
        if demands.is_empty() {
            return Ok(Vec::new());
        }
        self.validate(demands)?;
        let has_atomic = demands.iter().any(|demand| demand.atomic);
        // A speculative transaction must stay inside one physical UBATCH.
        // Reserve it first, but emit it after ordinary rows.  Recurrent
        // rollback snapshots belong to the most recent UBATCH until the tail
        // resolves verification, so no later UBATCH may follow it.
        if has_atomic {
            self.plan_atomic_window(
                demands,
                physical_capacity,
                equal_sequence_ubatch,
                max_atomic_sequences,
                atomic_batch_exclusive,
            )
        } else if equal_sequence_ubatch {
            self.plan_equal_ordinary(demands, physical_capacity)
        } else {
            self.plan_ordinary(demands, ordinary_capacity)
        }
    }

    fn plan_equal_ordinary(
        &mut self,
        demands: &[Demand],
        capacity: usize,
    ) -> Result<Vec<Allocation>, SchedulerError> {
        let start = self.cursor % demands.len();
        let order: Vec<usize> = (0..demands.len())
            .map(|offset| (start + offset) % demands.len())
            .take(capacity)
            .collect();
        // On a model whose memory forces equal per-sequence widths, one
        // decode row makes the common width one - a decode has exactly one
        // row to give. Mixing a prompt into that batch therefore sends one
        // row of a prompt that had thousands ready.
        //
        // Measured on a 35B hybrid: 944.6 rows ready on average at plan time
        // against 9.85 issued, with 54% of plans leaving rows behind and the
        // commonest widths 8, 2, 16 and 4. The prompts were not waiting for
        // arrivals; they were being cut to one row each by a decode sharing
        // their batch.
        //
        // So the participants are decided before the width is: if any decode
        // is ready, this batch is the decodes, and the prompts wait for the
        // next one, where they can share a wide equal UBATCH. Both keep
        // moving because every batch admits one cohort or the other and the
        // round-robin cursor below advances either way.
        let decoding: Vec<usize> = order
            .iter()
            .copied()
            .filter(|index| demands[*index].phase == Phase::Decode)
            .collect();
        let order = if decoding.is_empty() { order } else { decoding };
        let width = if order
            .iter()
            .any(|index| demands[*index].phase == Phase::Decode)
        {
            1
        } else {
            let shared = capacity / order.len();
            order
                .iter()
                .map(|index| demands[*index].available_rows)
                .min()
                .unwrap_or(1)
                .min(shared)
        };
        self.cursor = (start + 1) % demands.len();
        let mut allocations: Vec<_> = order
            .into_iter()
            .map(|index| Allocation {
                request_id: demands[index].request_id.clone(),
                sequence_id: demands[index].sequence_id,
                phase: demands[index].phase,
                rows: width,
            })
            .collect();
        // Stable physical membership is independent of map insertion order.
        allocations.sort_by_key(|allocation| allocation.sequence_id);
        Ok(allocations)
    }

    fn plan_atomic_window(
        &mut self,
        demands: &[Demand],
        capacity: usize,
        equal_sequence_ubatch: bool,
        max_atomic_sequences: usize,
        atomic_batch_exclusive: bool,
    ) -> Result<Vec<Allocation>, SchedulerError> {
        let start = self.cursor % demands.len();
        let order: Vec<usize> = (0..demands.len())
            .map(|offset| (start + offset) % demands.len())
            .collect();
        let first_atomic = order
            .iter()
            .copied()
            .find(|index| demands[*index].atomic)
            .expect("atomic window has an atomic demand");
        let width = demands[first_atomic].available_rows;
        if width > capacity {
            return Err(SchedulerError::AtomicDemandExceedsCapacity);
        }

        // One generation round may contain several independent speculative
        // sequences. Keep every allocation at the selected width so an atomic
        // group is never split by llama.cpp.
        let mut atomic = Vec::new();
        let mut used = 0usize;
        for index in order
            .iter()
            .copied()
            .filter(|index| demands[*index].atomic && demands[*index].available_rows == width)
            .take(max_atomic_sequences)
        {
            let Some(next) = used.checked_add(width) else {
                break;
            };
            if next > capacity {
                break;
            }
            atomic.push(index);
            used = next;
        }
        debug_assert!(!atomic.is_empty());

        let ordinary_compatible = |demand: &Demand| {
            !atomic_batch_exclusive
                && !demand.atomic
                && match demand.phase {
                    Phase::Prefill => demand.available_rows >= width,
                    Phase::Decode => width == 1,
                    Phase::Verify | Phase::Replay => false,
                }
        };
        let selected = if equal_sequence_ubatch {
            // split_equal(sequential=true) admits only consecutive sequence
            // ids. Build one contiguous, ascending run around the rotating
            // atomic seed; a missing or incompatible id is a hard UBATCH
            // boundary and must not be discovered after GPU submission.
            let candidates: HashMap<u32, usize> = demands
                .iter()
                .enumerate()
                .filter(|(_, demand)| {
                    (demand.atomic && demand.available_rows == width) || ordinary_compatible(demand)
                })
                .map(|(index, demand)| (demand.sequence_id, index))
                .collect();
            let mut selected = vec![first_atomic];
            let mut low = demands[first_atomic].sequence_id;
            let mut high = low;
            let mut atomic_count = 1usize;
            let max_sequences = capacity / width;
            let mut lower_open = true;
            let mut upper_open = true;
            while selected.len() < max_sequences && (lower_open || upper_open) {
                let mut added = false;
                if lower_open {
                    let next = low
                        .checked_sub(1)
                        .and_then(|id| candidates.get(&id).copied());
                    if let Some(index) = next.filter(|index| {
                        !demands[*index].atomic || atomic_count < max_atomic_sequences
                    }) {
                        low -= 1;
                        atomic_count += usize::from(demands[index].atomic);
                        selected.insert(0, index);
                        added = true;
                    } else {
                        lower_open = false;
                    }
                }
                if selected.len() < max_sequences && upper_open {
                    let next = high
                        .checked_add(1)
                        .and_then(|id| candidates.get(&id).copied());
                    if let Some(index) = next.filter(|index| {
                        !demands[*index].atomic || atomic_count < max_atomic_sequences
                    }) {
                        high += 1;
                        atomic_count += usize::from(demands[index].atomic);
                        selected.push(index);
                        added = true;
                    } else {
                        upper_open = false;
                    }
                }
                if !added {
                    break;
                }
            }
            selected
        } else {
            let mut selected = Vec::new();
            if !atomic_batch_exclusive {
                for index in order
                    .iter()
                    .copied()
                    .filter(|index| ordinary_compatible(&demands[*index]))
                {
                    if used.checked_add(width).is_some_and(|next| next <= capacity) {
                        selected.push(index);
                        used += width;
                    }
                }
            }
            selected.extend(atomic);
            selected
        };
        self.cursor = (start + 1) % demands.len();
        Ok(selected
            .into_iter()
            .map(|index| Allocation {
                request_id: demands[index].request_id.clone(),
                sequence_id: demands[index].sequence_id,
                phase: demands[index].phase,
                rows: width,
            })
            .collect())
    }

    fn plan_ordinary(
        &mut self,
        demands: &[Demand],
        capacity: usize,
    ) -> Result<Vec<Allocation>, SchedulerError> {
        if capacity == 0 {
            return Err(SchedulerError::ZeroCapacity);
        }
        if demands.is_empty() {
            return Ok(Vec::new());
        }
        self.validate(demands)?;
        let start = self.cursor % demands.len();
        let order: Vec<usize> = (0..demands.len())
            .map(|offset| (start + offset) % demands.len())
            .collect();
        let mut rows = vec![0usize; demands.len()];
        let mut remaining = capacity;

        for &index in &order {
            if remaining == 0 {
                break;
            }
            if demands[index].phase == Phase::Decode {
                rows[index] = 1;
                remaining -= 1;
            }
        }

        // Give every pending prompt one row before filling residual capacity.
        for &index in &order {
            if remaining == 0 {
                break;
            }
            if demands[index].phase == Phase::Prefill {
                rows[index] = 1;
                remaining -= 1;
            }
        }

        while remaining > 0 {
            let mut progressed = false;
            for &index in &order {
                if remaining == 0 {
                    break;
                }
                let demand = &demands[index];
                if demand.phase == Phase::Prefill && rows[index] < demand.available_rows {
                    rows[index] += 1;
                    remaining -= 1;
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }

        self.cursor = (start + 1) % demands.len();
        Ok(order
            .into_iter()
            .filter(|index| rows[*index] > 0)
            .map(|index| Allocation {
                request_id: demands[index].request_id.clone(),
                sequence_id: demands[index].sequence_id,
                phase: demands[index].phase,
                rows: rows[index],
            })
            .collect())
    }

    fn validate(&self, demands: &[Demand]) -> Result<(), SchedulerError> {
        let compatibility = &demands[0].compatibility;
        if compatibility.is_empty() {
            return Err(SchedulerError::EmptyIdentity);
        }
        let mut sequences = HashSet::with_capacity(demands.len());
        for demand in demands {
            if demand.request_id.is_empty() || demand.compatibility.is_empty() {
                return Err(SchedulerError::EmptyIdentity);
            }
            if &demand.compatibility != compatibility {
                return Err(SchedulerError::MixedCompatibility);
            }
            if !sequences.insert(demand.sequence_id) {
                return Err(SchedulerError::DuplicateSequence(demand.sequence_id));
            }
            if demand.available_rows == 0
                || (demand.phase == Phase::Decode && demand.available_rows != 1)
                || (demand.atomic != matches!(demand.phase, Phase::Verify | Phase::Replay))
            {
                return Err(SchedulerError::InvalidDemand);
            }
        }
        Ok(())
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

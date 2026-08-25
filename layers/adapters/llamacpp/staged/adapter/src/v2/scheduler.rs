use std::collections::HashSet;

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
/// One call fills one logical llama batch up to `llama_n_batch`. llama.cpp may
/// split it at `n_ubatch`; the stage callback, not this planner, is the
/// authority for those physical capsules. Decode consumes one row first; the
/// remaining rows are water-filled across Prefill in rotating order.
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
        self.plan_with_physical_capacity(demands, capacity, capacity)
    }

    pub fn plan_with_physical_capacity(
        &mut self,
        demands: &[Demand],
        ordinary_capacity: usize,
        physical_capacity: usize,
    ) -> Result<Vec<Allocation>, SchedulerError> {
        if ordinary_capacity == 0 || physical_capacity == 0 {
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
            self.plan_atomic_window(demands, physical_capacity)
        } else {
            self.plan_ordinary(demands, ordinary_capacity)
        }
    }

    fn plan_atomic_window(
        &mut self,
        demands: &[Demand],
        capacity: usize,
    ) -> Result<Vec<Allocation>, SchedulerError> {
        let start = self.cursor % demands.len();
        let order: Vec<usize> = (0..demands.len())
            .map(|offset| (start + offset) % demands.len())
            .collect();
        let atomic = order
            .iter()
            .copied()
            .find(|index| demands[*index].atomic)
            .expect("atomic window has an atomic demand");
        let width = demands[atomic].available_rows;
        if width > capacity {
            return Err(SchedulerError::AtomicDemandExceedsCapacity);
        }

        // llama_memory_recurrent asks split_equal() to take the same number
        // of rows from every participating sequence.  A shorter or longer
        // ordinary allocation would therefore leave either Verify or the
        // ordinary sequence in a later physical UBATCH.  Admit only equal
        // widths so this logical batch is mechanically one physical UBATCH.
        let mut selected = Vec::new();
        let mut used = width;
        for index in order
            .iter()
            .copied()
            .filter(|index| !demands[*index].atomic)
        {
            let demand = &demands[index];
            let compatible_width = match demand.phase {
                Phase::Prefill => demand.available_rows >= width,
                Phase::Decode => width == 1,
                Phase::Verify | Phase::Replay => false,
            };
            if compatible_width && used.checked_add(width).is_some_and(|next| next <= capacity) {
                selected.push(index);
                used += width;
            }
        }
        selected.push(atomic);
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

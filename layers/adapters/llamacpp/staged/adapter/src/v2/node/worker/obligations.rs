//! Event-ID obligations, not a host memory budget. Pending releases each own
//! one future receipt share; grouping can only reduce that count. A committed
//! suffix also keeps its shares while the active publication services ACKs.
use super::effects::CommittedEffect;
use super::*;

pub(super) fn effect_event_count(
    effects: &std::collections::VecDeque<CommittedEffect>,
) -> Result<u64, String> {
    effects.iter().try_fold(0u64, |total, effect| {
        total
            .checked_add(effect.event_count()?)
            .ok_or_else(|| "completion obligation count overflow".into())
    })
}

impl CommittedEffect {
    pub(super) fn event_count(&self) -> Result<u64, String> {
        match self {
            Self::Settle { .. } | Self::Release { .. } => Ok(0),
            Self::ForwardObserved { telemetry, .. } => u64::try_from(telemetry.len())
                .ok()
                .and_then(|count| count.checked_add(1))
                .ok_or_else(|| "completion obligation count overflow".into()),
            _ => Ok(1),
        }
    }
}

impl Worker {
    pub(super) fn ensure_event_id_obligations(
        &self,
        additional: u64,
        retired_future: u64,
    ) -> Result<(), String> {
        let future = u64::try_from(self.state.pending_releases.len())
            .map_err(|_| "completion obligation count overflow")?
            .checked_sub(retired_future)
            .ok_or("cannot retire an unowned receipt obligation")?;
        let count = effect_event_count(&self.effects)?
            .checked_add(self.active_effect_ids)
            .and_then(|v| v.checked_add(future))
            .and_then(|v| v.checked_add(u64::from(self.deferred_ack_error.is_some())))
            .and_then(|v| v.checked_add(additional))
            .ok_or("completion obligation count overflow")?;
        self.state
            .next_event
            .checked_add(count)
            .ok_or("completion event ID obligations are exhausted")?;
        Ok(())
    }
}

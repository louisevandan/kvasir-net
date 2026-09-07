use super::*;
use std::collections::HashSet;

impl PhysicalCapsule {
    pub fn validate(&self) -> Result<(), CapsuleError> {
        let rows = self.invocation.sequence_counts.len();
        if self.execution_id == 0
            || rows == 0
            || rows > MAX_ROWS
            || self.invocation.n_pos == 0
            || self.invocation.n_pos > 4
            || self.invocation.positions.len()
                != rows
                    .checked_mul(self.invocation.n_pos as usize)
                    .ok_or(CapsuleError::IntegerOverflow)?
            || self.invocation.output.len() != rows
            || self.owners.len() != rows
        {
            return Err(CapsuleError::InvalidInvocation);
        }
        let sequence_total =
            self.invocation
                .sequence_counts
                .iter()
                .try_fold(0usize, |sum, count| {
                    if *count == 0 {
                        return Err(CapsuleError::InvalidInvocation);
                    }
                    sum.checked_add(*count as usize)
                        .ok_or(CapsuleError::IntegerOverflow)
                })?;
        if sequence_total != self.invocation.sequence_ids.len() {
            return Err(CapsuleError::InvalidInvocation);
        }
        let mut owner_rows = HashSet::with_capacity(rows);
        for (index, owner) in self.owners.iter().enumerate() {
            let speculative = matches!(owner.phase, Phase::Verify | Phase::Replay);
            if owner.load_generation == 0
                || owner.incarnation == 0
                || !owner.has_canonical_request_identity()
                || validate_reply_options(&owner.reply, &owner.options).is_err()
                || owner.request_id.len() > MAX_STRING
                || owner.sequence_key.len() > MAX_STRING
                || owner.session_id.len() > MAX_STRING
                || owner.max_tokens == 0
                || owner.generated_tokens >= owner.max_tokens
                || owner.position > i32::MAX as u32
                || owner.output != self.invocation.output[index]
                || owner.position as i64 != self.invocation.positions[index] as i64
                || (speculative
                    && (owner.speculative_id == 0
                        || owner.speculative_count == 0
                        || owner.speculative_index >= owner.speculative_count))
                || (!speculative
                    && (owner.speculative_id != 0
                        || owner.speculative_index != 0
                        || owner.speculative_count != 0))
                || (owner.phase == Phase::Verify && !owner.output)
                || (owner.phase == Phase::Replay && owner.output)
                || !owner_rows.insert((owner.sequence_id, owner.position))
            {
                return Err(CapsuleError::InvalidOwner);
            }
        }
        if (!self.terminal && self.tensors.is_empty())
            || (self.terminal && !self.tensors.is_empty())
            || self.tensors.len() > MAX_TENSORS
        {
            return Err(CapsuleError::InvalidTensor);
        }
        for (index, tensor) in self.tensors.iter().enumerate() {
            let descriptor = &tensor.descriptor;
            if descriptor.dimensions.is_empty()
                || descriptor.dimensions.len() > 4
                || descriptor.dimensions.len() != descriptor.strides.len()
                || descriptor.name.is_empty()
                || descriptor.name.len() > MAX_STRING
                || descriptor.dimensions.iter().any(|value| *value <= 0)
            {
                return Err(CapsuleError::InvalidTensor);
            }
            match descriptor.alias_of {
                Some(alias) if alias as usize >= index || !tensor.data.is_empty() => {
                    return Err(CapsuleError::InvalidTensor);
                }
                Some(_) => {}
                None if tensor.data.len() as u64 != descriptor.nbytes => {
                    return Err(CapsuleError::InvalidTensor);
                }
                None => {}
            }
        }
        if !self.terminal && !self.outcomes.is_empty() {
            return Err(CapsuleError::InvalidOwner);
        }
        let mut outcome_owners = HashSet::with_capacity(self.outcomes.len());
        for outcome in &self.outcomes {
            let index = outcome.owner_index as usize;
            if index >= rows
                || !outcome_owners.insert(index)
                || (outcome.generated.is_empty()
                    && outcome.proposal.is_empty()
                    && outcome.replay_tokens.is_empty())
                || outcome
                    .retain_from
                    .is_some_and(|value| value > i32::MAX as u32)
                || outcome.generated.iter().any(|generated| {
                    generated.text.len() > MAX_STRING
                        || generated
                            .stop
                            .as_ref()
                            .is_some_and(|value| value.len() > MAX_STRING)
                })
                || (outcome.retain_from.is_none()
                    && (!outcome.replay_tokens.is_empty() || outcome.replay_position != 0))
                || (outcome.retain_from.is_some() && self.owners[index].phase != Phase::Verify)
                || (outcome.retain_from.is_some()
                    && if outcome.replay_tokens.is_empty() {
                        outcome.replay_position != 0
                    } else {
                        u32::try_from(outcome.replay_tokens.len())
                            .ok()
                            .and_then(|count| outcome.replay_position.checked_add(count))
                            != outcome.retain_from
                    })
            {
                return Err(CapsuleError::InvalidOwner);
            }
        }
        Ok(())
    }
}

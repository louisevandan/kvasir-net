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
            if owner.request_id.is_empty()
                || owner.sequence_key.is_empty()
                || owner.session_id.is_empty()
                || owner.reply.is_empty()
                || owner.request_id.len() > MAX_STRING
                || owner.sequence_key.len() > MAX_STRING
                || owner.session_id.len() > MAX_STRING
                || owner.reply.len() > MAX_STRING
                || owner.options.len() > MAX_STRING
                || owner.max_tokens == 0
                || owner.generated_tokens >= owner.max_tokens
                || owner.position > i32::MAX as u32
                || owner.output != self.invocation.output[index]
                || owner.position as i64
                    != self.invocation.positions[index * self.invocation.n_pos as usize] as i64
                || !owner_rows.insert((owner.sequence_id, owner.position))
            {
                return Err(CapsuleError::InvalidOwner);
            }
        }
        if (!self.terminal && self.tensors.is_empty()) || self.tensors.len() > MAX_TENSORS {
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
        let requested = self
            .invocation
            .output
            .iter()
            .filter(|value| **value)
            .count();
        if (!self.terminal && !self.outcomes.is_empty())
            || (self.terminal && self.outcomes.len() != requested)
        {
            return Err(CapsuleError::InvalidOwner);
        }
        let mut outcome_owners = HashSet::with_capacity(self.outcomes.len());
        for outcome in &self.outcomes {
            let index = outcome.owner_index as usize;
            if index >= rows
                || !self.invocation.output[index]
                || !outcome_owners.insert(index)
                || outcome.text.len() > MAX_STRING
                || outcome
                    .stop
                    .as_ref()
                    .is_some_and(|value| value.len() > MAX_STRING)
            {
                return Err(CapsuleError::InvalidOwner);
            }
        }
        Ok(())
    }
}

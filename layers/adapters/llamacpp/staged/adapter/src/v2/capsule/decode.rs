use super::cursor::Cursor;
use super::*;

pub(super) fn read_capsule(cursor: &mut Cursor<'_>) -> Result<PhysicalCapsule, CapsuleError> {
    let execution_id = cursor.u64()?;
    let capsule_flags = cursor.u32()?;
    if capsule_flags & !1 != 0 {
        return Err(CapsuleError::InvalidInvocation);
    }
    let terminal = capsule_flags & 1 != 0;
    let flags = cursor.u32()?;
    let n_seq_tokens = cursor.u32()?;
    let n_seqs = cursor.u32()?;
    let n_seqs_unq = cursor.u32()?;
    let n_pos = cursor.u32()?;
    let rows = cursor.u32()? as usize;
    let sequence_total = cursor.u32()? as usize;
    let tensor_count = cursor.u32()? as usize;
    let outcome_count = cursor.u32()? as usize;
    if rows == 0 || rows > MAX_ROWS || tensor_count > MAX_TENSORS {
        return Err(CapsuleError::LimitExceeded);
    }
    let position_count = rows
        .checked_mul(n_pos as usize)
        .ok_or(CapsuleError::IntegerOverflow)?;
    let positions = (0..position_count)
        .map(|_| cursor.i32())
        .collect::<Result<_, _>>()?;
    let sequence_counts = (0..rows).map(|_| cursor.u32()).collect::<Result<_, _>>()?;
    let sequence_ids = (0..sequence_total)
        .map(|_| cursor.i32())
        .collect::<Result<_, _>>()?;
    let output_bytes = cursor.take(rows)?;
    if output_bytes.iter().any(|value| *value > 1) {
        return Err(CapsuleError::InvalidInvocation);
    }
    let output = output_bytes.iter().map(|value| *value != 0).collect();
    let mut owners = Vec::with_capacity(rows);
    for _ in 0..rows {
        let load_generation = cursor.u64()?;
        let request_id = cursor.string()?;
        let sequence_key = cursor.string()?;
        let session_id = cursor.string()?;
        let reply = cursor.string()?;
        let sequence_id = cursor.u32()?;
        let phase = match cursor.byte()? {
            0 => Phase::Prefill,
            1 => Phase::Decode,
            2 => Phase::Verify,
            3 => Phase::Replay,
            _ => return Err(CapsuleError::InvalidOwner),
        };
        let owner_output = match cursor.byte()? {
            0 => false,
            1 => true,
            _ => return Err(CapsuleError::InvalidOwner),
        };
        if cursor.u16()? != 0 {
            return Err(CapsuleError::InvalidOwner);
        }
        let position = cursor.u32()?;
        let max_tokens = cursor.u32()?;
        let generated_tokens = cursor.u32()?;
        let input_token = cursor.i32()?;
        let speculative_id = cursor.u64()?;
        let speculative_index = cursor.u32()?;
        let speculative_count = cursor.u32()?;
        let options = cursor.string()?;
        owners.push(RowOwner {
            load_generation,
            request_id,
            sequence_key,
            session_id,
            reply,
            sequence_id,
            phase,
            position,
            max_tokens,
            generated_tokens,
            output: owner_output,
            input_token,
            speculative_id,
            speculative_index,
            speculative_count,
            options,
        });
    }
    let mut tensors = Vec::with_capacity(tensor_count);
    for _ in 0..tensor_count {
        let tensor_type = cursor.i32()?;
        let dimensions_count = cursor.byte()? as usize;
        if cursor.take(3)? != [0, 0, 0] || dimensions_count == 0 || dimensions_count > 4 {
            return Err(CapsuleError::InvalidTensor);
        }
        let dimensions = (0..dimensions_count)
            .map(|_| cursor.i64())
            .collect::<Result<_, _>>()?;
        let strides = (0..dimensions_count)
            .map(|_| cursor.u64())
            .collect::<Result<_, _>>()?;
        let nbytes = cursor.u64()?;
        let view_offset = cursor.u64()?;
        let alias_raw = cursor.i32()?;
        if alias_raw < -1 {
            return Err(CapsuleError::InvalidTensor);
        }
        let name = cursor.string()?;
        let data_size =
            usize::try_from(cursor.u64()?).map_err(|_| CapsuleError::IntegerOverflow)?;
        let data = cursor.take(data_size)?.to_vec();
        tensors.push(Tensor {
            descriptor: TensorDescriptor {
                tensor_type,
                dimensions,
                strides,
                nbytes,
                view_offset,
                alias_of: (alias_raw >= 0).then_some(alias_raw as u32),
                name,
            },
            data,
        });
    }
    let mut outcomes = Vec::with_capacity(outcome_count);
    for _ in 0..outcome_count {
        let owner_index = cursor.u32()?;
        let generated_count = cursor.u32()? as usize;
        let proposal_count = cursor.u32()? as usize;
        let replay_count = cursor.u32()? as usize;
        let retain_raw = cursor.i32()?;
        if retain_raw < -1 {
            return Err(CapsuleError::InvalidOwner);
        }
        let replay_position = cursor.u32()?;
        let mut generated = Vec::with_capacity(generated_count);
        for _ in 0..generated_count {
            let token = cursor.i32()?;
            let position = cursor.u32()?;
            let text = cursor.string()?;
            let stop_value = cursor.string()?;
            generated.push(GeneratedToken {
                token,
                text,
                position,
                stop: (!stop_value.is_empty()).then_some(stop_value),
            });
        }
        let proposal = (0..proposal_count)
            .map(|_| cursor.i32())
            .collect::<Result<Vec<_>, _>>()?;
        let replay_tokens = (0..replay_count)
            .map(|_| cursor.i32())
            .collect::<Result<Vec<_>, _>>()?;
        outcomes.push(PhysicalOutcome {
            owner_index,
            generated,
            proposal,
            retain_from: (retain_raw >= 0).then_some(retain_raw as u32),
            replay_tokens,
            replay_position,
        });
    }
    let capsule = PhysicalCapsule {
        execution_id,
        terminal,
        invocation: Invocation {
            flags,
            n_seq_tokens,
            n_seqs,
            n_seqs_unq,
            n_pos,
            positions,
            sequence_counts,
            sequence_ids,
            output,
        },
        owners,
        tensors,
        outcomes,
    };
    capsule.validate()?;
    Ok(capsule)
}

use super::cursor::Cursor;
use super::*;

// PBv4: owner index, three counts, retain position and replay position.
const OUTCOME_HEADER_BYTES: usize = 6 * 4;
// Token + position + two u16 string lengths; empty text/stop remain valid.
const GENERATED_MIN_BYTES: usize = 4 + 4 + 2 + 2;

/// Necessary wire-size feasibility before reserving decoded vectors. This is
/// not an aggregate heap/RSS budget or a semantic limit on valid wire counts.
/// Division before multiplication keeps hostile u32 counts safe on 32-bit too.
fn require_minimum_bytes(
    mut remaining: usize,
    fields: &[(usize, usize)],
) -> Result<(), CapsuleError> {
    for &(count, minimum) in fields {
        if count > remaining / minimum {
            return Err(CapsuleError::Truncated);
        }
        remaining -= count * minimum;
    }
    Ok(())
}

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
        let incarnation = cursor.u64()?;
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
            incarnation,
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
    require_minimum_bytes(cursor.remaining(), &[(outcome_count, OUTCOME_HEADER_BYTES)])?;
    #[cfg(test)]
    tests::record_capacity("outcomes", outcome_count);
    let mut outcomes = Vec::with_capacity(outcome_count);
    for outcome_index in 0..outcome_count {
        let owner_index = cursor.u32()?;
        let generated_count = cursor.u32()? as usize;
        let proposal_count = cursor.u32()? as usize;
        let replay_count = cursor.u32()? as usize;
        let retain_raw = cursor.i32()?;
        if retain_raw < -1 {
            return Err(CapsuleError::InvalidOwner);
        }
        let replay_position = cursor.u32()?;
        require_minimum_bytes(
            cursor.remaining(),
            &[
                (outcome_count - outcome_index - 1, OUTCOME_HEADER_BYTES),
                (generated_count, GENERATED_MIN_BYTES),
                (proposal_count, 4),
                (replay_count, 4),
            ],
        )?;
        #[cfg(test)]
        tests::record_capacity("generated", generated_count);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static CAPACITIES: RefCell<Option<Vec<(&'static str, usize)>>> = const { RefCell::new(None) };
    }

    // This observes the actual decoder immediately before Vec::with_capacity,
    // not a replacement parser. The ceiling is active only inside this probe:
    // a removed guard must fail safely, never try to exhaust the test machine.
    pub(super) fn record_capacity(kind: &'static str, count: usize) {
        CAPACITIES.with(|slot| {
            if let Some(trace) = slot.borrow_mut().as_mut() {
                assert!(
                    count <= 1024,
                    "unsafe declared {kind} capacity reached allocation: {count}"
                );
                trace.push((kind, count));
            }
        });
    }

    struct Probe;
    impl Drop for Probe {
        fn drop(&mut self) {
            CAPACITIES.with(|slot| *slot.borrow_mut() = None);
        }
    }

    fn decode_observed(
        bytes: &[u8],
    ) -> (Result<CapsuleSet, CapsuleError>, Vec<(&'static str, usize)>) {
        CAPACITIES.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(Vec::new());
        });
        let _probe = Probe;
        let result = CapsuleSet::decode(bytes);
        let trace = CAPACITIES.with(|slot| slot.borrow().as_ref().unwrap().clone());
        (result, trace)
    }

    fn empty_terminal() -> PhysicalCapsule {
        PhysicalCapsule {
            execution_id: 1,
            terminal: true,
            invocation: Invocation {
                flags: 0,
                n_seq_tokens: 1,
                n_seqs: 1,
                n_seqs_unq: 1,
                n_pos: 1,
                positions: vec![0],
                sequence_counts: vec![1],
                sequence_ids: vec![0],
                output: vec![false],
            },
            owners: vec![RowOwner {
                load_generation: 1,
                incarnation: 1,
                request_id: "a".into(),
                sequence_key: "s\0a".into(),
                session_id: "s".into(),
                reply: "r".into(),
                sequence_id: 0,
                phase: Phase::Prefill,
                position: 0,
                max_tokens: 8,
                generated_tokens: 0,
                output: false,
                input_token: 10,
                speculative_id: 0,
                speculative_index: 0,
                speculative_count: 0,
                options: String::new(),
            }],
            tensors: Vec::new(),
            outcomes: Vec::new(),
        }
    }

    fn declared_outcomes(count: u32) -> Vec<u8> {
        let mut bytes = CapsuleSet(vec![empty_terminal()]).encode().unwrap();
        // PBv4: set header 12; capsule execution u64 + ten u32 fields.
        // Outcome count is the last u32 in that 48-byte capsule header.
        bytes[56..60].copy_from_slice(&count.to_le_bytes());
        bytes
    }

    fn outcome_header(bytes: &mut Vec<u8>, generated: u32, proposal: u32, replay: u32) {
        for value in [0, generated, proposal, replay, u32::MAX, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }

    #[test]
    fn short_declared_outcome_table_is_rejected_before_capacity_request() {
        let (result, trace) = decode_observed(&declared_outcomes(3));
        assert_eq!(result, Err(CapsuleError::Truncated));
        assert!(
            trace.is_empty(),
            "short outcome table requested capacity: {trace:?}"
        );
    }

    #[test]
    fn short_declared_generated_table_is_rejected_before_capacity_request() {
        let mut bytes = declared_outcomes(1);
        outcome_header(&mut bytes, 3, 0, 0);
        let (result, trace) = decode_observed(&bytes);
        assert_eq!(result, Err(CapsuleError::Truncated));
        assert_eq!(
            trace,
            vec![("outcomes", 1)],
            "short generated table requested capacity"
        );
    }

    #[test]
    fn maximum_declared_counts_on_tiny_wire_never_reach_large_capacity_requests() {
        let (result, trace) = decode_observed(&declared_outcomes(u32::MAX));
        assert_eq!(result, Err(CapsuleError::Truncated));
        assert!(trace.is_empty());
        for (generated, proposal, replay) in [
            (u32::MAX, 0, 0),
            (0, u32::MAX, 0),
            (0, 0, u32::MAX),
            (u32::MAX, u32::MAX, u32::MAX),
        ] {
            let mut bytes = declared_outcomes(1);
            outcome_header(&mut bytes, generated, proposal, replay);
            let (result, trace) = decode_observed(&bytes);
            assert_eq!(result, Err(CapsuleError::Truncated));
            assert_eq!(trace, vec![("outcomes", 1)]);
        }
    }

    #[test]
    fn generated_storage_cannot_borrow_the_following_outcome_header_bytes() {
        let mut bytes = declared_outcomes(2);
        outcome_header(&mut bytes, 2, 0, 0);
        // This has enough bytes for either two minimum generated records OR
        // the second outcome header, never both. Checking each independently
        // would admit the generated allocation before detecting truncation.
        bytes.extend_from_slice(&[0; 24]);
        let (result, trace) = decode_observed(&bytes);
        assert_eq!(result, Err(CapsuleError::Truncated));
        assert_eq!(trace, vec![("outcomes", 2)]);
    }

    #[test]
    fn generated_storage_must_leave_room_for_both_token_arrays() {
        for (proposal, replay) in [(1, 0), (0, 1), (1, 1)] {
            let mut bytes = declared_outcomes(1);
            outcome_header(&mut bytes, 1, proposal, replay);
            let needed = 12 + 4 * (proposal + replay) as usize;
            bytes.resize(bytes.len() + needed - 1, 0);
            let (result, trace) = decode_observed(&bytes);
            assert_eq!(result, Err(CapsuleError::Truncated));
            assert_eq!(trace, vec![("outcomes", 1)]);
        }
    }

    fn minimum_generated() -> PhysicalCapsule {
        let mut capsule = empty_terminal();
        capsule.invocation.output[0] = true;
        capsule.owners[0].output = true;
        capsule.outcomes.push(PhysicalOutcome {
            owner_index: 0,
            generated: vec![GeneratedToken {
                token: 42,
                text: String::new(),
                position: 1,
                stop: None,
            }],
            proposal: Vec::new(),
            retain_from: None,
            replay_tokens: Vec::new(),
            replay_position: 0,
        });
        capsule
    }

    #[test]
    fn zero_and_exact_minimum_generated_encodings_still_round_trip() {
        let empty = CapsuleSet(vec![empty_terminal()]);
        let empty_wire = empty.encode().unwrap();
        let (result, trace) = decode_observed(&empty_wire);
        assert_eq!(result, Ok(empty));
        assert_eq!(trace, vec![("outcomes", 0)]);

        let minimum = CapsuleSet(vec![minimum_generated()]);
        let wire = minimum.encode().unwrap();
        // Independent literal wire accounting, not the guard's constants.
        assert_eq!(wire.len() - empty_wire.len(), 24 + 12);
        let (result, trace) = decode_observed(&wire);
        assert_eq!(result, Ok(minimum.clone()));
        assert_eq!(result.unwrap().encode().unwrap(), wire);
        assert_eq!(trace, vec![("outcomes", 1), ("generated", 1)]);

        let (result, trace) = decode_observed(&wire[..wire.len() - 1]);
        assert_eq!(result, Err(CapsuleError::Truncated));
        assert_eq!(trace, vec![("outcomes", 1)]);
    }

    #[test]
    fn mixed_capsules_and_checkpoint_arrays_preserve_existing_wire_counts() {
        let mut generated = minimum_generated();
        generated.outcomes[0].generated[0].text = "한🧪".into();
        generated.outcomes[0].generated[0].stop = Some("length".into());
        let mut checkpoint = empty_terminal();
        checkpoint.execution_id = 2;
        checkpoint.invocation.output = vec![true, true];
        checkpoint.invocation.positions = vec![0, 1];
        checkpoint.invocation.sequence_counts = vec![1, 1];
        checkpoint.invocation.sequence_ids = vec![0, 0];
        checkpoint.invocation.n_seq_tokens = 2;
        checkpoint.owners[0].phase = Phase::Verify;
        checkpoint.owners[0].output = true;
        checkpoint.owners[0].speculative_id = 1;
        checkpoint.owners[0].speculative_count = 2;
        let mut second = checkpoint.owners[0].clone();
        second.position = 1;
        second.speculative_index = 1;
        checkpoint.owners.push(second);
        checkpoint.outcomes.push(PhysicalOutcome {
            owner_index: 0,
            generated: Vec::new(),
            proposal: Vec::new(),
            retain_from: Some(2),
            replay_tokens: vec![10, 11],
            replay_position: 0,
        });
        let mut proposed = minimum_generated();
        proposed.execution_id = 3;
        proposed.outcomes[0].proposal = vec![42, 43];
        let expected = CapsuleSet(vec![generated, checkpoint, proposed, {
            let mut empty = empty_terminal();
            empty.execution_id = 4;
            empty
        }]);
        let bytes = expected.encode().unwrap();
        let (result, trace) = decode_observed(&bytes);
        assert_eq!(result, Ok(expected));
        assert_eq!(result.unwrap().encode().unwrap(), bytes);
        assert_eq!(
            trace,
            vec![
                ("outcomes", 1),
                ("generated", 1),
                ("outcomes", 1),
                ("generated", 0),
                ("outcomes", 1),
                ("generated", 1),
                ("outcomes", 0)
            ]
        );
    }
}

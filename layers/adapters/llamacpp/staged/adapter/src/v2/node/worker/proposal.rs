use super::*;

pub(super) fn ready_from_proposal(
    next_speculative_id: &mut u64,
    proposal: Vec<i32>,
    position: u32,
) -> Result<super::super::state::ReadyRows, String> {
    if proposal.is_empty() {
        return Err("continuing tail decision has no proposal".into());
    }
    let speculative = proposal.len() > 1;
    let speculative_id = if speculative {
        let value = *next_speculative_id;
        if value == 0 {
            return Err("speculative identity exhausted".into());
        }
        *next_speculative_id = value
            .checked_add(1)
            .ok_or_else(|| "speculative identity exhausted".to_owned())?;
        value
    } else {
        0
    };
    Ok(super::super::state::ReadyRows {
        phase: if speculative {
            Phase::Verify
        } else {
            Phase::Decode
        },
        tokens: proposal,
        position,
        speculative_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposal_shape_alone_selects_decode_or_atomic_verify() {
        let mut next = 7;
        let decode = ready_from_proposal(&mut next, vec![11], 42).unwrap();
        assert_eq!(decode.phase, Phase::Decode);
        assert_eq!(decode.speculative_id, 0);
        assert_eq!(next, 7);

        let verify = ready_from_proposal(&mut next, vec![11, 12], 43).unwrap();
        assert_eq!(verify.phase, Phase::Verify);
        assert_eq!(verify.speculative_id, 7);
        assert_eq!(next, 8);
        assert!(ready_from_proposal(&mut next, Vec::new(), 44).is_err());
    }
}

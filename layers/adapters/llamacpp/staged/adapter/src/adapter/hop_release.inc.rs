/// Whether an intermediate (non-sampling) stage has done its last useful
/// decode for a sequence that is about to hit its length terminal.
///
/// `carried` is this stage's inbound position; `remaining` is the request's
/// invariant generation ceiling. An intermediate stage does not observe the
/// tail's sampled position, so it releases on the decode whose hidden state
/// the tail will turn into the terminal token: the tail's `carried + 1 >=
/// remaining` rule, one stage earlier.
///
/// This must not become `carried >= remaining`: the tail sends no further hop
/// after its terminal, so waiting for that nonexistent arrival leaks the
/// intermediate slot. A duplicate continuation is a producer/terminal
/// ownership defect, not evidence that this release boundary is one hop late.
fn intermediate_stage_should_release(carried: u32, remaining: u32) -> bool {
    carried.saturating_add(1) >= remaining
}

#[cfg(test)]
mod release_prediction_tests {
    use super::intermediate_stage_should_release;

    #[test]
    fn does_not_release_before_the_final_useful_decode() {
        assert!(!intermediate_stage_should_release(37, 40));
        assert!(!intermediate_stage_should_release(38, 40));
    }

    #[test]
    fn releases_on_the_same_hop_the_tail_will_sample_the_terminal_from() {
        assert!(intermediate_stage_should_release(39, 40));
    }

    #[test]
    fn releases_past_remaining_too_rather_than_only_at_the_exact_boundary() {
        assert!(intermediate_stage_should_release(40, 40));
    }

    #[test]
    fn does_not_overflow_at_u32_max() {
        assert!(intermediate_stage_should_release(u32::MAX, u32::MAX));
    }
}

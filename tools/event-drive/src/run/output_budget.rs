//! OUTER's sampled-output budget. This has no scheduler/native dependency and
//! does not turn emitted text length into token count: an empty EOS is one
//! sampled output, too. `emitted` is the number approved BEFORE `outcome`.
use p4_llamacpp_staged_adapter::v2::OutcomePayload;

pub(super) fn validate_output(
    max_tokens: u32,
    emitted: usize,
    outcome: &OutcomePayload,
) -> Result<(), String> {
    if max_tokens == 0 {
        return Err("output token budget must be nonzero".into());
    }
    let sampled = emitted
        .checked_add(1)
        .ok_or_else(|| "output sampled token count overflow".to_owned())?;
    if sampled > max_tokens as usize {
        return Err(format!(
            "output sampled token count {sampled} exceeds max_tokens {max_tokens}"
        ));
    }
    match outcome.stop.as_deref() {
        Some("length") if sampled != max_tokens as usize => Err(format!(
            "length stop arrived before max_tokens: sampled {sampled} of {max_tokens}"
        )),
        Some("length" | "stop" | "eos") => Ok(()),
        Some(stop) => Err(format!("output stop reason is unknown: {stop}")),
        None if sampled == max_tokens as usize => {
            Err("output at max_tokens requires a terminal stop".into())
        }
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(stop: Option<&str>, text: &str) -> OutcomePayload {
        OutcomePayload {
            load_generation: 1,
            session_id: "session".into(),
            request_id: "request".into(),
            sequence_id: 0,
            token: 42,
            text: text.into(),
            position: 12,
            stop: stop.map(str::to_owned),
        }
    }

    #[test]
    fn empty_eos_is_a_sampled_token_and_cannot_bypass_the_cap() {
        let eos = output(Some("eos"), "");
        assert!(validate_output(1, 0, &eos).is_ok());
        assert_eq!(
            validate_output(1, 1, &eos).unwrap_err(),
            "output sampled token count 2 exceeds max_tokens 1"
        );
        assert_eq!(
            validate_output(0, 0, &eos).unwrap_err(),
            "output token budget must be nonzero"
        );
    }

    #[test]
    fn length_is_exact_and_early_eos_or_stop_remain_valid() {
        assert!(validate_output(3, 0, &output(None, "x")).is_ok());
        assert!(validate_output(3, 1, &output(None, "x")).is_ok());
        assert!(validate_output(3, 2, &output(Some("length"), "x")).is_ok());
        for stop in ["stop", "eos"] {
            assert!(validate_output(3, 0, &output(Some(stop), "x")).is_ok());
            assert!(validate_output(3, 2, &output(Some(stop), "x")).is_ok());
        }
        assert_eq!(
            validate_output(3, 0, &output(Some("length"), "x")).unwrap_err(),
            "length stop arrived before max_tokens: sampled 1 of 3"
        );
        assert_eq!(
            validate_output(3, 2, &output(None, "x")).unwrap_err(),
            "output at max_tokens requires a terminal stop"
        );
    }

    #[test]
    fn unknown_stop_and_count_overflow_fail_closed() {
        for stop in ["", "timeout", "LENGTH", "cancelled"] {
            assert_eq!(
                validate_output(3, 0, &output(Some(stop), "x")).unwrap_err(),
                format!("output stop reason is unknown: {stop}")
            );
        }
        assert_eq!(
            validate_output(u32::MAX, usize::MAX, &output(Some("eos"), "")).unwrap_err(),
            "output sampled token count overflow"
        );
    }
}

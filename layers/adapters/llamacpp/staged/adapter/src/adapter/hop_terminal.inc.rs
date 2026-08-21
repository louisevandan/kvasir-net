/// Commits a native tail result at the request's generation boundary to the
/// adapter-to-agent terminal contract.
///
/// The native position is authoritative for the KV session's lifetime.  Text
/// is not: a decode can advance the native state without yielding externally
/// visible text.  Leaving that boundary as `stop = None` makes the agent
/// count only its visible text, create another decode lap, and send it back to
/// an intermediate stage whose slot has correctly already been released.
///
/// Returns whether this call changed an otherwise-open tail result.  An
/// explicit backend stop (for example EOS) remains its own reason.
fn mark_native_length_terminal(outcome: &mut Option<OutcomeMetadata>, remaining: u32) -> bool {
    let Some(outcome) = outcome.as_mut() else {
        return false;
    };
    if outcome.position < remaining || outcome.stop.is_some() {
        return false;
    }
    outcome.stop = Some("length".to_owned());
    true
}

#[cfg(test)]
mod terminal_tests {
    use super::*;

    fn sampled(position: u32, stop: Option<&str>) -> Option<OutcomeMetadata> {
        Some(OutcomeMetadata {
            token: 1,
            text: "토큰".into(),
            position,
            stop: stop.map(Into::into),
        })
    }

    #[test]
    fn a_tail_at_the_native_limit_commits_one_length_terminal() {
        let mut outcome = sampled(200, None);

        assert!(mark_native_length_terminal(&mut outcome, 200));
        assert_eq!(outcome.as_ref().unwrap().stop.as_deref(), Some("length"));
        assert!(
            !mark_native_length_terminal(&mut outcome, 200),
            "revisiting the same terminal must not create a second transition"
        );
    }

    #[test]
    fn every_terminal_tail_in_a_batch_is_closed_without_changing_early_eos() {
        let mut outcomes = vec![sampled(200, None), sampled(201, None), sampled(200, Some("eos"))];

        let mut changed = 0;
        for outcome in &mut outcomes {
            changed += usize::from(mark_native_length_terminal(outcome, 200));
        }

        assert_eq!(changed, 2);
        assert_eq!(outcomes[0].as_ref().unwrap().stop.as_deref(), Some("length"));
        assert_eq!(outcomes[1].as_ref().unwrap().stop.as_deref(), Some("length"));
        assert_eq!(outcomes[2].as_ref().unwrap().stop.as_deref(), Some("eos"));
    }

    #[test]
    fn sequence_release_is_idempotent_and_leaves_one_tombstone() {
        let mut ledger = SequenceLedger::new();
        ledger.active.insert("sequence".into());

        assert!(!ledger.release("sequence"));
        assert!(!ledger.release("sequence"));

        assert!(!ledger.active.contains("sequence"));
        assert!(ledger.released.contains("sequence"));
        assert_eq!(ledger.released_order.iter().filter(|id| id.as_str() == "sequence").count(), 1);
    }
}

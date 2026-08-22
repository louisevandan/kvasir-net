use super::*;
use crate::contract::{Accepted, Produced, RejectReason, Rejected, SettleReason, Settled};

fn submit(id: &str) -> Submit {
    submit_gen(id, 1)
}

/// `begin` now trusts `Submit.deployment_generation` as-stated rather than
/// silently stamping the ledger's own current generation onto it -- the
/// caller (the P4 broker) is what decides that field, typically by reading
/// `Ledger::generation`/`DeploymentClient::generation` right before building
/// a `Submit`. Tests that call `advance_generation` before `begin` and mean
/// the new entry to land in the new generation must say so explicitly.
fn submit_gen(id: &str, generation: Generation) -> Submit {
    Submit {
        deployment_id: "dep".into(),
        deployment_generation: generation,
        submission_id: id.into(),
        request: "req".into(),
    }
}

#[test]
fn begin_is_new_once_and_already_known_after() {
    let mut ledger = Ledger::new(1);
    assert_eq!(ledger.begin(submit("s1")), Admission::New);
    assert_eq!(ledger.begin(submit("s1")), Admission::AlreadyKnown);
    assert_eq!(ledger.entry_count(), 1);
}

#[test]
fn two_submissions_are_independently_in_flight_at_once() {
    let mut ledger = Ledger::new(1);
    ledger.begin(submit("s1"));
    ledger.begin(submit("s2"));
    assert_eq!(
        ledger.apply(&Event::Accepted(Accepted {
            submission_id: "s1".into(),
        })),
        Verdict::Apply
    );
    // s2 has not settled yet, and applying an event for s1 must not disturb
    // it: this is the ledger-level half of "first submission does not have
    // to settle before a second reaches the coordinator."
    assert_eq!(ledger.state_of("s1"), Some(SubmissionState::Accepted));
    assert_eq!(ledger.state_of("s2"), Some(SubmissionState::Pending));
}

#[test]
fn produced_ordinals_must_be_contiguous_from_zero() {
    let mut ledger = Ledger::new(1);
    ledger.begin(submit("s1"));
    ledger.apply(&Event::Accepted(Accepted {
        submission_id: "s1".into(),
    }));
    assert_eq!(
        ledger.apply(&Event::Produced(Produced {
            submission_id: "s1".into(),
            event_ordinal: 0,
            text: "a".into(),
            generated_tokens: 1,
        })),
        Verdict::Apply
    );
    assert_eq!(
        ledger.apply(&Event::Produced(Produced {
            submission_id: "s1".into(),
            event_ordinal: 2,
            text: "c".into(),
            generated_tokens: 3,
        })),
        Verdict::OutOfOrder {
            expected: 1,
            got: 2
        }
    );
    assert_eq!(
        ledger.apply(&Event::Produced(Produced {
            submission_id: "s1".into(),
            event_ordinal: 1,
            text: "b".into(),
            generated_tokens: 2,
        })),
        Verdict::Apply
    );
}

#[test]
fn settled_is_accepted_once_and_refused_the_second_time() {
    let mut ledger = Ledger::new(1);
    ledger.begin(submit("s1"));
    ledger.apply(&Event::Accepted(Accepted {
        submission_id: "s1".into(),
    }));
    assert_eq!(
        ledger.apply(&Event::Settled(Settled {
            submission_id: "s1".into(),
            reason: SettleReason::Stop,
            generated_tokens: 5,
        })),
        Verdict::Apply
    );
    assert_eq!(
        ledger.apply(&Event::Settled(Settled {
            submission_id: "s1".into(),
            reason: SettleReason::Stop,
            generated_tokens: 5,
        })),
        Verdict::AlreadySettled
    );
}

/// Every rejection but `Full` ends the submission for good.
///
/// This once used `Full` as its example, which was the wrong one: `Full`
/// says "later", so it releases the id instead of burying it. See
/// `a_full_rejection_frees_the_id_so_a_resend_reaches_the_wire` for that
/// half of the rule -- the two together are the whole of it.
#[test]
fn rejected_is_terminal_and_only_happens_before_accepted() {
    for reason in [
        RejectReason::Conflict,
        RejectReason::Invalid,
        RejectReason::DeploymentClosed,
    ] {
        let mut ledger = Ledger::new(1);
        ledger.begin(submit("s1"));
        assert_eq!(
            ledger.apply(&Event::Rejected(Rejected {
                submission_id: "s1".into(),
                reason,
            })),
            Verdict::Apply
        );
        assert_eq!(
            ledger.state_of("s1"),
            Some(SubmissionState::Done),
            "{reason:?} must be terminal"
        );
    }
}

/// `Full` is the exception, at the ledger's own level.
///
/// The id has to be free again or the caller's retry never reaches the
/// wire -- the client-level proof of that is in `reconnect_tests`; this is
/// the same rule stated where it is actually implemented.
#[test]
fn a_full_rejection_releases_the_id_rather_than_burying_it() {
    let mut ledger = Ledger::new(1);
    ledger.begin(submit("s1"));
    assert_eq!(
        ledger.apply(&Event::Rejected(Rejected {
            submission_id: "s1".into(),
            reason: RejectReason::Full,
        })),
        Verdict::Apply,
        "the caller still has to hear about the rejection"
    );
    assert_eq!(ledger.state_of("s1"), None);
    assert_eq!(
        ledger.begin(submit("s1")),
        Admission::New,
        "a resend after Full is new work, not a duplicate"
    );
}

#[test]
fn unknown_submission_is_refused() {
    let mut ledger = Ledger::new(1);
    assert_eq!(
        ledger.apply(&Event::Accepted(Accepted {
            submission_id: "ghost".into(),
        })),
        Verdict::Unknown
    );
}

#[test]
fn event_from_a_superseded_generation_is_refused() {
    let mut ledger = Ledger::new(1);
    ledger.begin(submit("s1"));
    ledger.advance_generation(2);
    assert_eq!(
        ledger.apply(&Event::Accepted(Accepted {
            submission_id: "s1".into(),
        })),
        Verdict::StaleGeneration
    );
}

#[test]
fn replay_set_excludes_settled_and_superseded_generation_entries() {
    let mut ledger = Ledger::new(1);
    ledger.begin(submit("s1"));
    ledger.begin(submit("s2"));
    ledger.apply(&Event::Settled(Settled {
        submission_id: "s1".into(),
        reason: SettleReason::Stop,
        generated_tokens: 1,
    }));
    ledger.begin(submit("s3"));
    ledger.advance_generation(2);
    ledger.begin(submit_gen("s4", 2));
    let replay = ledger.in_flight_for_replay();
    let ids: Vec<&str> = replay.iter().map(|s| s.submission_id.as_str()).collect();
    // s1 settled, s3 belongs to the superseded generation 1 -- only s4 (the
    // current generation's still-open submission) is replayed. s2 also
    // belongs to generation 1 and is likewise excluded even though it was
    // never settled: it is moot once the generation moved on.
    assert_eq!(ids, vec!["s4"]);
}

#[test]
fn replay_order_is_stable_by_submission_id() {
    let mut ledger = Ledger::new(1);
    ledger.begin(submit("b"));
    ledger.begin(submit("a"));
    ledger.begin(submit("c"));
    let replay = ledger.in_flight_for_replay();
    let ids: Vec<&str> = replay.iter().map(|s| s.submission_id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b", "c"]);
}

/// The ledger has to stop growing.
///
/// Every settled submission stays remembered so a resend of it cannot start
/// a second execution, which is right and which is also why this map only
/// ever grew. A run that keeps starting sessions -- forty an hour, for
/// hours -- turns that into a leak with a slow fuse.
#[test]
fn settled_submissions_are_remembered_but_only_so_many() {
    let mut ledger = Ledger::new(1);
    for index in 0..(TOMBSTONE_CAP + 100) {
        let id = format!("s{index}");
        ledger.begin(submit(&id));
        assert_eq!(
            ledger.apply(&Event::Settled(Settled {
                submission_id: id.clone(),
                reason: SettleReason::Stop,
                generated_tokens: 1,
            })),
            Verdict::Apply
        );
    }

    assert!(
        ledger.tracked() <= TOMBSTONE_CAP,
        "the ledger holds {} entries against a cap of {TOMBSTONE_CAP}",
        ledger.tracked()
    );
    // The recent ones still dedup -- forgetting the oldest is the trade,
    // forgetting everything would not be.
    let recent = format!("s{}", TOMBSTONE_CAP + 99);
    assert_eq!(ledger.begin(submit(&recent)), Admission::AlreadyKnown);
}

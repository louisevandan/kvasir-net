use super::*;

#[test]
fn every_event_variant_names_its_submission() {
    let cases = vec![
        DeploymentEvent::Accepted(Accepted {
            submission_id: "s".into(),
        }),
        DeploymentEvent::Rejected(Rejected {
            submission_id: "s".into(),
            reason: RejectedReason::Full,
        }),
        DeploymentEvent::Produced(Produced {
            submission_id: "s".into(),
            event_ordinal: 0,
            text: "hi".into(),
            generated_tokens: 1,
        }),
        DeploymentEvent::Settled(Settled {
            submission_id: "s".into(),
            reason: SettledReason::Stop,
            generated_tokens: 1,
        }),
    ];
    for event in cases {
        assert_eq!(event.submission_id(), "s");
    }
}

#[test]
fn full_is_distinguished_by_matching_the_enum_not_by_reading_text() {
    let rejected = Rejected {
        submission_id: "s".into(),
        reason: RejectedReason::Full,
    };
    // The only way to tell `Full` apart from the other three reasons is this
    // match -- there is no message field on `Rejected` to search instead.
    let is_backpressure = matches!(rejected.reason, RejectedReason::Full);
    assert!(is_backpressure);
    let conflict = Rejected {
        submission_id: "s".into(),
        reason: RejectedReason::Conflict,
    };
    assert!(!matches!(conflict.reason, RejectedReason::Full));
}

#[test]
fn rejected_reasons_are_four_distinct_values() {
    let reasons = [
        RejectedReason::Full,
        RejectedReason::Conflict,
        RejectedReason::Invalid,
        RejectedReason::DeploymentClosed,
    ];
    for (i, a) in reasons.iter().enumerate() {
        for (j, b) in reasons.iter().enumerate() {
            assert_eq!(a == b, i == j);
        }
    }
}

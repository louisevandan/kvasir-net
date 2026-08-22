use super::*;
use crate::deployment::event::{
    Accepted, Produced, Rejected, RejectedReason, Settled, SettledReason,
};

fn accepted() -> DeploymentEvent {
    DeploymentEvent::Accepted(Accepted {
        submission_id: "s".into(),
    })
}

fn rejected(reason: RejectedReason) -> DeploymentEvent {
    DeploymentEvent::Rejected(Rejected {
        submission_id: "s".into(),
        reason,
    })
}

fn produced(ordinal: u64) -> DeploymentEvent {
    DeploymentEvent::Produced(Produced {
        submission_id: "s".into(),
        event_ordinal: ordinal,
        text: "hi".into(),
        generated_tokens: 1,
    })
}

fn settled(reason: SettledReason) -> DeploymentEvent {
    DeploymentEvent::Settled(Settled {
        submission_id: "s".into(),
        reason,
        generated_tokens: 2,
    })
}

#[test]
fn an_empty_stream_holds_the_contract_trivially() {
    assert_eq!(check(&[]), Ok(()));
}

#[test]
fn accepted_then_contiguous_produced_then_settled_holds() {
    let events = vec![
        accepted(),
        produced(0),
        produced(1),
        produced(2),
        settled(SettledReason::Stop),
    ];
    assert_eq!(check(&events), Ok(()));
}

#[test]
fn a_rejection_with_nothing_else_holds() {
    assert_eq!(check(&[rejected(RejectedReason::Full)]), Ok(()));
}

#[test]
fn a_gap_in_ordinals_is_caught() {
    let events = vec![accepted(), produced(0), produced(2)];
    assert_eq!(
        check(&events),
        Err(Violation::OrdinalGap {
            expected: 1,
            found: 2
        })
    );
}

#[test]
fn a_repeated_ordinal_is_caught_as_a_gap() {
    let events = vec![accepted(), produced(0), produced(0)];
    assert_eq!(
        check(&events),
        Err(Violation::OrdinalGap {
            expected: 1,
            found: 0
        })
    );
}

#[test]
fn a_second_settled_is_caught() {
    let events = vec![
        accepted(),
        settled(SettledReason::Stop),
        settled(SettledReason::Stop),
    ];
    assert_eq!(check(&events), Err(Violation::SettledMoreThanOnce));
}

#[test]
fn a_produced_after_settled_is_caught_as_an_event_after_settled() {
    let events = vec![accepted(), settled(SettledReason::Stop), produced(0)];
    assert_eq!(check(&events), Err(Violation::EventAfterSettled));
}

#[test]
fn a_rejected_after_settled_is_also_caught_as_an_event_after_settled() {
    let events = vec![
        accepted(),
        settled(SettledReason::Stop),
        rejected(RejectedReason::Full),
    ];
    assert_eq!(check(&events), Err(Violation::EventAfterSettled));
}

#[test]
fn a_produced_before_accepted_is_caught() {
    let events = vec![produced(0)];
    assert_eq!(check(&events), Err(Violation::ProducedBeforeAccepted));
}

#[test]
fn accepted_and_rejected_together_are_caught_regardless_of_order() {
    assert_eq!(
        check(&[accepted(), rejected(RejectedReason::Conflict)]),
        Err(Violation::AcceptedAndRejectedBothPresent)
    );
    assert_eq!(
        check(&[rejected(RejectedReason::Conflict), accepted()]),
        Err(Violation::AcceptedAndRejectedBothPresent)
    );
}

#[test]
fn the_first_violation_in_order_is_reported_not_the_last() {
    // A gap comes before the stream ever settles, so that is what must be
    // reported -- not the fact that it later settles twice.
    let events = vec![
        accepted(),
        produced(0),
        produced(5),
        settled(SettledReason::Stop),
        settled(SettledReason::Stop),
    ];
    assert_eq!(
        check(&events),
        Err(Violation::OrdinalGap {
            expected: 1,
            found: 5
        })
    );
}

use super::*;

fn about(action: CacheAction) -> Cache {
    Cache {
        deployment: "d1".into(),
        stage_id: "stage-1".into(),
        generation: 3,
        operation_id: "op-7".into(),
        sequence: "req-7".into(),
        action,
    }
}

#[test]
fn most_actions_leave_state_under_the_id_they_were_given() {
    for action in [
        CacheAction::Persist,
        CacheAction::Restore,
        CacheAction::Discard,
    ] {
        assert_eq!(about(action).subject(), "req-7");
    }
}

/// The one that does not, and the reason this is a method rather than a field
/// read at each call site.
#[test]
fn a_fork_leaves_it_under_the_new_one() {
    let forked = about(CacheAction::Fork {
        into: "req-7-branch-a".into(),
    });
    assert_eq!(forked.subject(), "req-7-branch-a");
    assert_eq!(forked.sequence, "req-7", "and the source is still named");
}

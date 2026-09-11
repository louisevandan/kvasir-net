use super::*;

fn shape() -> ServiceShape {
    ServiceShape {
        prefill_rows: 4,
        decode_rows: 1,
        members: 2,
        last_position: 63,
    }
}
fn sample(id: u64, stage_index: usize, rpc_us: u64) -> ServiceSample {
    ServiceSample {
        load_generation: 1,
        session_id: "s".into(),
        execution_ids: vec![id],
        stage_index,
        shape: shape(),
        rpc_us,
    }
}
fn open(ids: &[u64]) -> BTreeMap<u64, BTreeSet<u64>> {
    ids.iter().map(|id| (*id, BTreeSet::from([*id]))).collect()
}
fn learned(budget: u64) -> ServiceBudget {
    let mut policy = ServiceBudget::new(budget);
    policy.register(sample(1, 0, 10), 1, 3, &open(&[1]));
    policy.observe(&sample(1, 1, 100)).unwrap();
    policy.observe(&sample(1, 2, 20)).unwrap();
    policy
}

#[test]
fn service_budget_counts_already_issued_prefill_and_uses_real_stage_completion() {
    let mut policy = learned(150);
    policy.register(sample(2, 0, 10), 2, 3, &open(&[2]));
    let decision = policy
        .decide(1, "s", 3, &shape(), true, &open(&[2]))
        .unwrap();
    assert_eq!(decision.verdict, ServiceVerdict::DeferPrefill);
    assert_eq!(
        (
            decision.max_pending_us,
            decision.max_with_candidate_us,
            decision.known_stages
        ),
        (100, 200, 3)
    );
    assert_eq!(
        policy
            .decide(1, "s", 3, &shape(), false, &open(&[2]))
            .unwrap()
            .verdict,
        ServiceVerdict::PurePrefill
    );
    policy.observe(&sample(2, 1, 100)).unwrap();
    let decision = policy
        .decide(1, "s", 3, &shape(), true, &open(&[2]))
        .unwrap();
    assert_eq!(decision.verdict, ServiceVerdict::Admit);
    assert_eq!(
        decision.max_pending_us, 20,
        "tail remains in flight despite the middle stage sample"
    );
}

#[test]
fn service_budget_preserves_invalid_duplicate_identity_and_requires_all_costs() {
    let mut policy = learned(150);
    let before = policy.clone();
    assert!(!policy.observe(&sample(1, 1, 100)).unwrap());
    assert!(policy.observe(&sample(1, 1, 101)).is_err());
    let mut changed = sample(1, 1, 100);
    changed.shape.members = 1;
    assert!(policy.observe(&changed).is_err());
    assert_eq!(before, policy);
    let mut long = shape();
    long.last_position = 100000;
    assert_eq!(
        policy
            .decide(1, "s", 3, &long, true, &open(&[]))
            .unwrap()
            .verdict,
        ServiceVerdict::Cold
    );
    assert_eq!(
        policy
            .decide(1, "s", 3, &shape(), true, &open(&[99]))
            .unwrap()
            .verdict,
        ServiceVerdict::Cold
    );
    let mut cold = ServiceBudget::new(150);
    cold.register(sample(1, 0, 10), 1, 3, &open(&[1]));
    let decision = cold.decide(1, "s", 3, &shape(), true, &open(&[])).unwrap();
    assert_eq!(
        (decision.verdict, decision.known_stages),
        (ServiceVerdict::Cold, 1)
    );
}

#[test]
fn service_budget_preserves_prefill_progress_and_bounded_history_across_reuse() {
    let mut policy = learned(1);
    assert_eq!(
        policy
            .decide(1, "s", 3, &shape(), true, &open(&[]))
            .unwrap()
            .verdict,
        ServiceVerdict::ProgressProbe
    );
    for id in 2..600 {
        policy.register(sample(id, 0, 10), id, 3, &open(&[id]));
        policy.observe(&sample(id, 1, 100)).unwrap();
        policy.observe(&sample(id, 2, 20)).unwrap();
    }
    assert_eq!(policy.issued.len(), HISTORY);
    assert!(policy.profiles.len() <= PROFILE);
    assert!(policy.profiles.iter().all(|p| p.recent_us.len() <= 8));
    let before = policy.clone();
    assert!(!policy.observe(&sample(1, 1, 100)).unwrap());
    assert_eq!(
        before, policy,
        "expired optimization history cannot manufacture a sample"
    );
    policy.clear();
    assert_eq!(
        policy
            .decide(1, "s", 3, &shape(), true, &open(&[]))
            .unwrap()
            .verdict,
        ServiceVerdict::Cold
    );
}

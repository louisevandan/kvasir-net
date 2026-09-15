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
fn generation_budget_covers_the_serial_stage_path_not_only_its_largest_stage() {
    let mut policy = ServiceBudget::new(150);
    policy.register(sample(1, 0, 80), 1, 3, &open(&[1]));
    policy.observe(&sample(1, 1, 80)).unwrap();
    policy.observe(&sample(1, 2, 80)).unwrap();
    let decision = policy
        .decide(1, "s", 3, &shape(), true, &open(&[]))
        .unwrap();
    assert_eq!(
        decision.verdict,
        ServiceVerdict::ProgressProbe,
        "three serial 80us services cannot fit a 150us generation budget"
    );
}

#[test]
fn generation_service_calibrated_small_quantum_allows_a_fitting_larger_candidate() {
    let mut policy = ServiceBudget::new(150);
    for stage in 0..3 {
        let full = sample(1, stage, 80);
        if stage == 0 {
            policy.register(full, 1, 3, &open(&[1]));
        } else {
            policy.observe(&full).unwrap();
        }
    }
    for stage in 0..3 {
        let mut small = sample(2, stage, 10);
        small.shape.prefill_rows = 1;
        if stage == 0 {
            policy.register(small, 2, 3, &open(&[2]));
        } else {
            policy.observe(&small).unwrap();
        }
    }
    let mut half = shape();
    half.prefill_rows = 2;
    let decision = policy.decide(1, "s", 3, &half, true, &open(&[])).unwrap();
    assert_eq!(decision.verdict, ServiceVerdict::Admit);
    assert_eq!(decision.predicted_tail_rpc_us, Some(60));
    assert_eq!(
        policy
            .decide(1, "s", 3, &shape(), true, &open(&[]))
            .unwrap()
            .verdict,
        ServiceVerdict::ProgressProbe,
        "measured full shape still exceeds budget"
    );
}

#[test]
fn generation_service_charges_decode_flights_and_does_not_retire_their_authority() {
    let mut policy = learned(150);
    let decode = |id, stage| {
        let mut s = sample(id, stage, 80);
        s.shape.prefill_rows = 0;
        s.shape.members = 1;
        s
    };
    policy.register(decode(2, 0), 2, 3, &open(&[2]));
    policy.observe(&decode(2, 1)).unwrap();
    policy.observe(&decode(2, 2)).unwrap();
    policy.register(decode(3, 0), 3, 3, &open(&[3]));
    let authority = open(&[3]);
    let before = policy.clone();
    let decision = policy
        .decide(1, "s", 3, &shape(), true, &authority)
        .unwrap();
    assert_eq!(decision.predicted_tail_rpc_us, Some(200));
    assert_ne!(decision.verdict, ServiceVerdict::Admit);
    assert_eq!(policy, before);
    assert_eq!(authority, open(&[3]));
}

#[test]
fn generation_service_measures_fixed_cost_instead_of_multiplying_it_per_prefill_row() {
    let mut policy = ServiceBudget::new(50);
    for (id, rows, cost) in [(1, 2, 10), (2, 2, 10), (3, 4, 12), (4, 4, 12)] {
        for stage in 0..3 {
            let mut measured = sample(id, stage, cost);
            measured.shape.prefill_rows = rows;
            if stage == 0 {
                policy.register(measured, id, 3, &open(&[id]));
            } else {
                policy.observe(&measured).unwrap();
            }
        }
    }
    let mut candidate = shape();
    candidate.prefill_rows = 8;
    let chosen = policy
        .decide(1, "s", 3, &candidate, true, &open(&[]))
        .unwrap();
    assert_eq!(chosen.predicted_tail_rpc_us, Some(48));
    assert_eq!(chosen.verdict, ServiceVerdict::Admit);
    candidate.prefill_rows = 16;
    assert_eq!(
        policy
            .decide(1, "s", 3, &candidate, true, &open(&[]))
            .unwrap()
            .verdict,
        ServiceVerdict::ProgressProbe
    );
    assert!(
        policy.has_open_prefill(&open(&[999])),
        "unknown open work cannot grant another calibration probe"
    );
}

#[test]
fn generation_service_calibration_probe_is_one_bounded_unknown_quantum() {
    let mut policy = learned(1);
    assert!(!policy.has_open_calibration_probe(&open(&[1])));
    let mut probe = sample(2, 0, 10);
    probe.shape.prefill_rows = 1;
    policy.register(probe, 2, 3, &open(&[1, 2]));
    assert!(policy.has_open_calibration_probe(&open(&[1, 2])));
    assert!(
        policy.has_open_calibration_probe(&open(&[99])),
        "unknown authority cannot grant a probe"
    );
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

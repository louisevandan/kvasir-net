use super::*;

fn authority() -> IssueAuthority {
    IssueAuthority {
        head: Endpoint::node(Address::tcp("127.0.0.1", 42001), "head", 3),
        outer: OuterEndpoint {
            ingress_agent: Address::tcp("127.0.0.2", 42002),
            channel: "outer-channel".into(),
            connection_generation: 5,
        },
        load_generation: 7,
        session_id: "session-\u{03b1}".into(),
        request_id: "request-a".into(),
        submission_event_id: "outer-event-11".into(),
        sequence_id: 2,
        incarnation: 13,
    }
}

fn row(phase: Phase, position: u32) -> IssuedRow {
    IssuedRow { phase, position }
}

fn execution(execution_id: u64, phase: Phase, positions: &[u32]) -> IssuedExecution {
    IssuedExecution {
        execution_id,
        rows: positions
            .iter()
            .map(|position| row(phase, *position))
            .collect(),
    }
}

fn first_work() -> IssuedWork {
    IssuedWork {
        logical_ordinal: 1,
        executions: vec![
            execution(12, Phase::Prefill, &[1]),
            execution(11, Phase::Prefill, &[2, 0]),
        ],
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn canonical_bytes_and_digests_match_independent_literal_node_vectors() {
    // This checked-in oracle is emitted by generate_vectors.mjs using Node's
    // standard crypto implementation and manually declared canonical entries.
    // Rust never generates or updates the expected bytes or digest.
    let all: serde_json::Value = serde_json::from_str(include_str!("vectors-v1.json")).unwrap();
    assert_eq!(all["format"], 1);
    let vector = &all["unit"];
    let authority = authority();
    assert_eq!(
        hex(&canonical_authority_bytes(&authority).unwrap()),
        vector["authority_hex"]
    );
    let mut witness = IssueWitness::new(&authority).unwrap();
    assert_eq!(hex(&witness.authority_digest()), vector["authority_digest"]);
    assert_eq!(hex(&witness.digest()), vector["initial_digest"]);
    let works = [
        first_work(),
        IssuedWork {
            logical_ordinal: 9,
            executions: vec![execution(22, Phase::Verify, &[4, 3])],
        },
        IssuedWork {
            logical_ordinal: 12,
            executions: vec![execution(23, Phase::Replay, &[3, 4])],
        },
    ];
    for (work, expected) in works.iter().zip(vector["steps"].as_array().unwrap()) {
        assert_eq!(
            hex(&witness.canonical_work_bytes(&authority, work).unwrap()),
            expected["canonical_hex"]
        );
        witness = witness.advanced(&authority, work).unwrap();
        assert_eq!(hex(&witness.digest()), expected["digest"]);
        assert_eq!(
            witness.issue_count(),
            expected["issue_count"].as_u64().unwrap()
        );
        assert_eq!(
            witness.last_ordinal(),
            expected["logical_ordinal"].as_u64().unwrap()
        );
    }
}

#[test]
fn the_standard_digest_matches_independent_known_vectors() {
    for (input, digest) in [
        (
            b"".as_slice(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc".as_slice(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
    ] {
        assert_eq!(hex(&Sha256::digest(input)), digest);
    }
}

#[test]
fn terminal_wire_snapshot_exports_the_exact_independent_chain_without_advancing_it() {
    let authority = authority();
    let initial = IssueWitness::new(&authority).unwrap();
    assert!(
        initial.proof().validate().is_err(),
        "an unissued seed cannot prove terminal work"
    );
    let witness = initial.advanced(&authority, &first_work()).unwrap();
    let before = witness;
    let proof = witness.proof();
    proof.validate().unwrap();
    let vectors: serde_json::Value = serde_json::from_str(include_str!("vectors-v1.json")).unwrap();
    assert_eq!(
        hex(&proof.authority_digest),
        vectors["unit"]["authority_digest"]
    );
    assert_eq!(hex(&proof.digest), vectors["unit"]["steps"][0]["digest"]);
    assert_eq!(proof.issue_count, 1);
    assert_eq!(proof.last_ordinal, 1);
    assert_eq!(witness, before);
    let wire = serde_json::to_vec(&proof).unwrap();
    assert_eq!(
        serde_json::from_slice::<IssuedWorkProof>(&wire).unwrap(),
        proof
    );
    for phase in [Phase::Prefill, Phase::Decode, Phase::Verify, Phase::Replay] {
        let row = IssuedRow {
            phase,
            position: 19,
        };
        let bytes = serde_json::to_vec(&row).unwrap();
        assert_eq!(serde_json::from_slice::<IssuedRow>(&bytes).unwrap(), row);
    }
    for bad in [
        r#"{"phase":"future","position":19}"#,
        r#"{"phase":"decode","position":19,"token":9}"#,
        r#"{"phase":"decode","position":19,"position":20}"#,
    ] {
        assert!(serde_json::from_str::<IssuedRow>(bad).is_err());
    }
}

#[test]
fn one_logical_issue_counts_once_despite_a_physical_split() {
    let authority = authority();
    let original = IssueWitness::new(&authority).unwrap();
    assert_eq!((original.issue_count(), original.last_ordinal()), (0, 0));
    assert_eq!(original.digest(), original.authority_digest());
    let issued = original.advanced(&authority, &first_work()).unwrap();
    assert_eq!((issued.issue_count(), issued.last_ordinal()), (1, 1));
    assert_ne!(issued.digest(), original.digest());
    assert_eq!(
        original.issue_count(),
        0,
        "advancing creates a candidate, not a mutation"
    );
}

#[test]
fn input_permutations_have_one_encoding_without_changing_the_inputs() {
    let authority = authority();
    let witness = IssueWitness::new(&authority).unwrap();
    let work = first_work();
    let before = work.clone();
    let mut permuted = work.clone();
    permuted.executions.reverse();
    for execution in &mut permuted.executions {
        execution.rows.reverse();
    }
    assert_eq!(
        witness.advanced(&authority, &work),
        witness.advanced(&authority, &permuted)
    );
    assert_eq!(
        witness.canonical_work_bytes(&authority, &work),
        witness.canonical_work_bytes(&authority, &permuted),
    );
    assert_eq!(work, before);
}

#[test]
fn equal_counts_cannot_hide_missing_replaced_or_redistributed_membership() {
    let authority = authority();
    let witness = IssueWitness::new(&authority).unwrap();
    let work = first_work();
    let normal = witness.advanced(&authority, &work).unwrap();
    let mut changes = Vec::new();
    let mut changed = work.clone();
    changed.executions.remove(0);
    changes.push(changed);
    let mut changed = work.clone();
    changed.executions[0].execution_id = 999;
    changes.push(changed);
    let mut changed = work.clone();
    changed.executions[0].rows[0].position = 9;
    changes.push(changed);
    let mut changed = work.clone();
    changed.executions[0].rows[0].phase = Phase::Decode;
    changes.push(changed);
    let mut changed = work.clone();
    changed.executions[0].rows[0].position = 0;
    changed.executions[1].rows[1].position = 1;
    changes.push(changed);
    for changed in changes {
        let different = witness.advanced(&authority, &changed).unwrap();
        assert_eq!(different.issue_count(), 1);
        assert_ne!(different.digest(), normal.digest(), "{changed:?}");
    }
}

#[test]
fn exact_positions_distinguish_equal_minimum_maximum_and_row_count() {
    let authority = authority();
    let witness = IssueWitness::new(&authority).unwrap();
    let first = IssuedWork {
        logical_ordinal: 1,
        executions: vec![execution(11, Phase::Prefill, &[0, 1, 4])],
    };
    let second = IssuedWork {
        logical_ordinal: 1,
        executions: vec![execution(11, Phase::Prefill, &[0, 3, 4])],
    };
    assert_ne!(
        witness.advanced(&authority, &first).unwrap(),
        witness.advanced(&authority, &second).unwrap()
    );
}

#[test]
fn ordinal_gaps_and_later_verify_replay_position_reuse_are_legal() {
    let authority = authority();
    let start = IssueWitness::new(&authority).unwrap();
    let first = start.advanced(&authority, &first_work()).unwrap();
    let verify = IssuedWork {
        logical_ordinal: 9,
        executions: vec![execution(22, Phase::Verify, &[3, 4])],
    };
    let replay = IssuedWork {
        logical_ordinal: 12,
        executions: vec![execution(23, Phase::Replay, &[3, 4])],
    };
    let second = first.advanced(&authority, &verify).unwrap();
    let third = second.advanced(&authority, &replay).unwrap();
    assert_eq!((third.issue_count(), third.last_ordinal()), (3, 12));
    let another_verify = IssuedWork {
        logical_ordinal: 15,
        executions: vec![execution(24, Phase::Verify, &[3, 4])],
    };
    assert!(third.advanced(&authority, &another_verify).is_ok());
    for ordinal in [0, 1, 8, 9] {
        let mut invalid = replay.clone();
        invalid.logical_ordinal = ordinal;
        assert_eq!(
            second.advanced(&authority, &invalid),
            Err("issued-work ordinal must strictly advance")
        );
    }
    assert_eq!((second.issue_count(), second.last_ordinal()), (2, 9));
}

#[test]
fn malformed_work_is_rejected_without_changing_the_original() {
    let authority = authority();
    let witness = IssueWitness::new(&authority).unwrap();
    let before = witness;
    let mut invalids = Vec::new();
    let mut work = first_work();
    work.executions.clear();
    invalids.push(work);
    let mut work = first_work();
    work.executions[0].execution_id = 0;
    invalids.push(work);
    let mut work = first_work();
    work.executions[0].execution_id = 11;
    invalids.push(work);
    let mut work = first_work();
    work.executions[0].rows.clear();
    invalids.push(work);
    let mut work = first_work();
    work.executions[1].rows.push(row(Phase::Prefill, 2));
    invalids.push(work);
    let mut work = first_work();
    work.executions[0].rows[0].position = 2;
    invalids.push(work);
    for work in invalids {
        assert!(witness.advanced(&authority, &work).is_err(), "{work:?}");
        assert!(witness.canonical_work_bytes(&authority, &work).is_err());
        assert_eq!(witness, before);
    }
}

#[test]
fn every_authority_field_is_bound_before_an_advance() {
    let authority = authority();
    let witness = IssueWitness::new(&authority).unwrap();
    let mut mutations: Vec<(&str, IssueAuthority)> = Vec::new();
    macro_rules! changed {
        ($name:literal, $change:expr) => {{
            let mut altered = authority.clone();
            ($change)(&mut altered);
            mutations.push(($name, altered));
        }};
    }
    changed!(
        "head address host",
        |a: &mut IssueAuthority| if let Endpoint::Node { agent, .. } = &mut a.head {
            agent.host = "127.0.0.9".into()
        }
    );
    changed!(
        "head address port",
        |a: &mut IssueAuthority| if let Endpoint::Node { agent, .. } = &mut a.head {
            agent.port += 1
        }
    );
    changed!(
        "head name",
        |a: &mut IssueAuthority| if let Endpoint::Node { node, .. } = &mut a.head {
            *node = "another-head".into()
        }
    );
    changed!(
        "head generation",
        |a: &mut IssueAuthority| if let Endpoint::Node { generation, .. } = &mut a.head {
            *generation += 1
        }
    );
    changed!("outer address host", |a: &mut IssueAuthority| a
        .outer
        .ingress_agent
        .host =
        "127.0.0.9".into());
    changed!("outer address port", |a: &mut IssueAuthority| a
        .outer
        .ingress_agent
        .port +=
        1);
    changed!("outer channel", |a: &mut IssueAuthority| a.outer.channel =
        "another-channel".into());
    changed!("connection generation", |a: &mut IssueAuthority| a
        .outer
        .connection_generation +=
        1);
    changed!("load", |a: &mut IssueAuthority| a.load_generation += 1);
    changed!("session", |a: &mut IssueAuthority| a.session_id =
        "another-session".into());
    changed!("request", |a: &mut IssueAuthority| a.request_id =
        "another-request".into());
    changed!("submission", |a: &mut IssueAuthority| a
        .submission_event_id =
        "another-submission".into());
    changed!("slot", |a: &mut IssueAuthority| a.sequence_id += 1);
    changed!("incarnation", |a: &mut IssueAuthority| a.incarnation += 1);
    let mut seeds = BTreeSet::new();
    seeds.insert(witness.authority_digest());
    for (field, altered) in mutations {
        assert_eq!(
            witness.advanced(&altered, &first_work()),
            Err("issued-work authority differs from the admitted attempt"),
            "{field}"
        );
        let alternative = IssueWitness::new(&altered).unwrap();
        assert!(
            seeds.insert(alternative.authority_digest()),
            "{field} was not in the encoding"
        );
    }
    assert_eq!(seeds.len(), 15);
}

#[test]
fn invalid_endpoints_strings_and_zero_generations_cannot_seed_a_witness() {
    let original = authority();
    let mut invalids = Vec::new();
    for kind in [
        Endpoint::agent(Address::tcp("127.0.0.1", 42001)),
        Endpoint::Outer(original.outer.clone()),
    ] {
        let mut invalid = original.clone();
        invalid.head = kind;
        invalids.push(invalid);
    }
    for value in ["", "bad\0identity"] {
        for field in 0..7 {
            let mut invalid = original.clone();
            match field {
                0 => {
                    if let Endpoint::Node { agent, .. } = &mut invalid.head {
                        agent.host = value.into();
                    }
                }
                1 => {
                    if let Endpoint::Node { node, .. } = &mut invalid.head {
                        *node = value.into();
                    }
                }
                2 => invalid.outer.ingress_agent.host = value.into(),
                3 => invalid.outer.channel = value.into(),
                4 => invalid.session_id = value.into(),
                5 => invalid.request_id = value.into(),
                6 => invalid.submission_event_id = value.into(),
                _ => unreachable!(),
            }
            invalids.push(invalid);
        }
    }
    for field in 0..6 {
        let mut invalid = original.clone();
        match field {
            0 => {
                if let Endpoint::Node { agent, .. } = &mut invalid.head {
                    agent.port = 0;
                }
            }
            1 => {
                if let Endpoint::Node { generation, .. } = &mut invalid.head {
                    *generation = 0;
                }
            }
            2 => invalid.outer.ingress_agent.port = 0,
            3 => invalid.outer.connection_generation = 0,
            4 => invalid.load_generation = 0,
            5 => invalid.incarnation = 0,
            _ => unreachable!(),
        }
        invalids.push(invalid);
    }
    let witness = IssueWitness::new(&original).unwrap();
    for invalid in invalids {
        assert!(IssueWitness::new(&invalid).is_err(), "{invalid:?}");
        assert!(canonical_authority_bytes(&invalid).is_err());
        assert!(witness.advanced(&invalid, &first_work()).is_err());
    }
}

#[test]
fn typed_address_round_trip_is_canonical_without_guessing_dns_equivalence() {
    let authority = authority();
    let mut parsed = authority.clone();
    if let Endpoint::Node { agent, .. } = &mut parsed.head {
        *agent = Address::from_str("tcp://127.0.0.1:42001").unwrap();
    }
    parsed.outer.ingress_agent = Address::from_str("tcp://127.0.0.2:42002").unwrap();
    assert_eq!(IssueWitness::new(&authority), IssueWitness::new(&parsed));
    let mut ipv6 = authority;
    if let Endpoint::Node { agent, .. } = &mut ipv6.head {
        *agent = Address::from_str("tcp://[fe80::1]:42001").unwrap();
    }
    assert!(IssueWitness::new(&ipv6).is_ok());
}

#[test]
fn counters_fail_closed_and_the_witness_has_no_history_storage() {
    fn requires_copy<T: Copy>(_: T) {}
    let authority = authority();
    let witness = IssueWitness::new(&authority).unwrap();
    requires_copy(witness);
    assert_eq!(std::mem::size_of::<IssueWitness>(), 80);
    let full = witness.with_test_counters(u64::MAX, 4);
    let mut work = first_work();
    work.logical_ordinal = 5;
    assert_eq!(
        full.advanced(&authority, &work),
        Err("issued-work count exhausted")
    );
    assert_eq!(full.issue_count(), u64::MAX);
    let mut work = first_work();
    work.logical_ordinal = u64::MAX;
    let last = witness.advanced(&authority, &work).unwrap();
    assert_eq!((last.issue_count(), last.last_ordinal()), (1, u64::MAX));
    assert_eq!(
        last.advanced(&authority, &work),
        Err("issued-work ordinal must strictly advance")
    );
}

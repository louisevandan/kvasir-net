//! Actual worker producer/owner boundary. Expected routes come from submitted
//! PREFILLs, membership from native-bound PHYSICALs, and operation IDs from the
//! head's issued RELEASE, never from the returning ACK/receipt under test.
use super::*;
use crate::v2::{
    ApprovedOutputPayload, RELEASE_RECEIPT_CONTENT_TYPE, ReleaseMember, ReleaseReceipt,
};

pub(super) fn assert_complete(h: &Harness) {
    super::observation_contract::assert_terminal_proofs(h);
    let mut submitted = BTreeMap::new();
    for input in &h.submissions {
        let command: InferenceCommand = serde_json::from_slice(&input.payload).unwrap();
        assert!(
            submitted
                .insert(command.request_id.clone(), (command, input))
                .is_none()
        );
    }
    let mut physical = BTreeMap::new();
    let mut issued = BTreeMap::new();
    for event in &h.stage_events {
        if event.envelope.source != endpoint(0) {
            continue;
        }
        if event.envelope.payload_content_type == PHYSICAL_BATCH_CONTENT_TYPE {
            for capsule in CapsuleSet::decode(&event.payload).unwrap().0 {
                for owner in capsule.owners {
                    let identity = (owner.sequence_id, owner.incarnation);
                    if let Some(previous) = physical.insert(owner.request_id, identity) {
                        assert_eq!(previous, identity, "request changed physical incarnation");
                    }
                }
            }
        } else if event.envelope.payload_content_type == RELEASE_CONTENT_TYPE {
            let command: ReleaseCommand = serde_json::from_slice(&event.payload).unwrap();
            for sequence in command.sequences {
                let request = sequence
                    .key
                    .strip_prefix("loop-session\0")
                    .unwrap()
                    .to_owned();
                assert!(
                    issued.insert(request, sequence).is_none(),
                    "head issued a release twice"
                );
            }
        }
    }
    assert_eq!(issued.len(), submitted.len());
    let mut terminal = BTreeMap::new();
    let mut terminal_index = BTreeMap::new();
    for (index, event) in h
        .received
        .iter()
        .enumerate()
        .filter(|(_, event)| event.envelope.payload_content_type == OUTPUT_CONTENT_TYPE)
    {
        let approved: ApprovedOutputPayload = serde_json::from_slice(&event.payload).unwrap();
        approved.validate().unwrap();
        let outcome = &approved.outcome;
        let (command, original) = &submitted[&outcome.request_id];
        assert_eq!(outcome.load_generation, command.load_generation);
        assert_eq!(outcome.session_id, command.session_id);
        assert_eq!(approved.submission_event_id, original.envelope.event_id);
        assert_eq!(
            (outcome.sequence_id, approved.incarnation),
            physical[&outcome.request_id]
        );
        assert_owner(event, original);
        if outcome.stop.is_some() {
            let sequence = &issued[&outcome.request_id];
            assert_eq!(sequence.id, outcome.sequence_id);
            assert_eq!(sequence.incarnation, approved.incarnation);
            assert_eq!(approved.release_operation_id, Some(sequence.operation_id));
            let member = ReleaseMember {
                request_id: outcome.request_id.clone(),
                submission_event_id: original.envelope.event_id.clone(),
                sequence_id: outcome.sequence_id,
                incarnation: approved.incarnation,
                operation_id: sequence.operation_id,
            };
            assert!(
                terminal
                    .insert(outcome.request_id.clone(), member)
                    .is_none()
            );
            terminal_index.insert(outcome.request_id.clone(), index);
        }
    }
    assert_eq!(
        terminal.len(),
        submitted.len(),
        "every request needs one terminal OUTPUT"
    );
    let mut seen = BTreeMap::new();
    let mut event_ids = std::collections::BTreeSet::new();
    let mut last_sequence = None;
    for event in h.received.iter().filter(|event| {
        matches!(
            event.envelope.payload_content_type.as_str(),
            OUTPUT_CONTENT_TYPE | RELEASE_RECEIPT_CONTENT_TYPE
        )
    }) {
        assert!(
            event_ids.insert(&event.envelope.event_id),
            "OUTPUT/receipt event ID repeated"
        );
        if let Some(previous) = last_sequence {
            assert!(
                event.envelope.sequence > previous,
                "head publication sequence regressed"
            );
        }
        last_sequence = Some(event.envelope.sequence);
    }
    for (index, event) in
        h.received.iter().enumerate().filter(|(_, event)| {
            event.envelope.payload_content_type == RELEASE_RECEIPT_CONTENT_TYPE
        })
    {
        let receipt: ReleaseReceipt = serde_json::from_slice(&event.payload).unwrap();
        receipt.validate().unwrap();
        assert_eq!(receipt.load_generation, 1);
        assert_eq!(receipt.session_id, "loop-session");
        for member in receipt.members {
            assert!(
                index > terminal_index[&member.request_id],
                "receipt must follow its terminal OUTPUT on the actual mailbox"
            );
            assert_eq!(&member, &terminal[&member.request_id]);
            assert_owner(event, submitted[&member.request_id].1);
            assert!(
                seen.insert(member.request_id.clone(), member).is_none(),
                "release notification repeated"
            );
        }
    }
    assert_eq!(seen, terminal);
    // Decode the independent fake's raw P4ID call log; no production codec or
    // transition helper supplies these expectations. Every stage did the same
    // issued operation exactly once, in addition to existing exact KV oracles.
    for node in &h.nodes {
        let native = node.native.lock().unwrap();
        let mut native_members = BTreeMap::new();
        for body in &native.release_bodies {
            assert_eq!(&body[..8], b"P4ID\x01\x00\x00\x00");
            assert_eq!(u64::from_le_bytes(body[8..16].try_into().unwrap()), 1);
            let incarnation = u64::from_le_bytes(body[16..24].try_into().unwrap());
            let operation = u64::from_le_bytes(body[24..32].try_into().unwrap());
            let slot = u32::from_le_bytes(body[32..36].try_into().unwrap());
            let session_len = u32::from_le_bytes(body[36..40].try_into().unwrap()) as usize;
            assert_eq!(&body[40..40 + session_len], b"loop-session");
            let at = 40 + session_len;
            let key_len = u32::from_le_bytes(body[at..at + 4].try_into().unwrap()) as usize;
            assert_eq!(body.len(), at + 4 + key_len);
            let key = std::str::from_utf8(&body[at + 4..]).unwrap();
            let request = key.strip_prefix("loop-session\0").unwrap();
            assert!(
                native_members
                    .insert(request.to_owned(), (slot, incarnation, operation))
                    .is_none()
            );
        }
        let expected: BTreeMap<_, _> = issued
            .iter()
            .map(|(request, sequence)| {
                (
                    request.clone(),
                    (sequence.id, sequence.incarnation, sequence.operation_id),
                )
            })
            .collect();
        assert_eq!(native_members, expected);
    }
}

fn assert_owner(event: &Event, original: &Event) {
    let route = original.envelope.return_route.clone().unwrap();
    assert_eq!(event.envelope.source, endpoint(0));
    assert_eq!(event.envelope.target, Endpoint::Outer(route.clone()));
    assert_eq!(event.envelope.return_route, Some(route));
    assert_eq!(
        event.envelope.correlation_id,
        original.envelope.correlation_id
    );
    assert_eq!(
        event.envelope.deadline_unix_ms,
        original.envelope.deadline_unix_ms
    );
}

fn owner_input(
    name: &str,
    port: u16,
    channel: &str,
    generation: u64,
    correlation: &str,
) -> (InferenceCommand, Event) {
    let command = request(name, 1, 1);
    let mut event = submission_event(
        &command,
        1,
        OuterEndpoint {
            ingress_agent: Address::tcp("127.0.0.1", port),
            channel: channel.into(),
            connection_generation: generation,
        },
    );
    event.envelope.correlation_id = correlation.into();
    (command, event)
}

fn multi_owner(stages: usize, capacity: usize) -> (Harness, Vec<InferenceCommand>) {
    let (a, input_a) = owner_input("release-owner-a", 42991, "owner-a", 11, "correlation-a");
    let (b, input_b) = owner_input("release-owner-b", 42992, "owner-b", 12, "correlation-b");
    (
        Harness::configured_events(stages, 1, capacity, &[input_a, input_b], 0, None),
        vec![a, b],
    )
}

fn assert_one_mixed_terminal(h: &mut Harness) {
    h.hold_tail = true;
    h.until("both owners finish in one physical terminal", |h| {
        !h.held_tail.is_empty()
    });
    assert_eq!(h.held_tail.len(), 1);
    let event = &h.held_tail[0];
    assert_eq!(event.envelope.source, endpoint(h.nodes.len() - 1));
    assert_eq!(event.envelope.target, endpoint(0));
    let set = CapsuleSet::decode(&event.payload).unwrap();
    assert_eq!(set.0.len(), 1, "same physical, not just logical, batch");
    let capsule = &set.0[0];
    assert!(capsule.terminal);
    assert_eq!(capsule.owners.len(), 2);
    assert_eq!(capsule.outcomes.len(), 2);
    let mut names: Vec<_> = capsule
        .outcomes
        .iter()
        .map(|outcome| {
            assert_eq!(outcome.generated.len(), 1);
            assert_eq!(outcome.generated[0].stop.as_deref(), Some("length"));
            capsule.owners[outcome.owner_index as usize]
                .request_id
                .as_str()
        })
        .collect();
    names.sort();
    assert_eq!(names, ["release-owner-a", "release-owner-b"]);
}

#[test]
fn release_receipt_single_outer_is_the_positive_control() {
    let (command, input) = owner_input("release-owner-a", 42991, "owner-a", 11, "correlation-a");
    let mut h = Harness::configured_events(2, 1, 8, &[input], 0, None);
    h.finish(&[command]);
    assert_complete(&h);
    assert_eq!(
        h.received
            .iter()
            .filter(|event| event.envelope.payload_content_type == RELEASE_RECEIPT_CONTENT_TYPE)
            .count(),
        1
    );
}

#[test]
fn release_receipts_retain_each_outer_owner_after_one_mixed_physical_terminal() {
    for stages in [2, 4] {
        let (mut h, commands) = multi_owner(stages, 8);
        assert_one_mixed_terminal(&mut h);
        h.resume_tail();
        h.finish(&commands);
        assert_complete(&h);
        let outputs: Vec<_> = h
            .received
            .iter()
            .filter(|event| event.envelope.payload_content_type == OUTPUT_CONTENT_TYPE)
            .cloned()
            .collect();
        super::output_contract::assert_live_matches(
            &format!("mixed-owner-{stages}"),
            &h.submissions,
            &outputs,
            &h.received,
        );
        super::output_contract::assert_live_prefill_counts(
            &format!("mixed-owner-{stages}"),
            &h.received,
        );
        assert_eq!(
            h.received
                .iter()
                .filter(|event| event.envelope.payload_content_type == RELEASE_RECEIPT_CONTENT_TYPE)
                .count(),
            2
        );
    }
}

#[test]
fn mixed_outer_sender_shaped_inputs_preserve_the_same_physical_and_release_oracles() {
    for stages in [2, 4] {
        // This is an additional original input, not an edited capture. The
        // previous correlation-a/b cases remain to prove independent routing;
        // event-drive's actual Sender uses request_id as its correlation.
        let (a, input_a) = owner_input("release-owner-a", 42991, "owner-a", 11, "release-owner-a");
        let (b, input_b) = owner_input("release-owner-b", 42992, "owner-b", 12, "release-owner-b");
        let mut h = Harness::configured_events(stages, 1, 8, &[input_a, input_b], 0, None);
        assert_one_mixed_terminal(&mut h);
        h.resume_tail();
        h.finish(&[a, b]);
        assert_complete(&h);
        let outputs: Vec<_> = h
            .received
            .iter()
            .filter(|event| event.envelope.payload_content_type == OUTPUT_CONTENT_TYPE)
            .cloned()
            .collect();
        let case = format!("mixed-consumer-{stages}");
        super::output_contract::assert_live_matches(&case, &h.submissions, &outputs, &h.received);
        super::output_contract::assert_live_prefill_counts(&case, &h.received);
    }
}

#[test]
fn release_receipt_full_before_first_and_after_first_resumes_without_native_repetition() {
    for before_first in [true, false] {
        let (mut h, commands) = multi_owner(2, 1);
        assert_one_mixed_terminal(&mut h);
        h.hold_control = Some((RELEASED_CONTENT_TYPE.into(), 0));
        h.resume_tail();
        h.until(
            "native release chain completes with its ACK withheld",
            |h| !h.held_control.is_empty() && h.outputs.len() == 2,
        );
        assert_eq!(h.held_control.len(), 1);
        assert!(!h.nodes[0].thread.as_ref().unwrap().is_finished());
        assert!(
            h.pending
                .iter()
                .all(|event| event.envelope.target != endpoint(0))
        );
        assert!(h.held_tail.is_empty());
        assert_eq!(h.nodes[0].mailbox.try_take(), Poll::Empty);
        // Status strings are sticky observations, not a queue epoch. Earlier
        // capacity-one work may have left the same Full string behind. Reset
        // only this test's observation baseline while the head awaits its held
        // ACK; do not mutate request/ledger/native/queue state to obtain Full.
        *h.nodes[0].snapshot.lock().unwrap() = "fixture:awaiting_receipt_publication".into();
        let native_before: Vec<_> = h
            .nodes
            .iter()
            .map(|node| {
                let native = node.native.lock().unwrap();
                assert!(native.live.is_empty());
                (
                    native.logical_calls,
                    native.physical_calls,
                    native.sampler_calls,
                    native.release_bodies.clone(),
                )
            })
            .collect();
        h.paused_completions[0] = true;
        if before_first {
            // The only producer remains the real worker. An exact valid SESSION
            // retransmission fills capacity one with SESSION_READY before ACK
            // processing; no test publisher invents a native/completion result.
            let session = SessionCommand {
                load_generation: 1,
                session_id: "loop-session".into(),
                stages: (0..2).map(node_address).collect(),
                stage_index: 0,
            };
            h.nodes[0]
                .sender
                .as_ref()
                .unwrap()
                .try_send(WorkerInput::Event(event_wire(event(
                    0,
                    "receipt-capacity-filler",
                    SESSION_CONTENT_TYPE,
                    serde_json::to_vec(&session).unwrap(),
                ))))
                .unwrap_or_else(|_| panic!("valid SESSION fixture must fit input"));
        }
        let acknowledgement = h.held_control.pop_front().unwrap();
        h.nodes[0]
            .sender
            .as_ref()
            .unwrap()
            .try_send(WorkerInput::Event(event_wire(acknowledgement)))
            .unwrap_or_else(|_| panic!("genuine ACK fixture must fit input"));
        let deadline = Instant::now() + Duration::from_secs(2);
        while h.nodes[0].snapshot.lock().unwrap().as_str() != "completion_queue_full:waiting" {
            assert!(
                Instant::now() < deadline,
                "the requested receipt publication must encounter Full"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!h.nodes[0].thread.as_ref().unwrap().is_finished());
        let Poll::Event(first) = h.nodes[0].mailbox.try_take() else {
            panic!("Full must have an actual queued event");
        };
        let first = event_wire(first);
        assert_eq!(
            first.envelope.payload_content_type,
            if before_first {
                SESSION_READY_CONTENT_TYPE
            } else {
                RELEASE_RECEIPT_CONTENT_TYPE
            }
        );
        h.received.push(first);
        h.paused_completions[0] = false;
        h.hold_control = None;
        h.finish(&commands);
        assert_complete(&h);
        let native_after: Vec<_> = h
            .nodes
            .iter()
            .map(|node| {
                let native = node.native.lock().unwrap();
                (
                    native.logical_calls,
                    native.physical_calls,
                    native.sampler_calls,
                    native.release_bodies.clone(),
                )
            })
            .collect();
        assert_eq!(
            native_after, native_before,
            "notification backpressure cannot repeat computation or native release"
        );
    }
}

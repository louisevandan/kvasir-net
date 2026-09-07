//! The real Worker::run must consume a genuine RELEASED while unrelated output
//! is backpressured. This is a local actor/mailbox test, not EventNode/broker,
//! network drain, a Cancel command, or a fully saturated cyclic-network proof.
use super::*;
use crate::v2::RELEASE_RECEIPT_CONTENT_TYPE;
use std::sync::Condvar;

#[derive(Clone, Debug)]
struct ReleaseView {
    point: &'static str,
    pending: BTreeMap<String, (u32, u64, u64)>,
    free: Vec<u32>,
    requests: Vec<String>,
}

type Views = Arc<(Mutex<Vec<ReleaseView>>, Condvar)>;

fn observe_releases() -> (IssueObserver, Views) {
    let views = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
    let observed = Arc::clone(&views);
    let observer: IssueObserver = Arc::new(move |point, state| {
        let view = ReleaseView {
            point,
            pending: state
                .pending_releases
                .iter()
                .map(|(key, pending)| {
                    (
                        key.clone(),
                        (
                            pending.sequence.id,
                            pending.sequence.incarnation,
                            pending.sequence.operation_id,
                        ),
                    )
                })
                .collect(),
            free: state.free_sequences.iter().copied().collect(),
            requests: state.requests.keys().cloned().collect(),
        };
        let (history, changed) = &*observed;
        let mut history = history.lock().unwrap();
        assert!(history.len() < 128, "bounded read-only test observation");
        history.push(view);
        changed.notify_all();
    });
    (observer, views)
}

fn committed_a(views: &[ReleaseView], key: &str, slot: u32) -> bool {
    views.iter().any(|view| {
        view.point == "after_release_committed"
            && !view.pending.contains_key(key)
            && view.free.contains(&slot)
    })
}

struct FullRelease {
    h: Harness,
    a: InferenceCommand,
    b: InferenceCommand,
    acknowledgement: Event,
    acknowledgement_bytes: Vec<u8>,
    views: Views,
    key_a: String,
    slot_a: u32,
}

// Every case establishes Full from the same genuine native result, rather
// than synthesizing a filler or constructing pending request/control state.
fn full_with_pending_release() -> FullRelease {
    let a = request("ack-before-output-space", 1, 1);
    let b = request("output-blocks-the-actor", 1, 1);
    let input_a = submission_event(&a, 1, default_route());
    let (observer, views) = observe_releases();
    let mut h = Harness::observed_events(2, 1, 1, &[input_a], 0, None, Some(observer), None);

    h.hold_control = Some((RELEASED_CONTENT_TYPE.into(), 0));
    h.until(
        "A completed natively and its authentic final ACK is held",
        |h| h.outputs.len() == 1 && h.held_control.len() == 1,
    );
    let acknowledgement = h.held_control.pop_front().unwrap();
    let acknowledgement_bytes = p4_protocol::event::encode(&acknowledgement).unwrap();
    assert_eq!(acknowledgement.envelope.source, endpoint(1));
    assert_eq!(acknowledgement.envelope.target, endpoint(0));
    let command: ReleaseCommand = serde_json::from_slice(&acknowledgement.payload).unwrap();
    assert_eq!(command.sequences.len(), 1);
    let released_a = &command.sequences[0];
    let key_a = request_key("loop-session", &a.request_id);
    assert_eq!(released_a.key, key_a);
    let slot_a = released_a.id;
    for node in &h.nodes {
        let native = node.native.lock().unwrap();
        assert!(native.live.is_empty());
        assert_eq!(native.releases.len(), 1);
        assert_eq!(native.releases.values().copied().collect::<Vec<_>>(), [1]);
    }
    assert!(
        h.received
            .iter()
            .all(|event| { event.envelope.payload_content_type != RELEASE_RECEIPT_CONTENT_TYPE })
    );

    // B is submitted after A's native release, but before A's head ACK. It must
    // therefore use a different slot; we do not manufacture request/flight state.
    h.hold_tail = true;
    h.enqueue(&b);
    h.until(
        "B's real terminal result is held before head approval",
        |h| h.held_tail.len() == 1,
    );
    let terminal_b = h.held_tail.pop_front().unwrap();
    let set = CapsuleSet::decode(&terminal_b.payload).unwrap();
    assert_eq!(set.0.len(), 1);
    assert_eq!(set.0[0].owners.len(), 1);
    assert_eq!(set.0[0].owners[0].request_id, b.request_id);
    assert_ne!(set.0[0].owners[0].sequence_id, slot_a);
    assert_eq!(set.0[0].outcomes[0].generated[0].token, 1000);

    // Neither held return has been offered. Let already emitted head telemetry
    // drain, then clear only the sticky observational status to distinguish a
    // new Full from earlier capacity-one pressure. No publisher or state helper
    // injects a synthetic filler into the completion queue.
    h.pump_for(Duration::from_millis(20));
    assert!(h.pending.is_empty());
    assert_eq!(h.nodes[0].mailbox.try_take(), Poll::Empty);
    assert_eq!(h.outputs.len(), 1);
    let before = views.0.lock().unwrap().last().unwrap().clone();
    assert_eq!(
        before.pending[&key_a],
        (
            released_a.id,
            released_a.incarnation,
            released_a.operation_id
        )
    );
    assert!(!before.free.contains(&slot_a));
    *h.nodes[0].snapshot.lock().unwrap() = "fixture:waiting_for_B_completion".into();
    h.paused_completions[0] = true;
    h.nodes[0]
        .sender
        .as_ref()
        .unwrap()
        .try_send(WorkerInput::Event(event_wire(terminal_b)))
        .unwrap_or_else(|_| panic!("B's genuine terminal must fit the real input queue"));

    let full_deadline = Instant::now() + Duration::from_secs(2);
    while h.nodes[0].snapshot.lock().unwrap().as_str() != "completion_queue_full:waiting" {
        assert!(
            Instant::now() < full_deadline,
            "B never caused a new actual Full"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!h.nodes[0].thread.as_ref().unwrap().is_finished());
    FullRelease {
        h,
        a,
        b,
        acknowledgement,
        acknowledgement_bytes,
        views,
        key_a,
        slot_a,
    }
}

fn release_output_space(h: &mut Harness, b: &InferenceCommand) {
    let Poll::Event(occupied) = h.nodes[0].mailbox.try_take() else {
        panic!("actual Full must contain B's real completion, not just a status string");
    };
    let occupied_bytes = p4_protocol::event::encode(&occupied).unwrap();
    let occupied = event_wire(occupied);
    assert_eq!(
        p4_protocol::event::encode(&occupied).unwrap(),
        occupied_bytes
    );
    assert_eq!(occupied.envelope.payload_content_type, OUTPUT_CONTENT_TYPE);
    let output: OutcomePayload = serde_json::from_slice(&occupied.payload).unwrap();
    assert_eq!(output.request_id, b.request_id);
    assert_eq!(output.token, 1000);
    assert_eq!(output.position, 1);
    assert_eq!(output.text, "token-1000 ");
    assert_eq!(output.stop.as_deref(), Some("length"));
    h.outputs.push(output);
    h.received.push(occupied);
    h.paused_completions[0] = false;
    h.hold_tail = false;
    h.hold_control = None;
}

fn assert_unique_wire(h: &Harness) {
    let mut ids = std::collections::BTreeSet::new();
    for event in h.received.iter().chain(&h.stage_events) {
        assert!(
            ids.insert(event.envelope.event_id.clone()),
            "completion repeated"
        );
        assert_eq!(event_wire(event.clone()), *event);
    }
}

fn native_calls(h: &Harness) -> Vec<(usize, usize, usize, usize, usize)> {
    h.nodes
        .iter()
        .map(|node| {
            let native = node.native.lock().unwrap();
            (
                native.logical_calls,
                native.physical_calls,
                native.tokenize_calls,
                native.sampler_calls,
                native.releases.values().sum(),
            )
        })
        .collect()
}

#[test]
fn completion_full_cannot_starve_a_genuine_release_acknowledgement() {
    let FullRelease {
        mut h,
        a,
        b,
        acknowledgement,
        acknowledgement_bytes,
        views,
        key_a,
        slot_a,
    } = full_with_pending_release();
    let accepted_ack = event_wire(acknowledgement);
    assert_eq!(
        p4_protocol::event::encode(&accepted_ack).unwrap(),
        acknowledgement_bytes
    );
    h.nodes[0]
        .sender
        .as_ref()
        .unwrap()
        .try_send(WorkerInput::Event(accepted_ack))
        .unwrap_or_else(|_| panic!("authentic A ACK must be accepted by the actual input queue"));

    // A timeout is not the evidence of Full: the new snapshot above and the
    // actual queued B OUTPUT below establish it independently. This bounded
    // deadline only tests the actor's opportunity to apply the accepted ACK.
    let (history, changed) = &*views;
    let (history, _) = changed
        .wait_timeout_while(
            history.lock().unwrap(),
            Duration::from_millis(200),
            |history| !committed_a(history, &key_a, slot_a),
        )
        .unwrap();
    let processed_before_room = committed_a(&history, &key_a, slot_a);
    let before_room = history.clone();
    drop(history);
    assert!(!h.nodes[0].thread.as_ref().unwrap().is_finished());
    assert_eq!(
        h.outputs.len(),
        1,
        "the harness has not consumed B's output"
    );

    // Always recover before asserting the new guarantee. Even the RED run must
    // retain the original positive token/text/position/KV/release/observation
    // oracles and prove that the held byte-identical ACK was merely delayed.
    release_output_space(&mut h, &b);
    h.finish(&[a, b]);
    release_notifications::assert_complete(&h);
    assert!(committed_a(&views.0.lock().unwrap(), &key_a, slot_a));
    assert_unique_wire(&h);
    println!(
        "actual_full=true; queue_capacity=1; input_ack_accepted=true; \
         ack_committed_before_room={processed_before_room}; recovery_outputs={}; \
         recovery_receipts={}; native_release_counts={:?}; before_room={before_room:?}",
        h.outputs.len(),
        h.received
            .iter()
            .filter(|event| event.envelope.payload_content_type == RELEASE_RECEIPT_CONTENT_TYPE)
            .count(),
        h.nodes
            .iter()
            .map(|node| node
                .native
                .lock()
                .unwrap()
                .releases
                .values()
                .copied()
                .collect::<Vec<_>>())
            .collect::<Vec<_>>(),
    );
    assert!(
        processed_before_room,
        "completion Full starved a genuine RELEASED already accepted at input; \
         A's pending release and slot stayed blocked until unrelated output gained space"
    );
}

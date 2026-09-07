//! Busy UNLOAD must refuse without ending a native owner or forgetting work.
//! These checks use the existing public UNLOAD event and actual Worker::run;
//! they do not invent a drain/cancel command or call a state predicate oracle.
use super::*;

#[derive(Debug, PartialEq, Eq)]
struct NativeSnapshot {
    logical_calls: usize,
    physical_calls: usize,
    sampler_calls: usize,
    live: BTreeMap<NativeKey, Vec<i32>>,
    written: BTreeMap<NativeKey, Vec<(u32, i32)>>,
    releases: BTreeMap<NativeKey, usize>,
    release_bodies: Vec<Vec<u8>>,
    shutdowns: usize,
    speculative: speculative::ScriptTrace,
}

fn native_snapshot(h: &Harness, target: usize) -> NativeSnapshot {
    let native = h.nodes[target].native.lock().unwrap();
    NativeSnapshot {
        logical_calls: native.logical_calls,
        physical_calls: native.physical_calls,
        sampler_calls: native.sampler_calls,
        live: native.live.clone(),
        written: native.written.clone(),
        releases: native.releases.clone(),
        release_bodies: native.release_bodies.clone(),
        shutdowns: native.shutdowns,
        speculative: native.speculative.clone(),
    }
}

fn submit(h: &mut Harness, target: usize, tag: &str, generation: u64) {
    assert!(!h.received.iter().any(|e| e.envelope.correlation_id == tag));
    h.pending.push_back(event_wire(event(
        target,
        tag,
        UNLOAD_CONTENT_TYPE,
        serde_json::to_vec(&UnloadCommand {
            load_generation: generation,
        })
        .unwrap(),
    )));
}

fn is_response(event: &Event, target: usize, tag: &str) -> bool {
    event.envelope.source == endpoint(target)
        && event.envelope.correlation_id == tag
        && matches!(
            event.envelope.payload_content_type.as_str(),
            ERROR_CONTENT_TYPE | UNLOADED_CONTENT_TYPE
        )
}

fn assert_refused_unload(h: &mut Harness, target: usize, tag: &str, generation: u64) -> String {
    let before = native_snapshot(h, target);
    assert!(
        !before.live.is_empty(),
        "busy fixture must have real fake-native KV, not just a manually populated worker map"
    );
    let outputs_before = h.outputs.len();
    assert!(h.expected_unload_error.is_none());
    h.expected_unload_error = Some((tag.into(), target));
    submit(h, target, tag, generation);
    h.until("busy UNLOAD has an exact consumer response", |h| {
        h.received.iter().any(|e| is_response(e, target, tag))
    });
    let after = native_snapshot(h, target);
    let replies: Vec<_> = h
        .received
        .iter()
        .filter(|e| is_response(e, target, tag))
        .collect();
    let errors: Vec<_> = replies
        .iter()
        .filter(|e| e.envelope.payload_content_type == ERROR_CONTENT_TYPE)
        .collect();
    let successes = replies
        .iter()
        .filter(|e| e.envelope.payload_content_type == UNLOADED_CONTENT_TYPE)
        .count();
    println!(
        "BUSY_UNLOAD target={target} native_shutdown_delta={} errors={} successes={successes} snapshot={}",
        after.shutdowns - before.shutdowns,
        errors.len(),
        h.nodes[target].snapshot.lock().unwrap()
    );
    assert_eq!(
        after.shutdowns, before.shutdowns,
        "busy UNLOAD must refuse before native shutdown"
    );
    assert_eq!(successes, 0, "UNLOADED may not conceal abandoned inference");
    assert_eq!(
        errors.len(),
        1,
        "the exact UNLOAD caller needs one explicit refusal"
    );
    let rejection: serde_json::Value = serde_json::from_slice(&errors[0].payload).unwrap();
    assert_eq!(rejection["code"], "LLAMA_ADAPTER_EVENT_REJECTED");
    let detail = rejection["detail"]
        .as_str()
        .expect("refusal carries a diagnostic")
        .to_owned();
    assert_eq!(
        h.nodes[target].snapshot.lock().unwrap().as_str(),
        format!("failed:{detail}")
    );
    assert_eq!(
        after, before,
        "busy refusal may not mutate native calls, KV, write history or releases"
    );
    assert_eq!(
        h.outputs.len(),
        outputs_before,
        "refused UNLOAD cannot manufacture request completion"
    );
    h.expected_unload_error = None;
    detail
}

pub(super) fn assert_busy_unload(h: &mut Harness, target: usize, tag: &str) -> serde_json::Value {
    let detail = assert_refused_unload(h, target, tag, 1);
    let work = detail
        .strip_prefix("unload is busy;work=")
        .expect("busy refusal must identify retained work using the specified contract");
    serde_json::from_str(work).expect("busy work census is JSON")
}

pub(super) fn assert_stale_unload(h: &mut Harness, target: usize, tag: &str) {
    let detail = assert_refused_unload(h, target, tag, 2);
    assert_eq!(
        detail, "unload load generation is stale",
        "identity mismatch must be rejected before the busy-state predicate"
    );
}

pub(super) fn assert_idle_unload(h: &mut Harness, target: usize, tag: &str) {
    let before = native_snapshot(h, target);
    assert!(
        before.live.is_empty(),
        "idle positive control needs all native KV released first"
    );
    assert!(h.expected_unload_error.is_none());
    submit(h, target, tag, 1);
    h.until("idle UNLOAD succeeds after actual completion", |h| {
        h.received.iter().any(|e| is_response(e, target, tag))
    });
    let replies: Vec<_> = h
        .received
        .iter()
        .filter(|e| is_response(e, target, tag))
        .collect();
    assert_eq!(replies.len(), 1);
    assert_eq!(
        replies[0].envelope.payload_content_type,
        UNLOADED_CONTENT_TYPE
    );
    let reply: serde_json::Value = serde_json::from_slice(&replies[0].payload).unwrap();
    assert_eq!(
        reply,
        serde_json::json!({"state":"unloaded","load_generation":1})
    );
    let after = native_snapshot(h, target);
    let mut expected = before;
    expected.shutdowns += 1;
    assert_eq!(
        after, expected,
        "idle unload must only close the native owner, not rewrite completed history"
    );
    assert_eq!(
        h.nodes[target].snapshot.lock().unwrap().as_str(),
        "unloaded"
    );
}

#[test]
fn b2_busy_head_unload_preserves_held_tail_work_then_idle_unload_succeeds() {
    for stages in [2, 4] {
        let commands = [request("must-survive-head-unload", 7, 5)];
        let mut h = Harness::new(stages, 1, 16, &commands);
        h.hold_tail = true;
        h.until("first native result is held before head return", |h| {
            !h.held_tail.is_empty()
        });
        assert!(h.outputs.is_empty());
        assert_eq!(h.nodes[0].native.lock().unwrap().logical_calls, 1);
        let held: Vec<_> = h.held_tail.iter().map(|e| e.payload.clone()).collect();
        let work = assert_busy_unload(&mut h, 0, "busy-head-unload");
        assert_eq!(work["requests"], 1);
        assert_eq!(work["pending"], 0);
        assert_eq!(work["flight_batches"], 1);
        assert_eq!(work["flight_executions"], 2);
        assert_eq!(work["active_owners"], 1);
        assert_eq!(work["active_frontiers"], 1);
        assert_eq!(
            h.held_tail
                .iter()
                .map(|e| e.payload.clone())
                .collect::<Vec<_>>(),
            held
        );
        h.resume_tail();
        // Existing ordinary oracle checks each output and each native write;
        // erasing a request/flight/slot to refuse cleanly cannot pass this.
        h.finish(&commands);
        assert_idle_unload(&mut h, 0, "idle-head-unload");
    }
}

#[test]
fn b2_busy_middle_unload_preserves_native_kv_without_head_request_state() {
    let commands = [request("must-survive-middle-unload", 7, 5)];
    let mut h = Harness::new(4, 1, 16, &commands);
    h.hold_tail = true;
    h.until("middle has forwarded native rows and tail is held", |h| {
        !h.held_tail.is_empty()
    });
    // Harness queues a Middle SESSION at node 1 and sends inference only to
    // head. Node 1 got real PHYSICAL frames, never a local PREFILL request.
    assert_eq!(h.nodes[1].native.lock().unwrap().logical_calls, 0);
    assert!(h.nodes[1].native.lock().unwrap().physical_calls > 0);
    let work = assert_busy_unload(&mut h, 1, "busy-middle-unload");
    assert_eq!(work["requests"], 0);
    assert_eq!(work["pending"], 0);
    assert_eq!(work["flight_batches"], 0);
    assert_eq!(work["flight_executions"], 0);
    assert_eq!(work["active_owners"], 1);
    assert_eq!(work["active_frontiers"], 1);
    h.resume_tail();
    h.finish(&commands);
    assert_idle_unload(&mut h, 1, "idle-middle-unload");
}

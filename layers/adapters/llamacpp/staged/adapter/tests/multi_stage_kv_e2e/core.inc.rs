#[test]
#[ignore = "real 2-stage llama.cpp KV E2E; set P4_STAGED_LLAMA_SERVER_BINARY and P4_STAGED_LLAMA_MODEL, then pass --ignored"]
fn real_two_stage_kv_save_restore_drop_is_equivalent() {
    run_real_multi_stage_kv(&[0, 14, 28], "two-stage");
}

#[test]
#[ignore = "real 4-stage llama.cpp KV E2E; set P4_STAGED_LLAMA_SERVER_BINARY and P4_STAGED_LLAMA_MODEL, then pass --ignored"]
fn real_four_stage_kv_save_restore_drop_is_equivalent() {
    run_real_multi_stage_kv(&[0, 7, 14, 21, 28], "four-stage");
}

fn run_real_multi_stage_kv(default_boundaries: &[i32], label: &str) {
    let Some(binary) = required_file(SERVER_ENV, "staged C++ server") else {
        return;
    };
    let Some(model) = required_file(MODEL_ENV, "GGUF model") else {
        return;
    };
    let boundaries = boundaries_from_environment(default_boundaries);
    assert!(boundaries.len() >= 2, "at least one stage is required");
    assert!(boundaries.windows(2).all(|range| range[1] > range[0]));

    let limits = ProtocolLimits::default();
    let model_identity = model.to_string_lossy().into_owned();
    let root = TemporaryRoot::new(label);
    let mut stages = boundaries
        .windows(2)
        .enumerate()
        .map(|(index, range)| {
            let stage_root = root.path.join(format!("stage-{index}"));
            let mut control = launch_stage(
                &binary,
                &model,
                range[0],
                range[1],
                &stage_root,
                index,
            );
            control.start().expect("start real KV stage server");
            let ready = wait_ready(&mut control);
            assert_eq!(ready.protocol_revision, PROTOCOL_REVISION);
            assert!(
                ready.server_id.split(';').any(|field| field == "kv=1"),
                "stage {index} did not advertise KV persistence: {}",
                ready.server_id
            );
            println!(
                "REAL_MULTI_STAGE_KV READY label={label} stage={index} range=[{}, {}) capabilities={}",
                range[0], range[1], ready.server_id
            );
            control
        })
        .collect::<Vec<_>>();

    let sequence_id = format!("{label}-sequence");
    let cache_key = format!("{label}-cache");
    let prefill_cuts = prefill_pipeline(
        &mut stages,
        &sequence_id,
        PROMPT,
        limits,
    );
    assert_eq!(prefill_cuts.len(), stages.len());

    let saved = save_all(
        &mut stages,
        &boundaries,
        &sequence_id,
        &cache_key,
        &model_identity,
        &root.path,
        limits,
    );
    assert_eq!(saved.len(), stages.len());
    for (index, result) in saved.iter().enumerate() {
        assert!(result.bytes > 0, "stage {index} exported an empty KV state");
        assert_eq!(result.checksum.len(), 64, "stage {index} checksum is not SHA-256");
        assert!(
            result.checksum.chars().all(|value| value.is_ascii_hexdigit()),
            "stage {index} checksum is not hexadecimal"
        );
        let path = root.path.join(format!("stage-{index}")).join(format!("{cache_key}.lkv"));
        let file_bytes = fs::metadata(&path)
            .unwrap_or_else(|error| panic!("stage {index} KV file missing: {path:?}: {error}"))
            .len();
        assert!(file_bytes > result.bytes, "stage {index} file has no metadata header");
        println!(
            "REAL_MULTI_STAGE_KV SAVED stage={index} range=[{}, {}) state_bytes={} file_bytes={} checksum={}",
            boundaries[index], boundaries[index + 1], result.bytes, file_bytes, result.checksum
        );
    }

    restore_all(
        &mut stages,
        &boundaries,
        &sequence_id,
        &cache_key,
        &model_identity,
        &saved,
        limits,
    );
    let mut decode_input = SequencePayload {
        sequence_id: sequence_id.clone(),
        descriptors: Vec::new(),
        payloads: Vec::new(),
        n_tokens: Some(1),
        prompt: None,
        options: String::new(),
        initial_tokens: None,
        position: Some(prefill_cuts[0].n_tokens.unwrap_or(0)),
        outcome: None,
    };
    for (index, stage) in stages.iter_mut().enumerate() {
        let body = HopPayload {
            phase: HopPhase::Decode,
            sequences: vec![decode_input],
            legacy: false,
        }
        .encode(limits)
        .unwrap_or_else(|error| panic!("encode stage {index} restore decode: {error}"));
        let response = stage
            .request(Frame::new(Operation::Hop, body).expect("create restore decode HOP"))
            .expect("restore decode HOP response");
        assert_operation(&response, Operation::HopResult);
        decode_input = HopPayload::decode(&response.body, limits)
            .expect("decode restore decode HOP result")
            .sequences
            .into_iter()
            .next()
            .expect("restore decode stage returned no sequence");
        println!(
            "REAL_MULTI_STAGE_KV RESTORE_DECODE stage={index} output_descriptors={} outcome={}",
            decode_input.descriptors.len(),
            decode_input.outcome.is_some()
        );
    }
    assert!(
        decode_input.outcome.is_some(),
        "restored decode pipeline did not produce a tail outcome"
    );
    // Export the restored state again. Matching bytes and checksums prove that
    // every stage reconstructed the same llama state blob. The decode above is
    // also intentional: it is the regression guard for the asynchronous KV
    // upload that previously surfaced as llama_decode(-3).
    restore_all(
        &mut stages,
        &boundaries,
        &sequence_id,
        &cache_key,
        &model_identity,
        &saved,
        limits,
    );
    let roundtrip = save_all(
        &mut stages,
        &boundaries,
        &sequence_id,
        &cache_key,
        &model_identity,
        &root.path,
        limits,
    );
    assert_eq!(roundtrip, saved, "restored state differs when re-exported");
    println!(
        "REAL_MULTI_STAGE_KV RESTORE_EQUAL label={label} stages={} bytes_and_checksums=identical",
        saved.len()
    );

    for (index, stage) in stages.iter_mut().enumerate() {
        let request = kv_request(
            Operation::KvDrop,
            &sequence_id,
            &cache_key,
            &model_identity,
            boundaries[index],
            boundaries[index + 1],
            limits,
        );
        let response = stage.request(request).expect("KV_DROP response");
        assert_operation(&response, Operation::KvResult);
        let dropped = KvResult::decode(&response.body, limits).expect("decode KV_DROP result");
        assert_eq!(dropped.sequence_id, sequence_id);
        assert_eq!(dropped.cache_key, cache_key);
        assert_eq!(dropped.bytes, 0);
        assert_eq!(dropped.checksum, saved[index].checksum);
        let path = root.path.join(format!("stage-{index}")).join(format!("{cache_key}.lkv"));
        assert!(!path.exists(), "stage {index} KV file survived KV_DROP: {path:?}");
        println!(
            "REAL_MULTI_STAGE_KV DROPPED stage={index} checksum={} file_exists=false",
            dropped.checksum
        );
    }

    for stage in &mut stages {
        unload_and_reap(stage);
    }
    println!(
        "REAL_MULTI_STAGE_KV PASS label={label} stages={} boundaries={:?}",
        stages.len(), boundaries
    );
}

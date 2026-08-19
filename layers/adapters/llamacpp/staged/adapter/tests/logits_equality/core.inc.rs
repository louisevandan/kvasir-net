#[test]
#[ignore = "real staged logits E2E; set P4_STAGED_LLAMA_SERVER_BINARY and P4_STAGED_LLAMA_MODEL, then pass --ignored"]
fn staged_logits_match_full_model_numeric_output() {
    let Some(binary) = required_file(SERVER_ENV, "staged C++ server") else {
        return;
    };
    let Some(model) = required_file(MODEL_ENV, "GGUF model") else {
        return;
    };
    let layer_end = env_i32(LAYER_END_ENV, 28);
    let split_layer = env_i32(SPLIT_LAYER_ENV, layer_end / 2);
    assert!(layer_end > 1 && split_layer > 0 && split_layer < layer_end);

    let limits = ProtocolLimits::default();
    let mut full = launch_stage(&binary, &model, 0, layer_end);
    let mut first = launch_stage(&binary, &model, 0, split_layer);
    let mut last = launch_stage(&binary, &model, split_layer, layer_end);
    full.start().expect("start full-model server");
    first.start().expect("start first staged server");
    last.start().expect("start last staged server");
    assert_eq!(wait_ready(&mut full).protocol_revision, PROTOCOL_REVISION);
    assert_eq!(wait_ready(&mut first).protocol_revision, PROTOCOL_REVISION);
    assert_eq!(wait_ready(&mut last).protocol_revision, PROTOCOL_REVISION);

    let full_output = prefill(&mut full, "full-model", PROMPT, None, limits);
    let first_output = prefill(&mut first, "staged-model", PROMPT, None, limits);
    let staged_output = prefill(&mut last, "staged-model", "", Some(first_output), limits);

    let comparison = compare_logits(&full_output, &staged_output);
    assert!(
        comparison.max_abs <= ABS_TOLERANCE && comparison.max_relative <= REL_TOLERANCE,
        "staged logits differ: {comparison:?}"
    );
    println!(
        "LOGITS_EQUAL full_outputs={} staged_outputs={} values={} max_abs={:.9e} max_relative={:.9e}",
        full_output.descriptors.len(),
        staged_output.descriptors.len(),
        comparison.values,
        comparison.max_abs,
        comparison.max_relative
    );

    unload_and_reap(&mut full);
    unload_and_reap(&mut first);
    unload_and_reap(&mut last);
}

#[test]
#[ignore = "real staged sampled-output E2E; set P4_STAGED_LLAMA_SERVER_BINARY and P4_STAGED_LLAMA_MODEL, then pass --ignored"]
fn staged_sampled_tokens_match_full_model_for_two_and_four_stages() {
    let Some(binary) = required_file(SERVER_ENV, "staged C++ server") else {
        return;
    };
    let Some(model) = required_file(MODEL_ENV, "GGUF model") else {
        return;
    };
    let limits = ProtocolLimits::default();
    let layer_end = env_i32(LAYER_END_ENV, 28);
    let split_layer = env_i32(SPLIT_LAYER_ENV, layer_end / 2);
    assert!(layer_end > 3 && split_layer > 0 && split_layer < layer_end);

    let baseline = sampled_tokens(&binary, &model, &[0, layer_end], "baseline", limits);
    let two_stage = sampled_tokens(
        &binary,
        &model,
        &[0, split_layer, layer_end],
        "two-stage",
        limits,
    );
    let quarter = layer_end / 4;
    assert!(
        quarter > 0,
        "four-stage split requires at least four layers"
    );
    let four_stage = sampled_tokens(
        &binary,
        &model,
        &[0, quarter, quarter * 2, quarter * 3, layer_end],
        "four-stage",
        limits,
    );

    println!(
        "SAMPLED_OUTPUT_EQUAL steps={} contract=multi-token-tail-token-id",
        baseline.len()
    );
    assert_eq!(baseline, two_stage, "two-stage sampled sequence");
    assert_eq!(baseline, four_stage, "four-stage sampled sequence");
}

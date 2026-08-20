const SAMPLED_STEPS: usize = 8;

fn sampled_tokens(
binary: &Path,
    model: &Path,
    boundaries: &[i32],
    label: &str,
    limits: ProtocolLimits,
) -> Vec<OutcomeMetadata> {
    assert!(boundaries.len() >= 2);
    let mut controls = boundaries
        .windows(2)
        .enumerate()
        .map(|(index, range)| {
            let mut control = launch_stage(binary, model, range[0], range[1]);
            control.start().expect("start sampled-output stage server");
            let ready = wait_ready(&mut control);
            assert_eq!(ready.protocol_revision, PROTOCOL_REVISION);
            println!(
                "SAMPLED_OUTPUT_READY label={label} stage={} range=[{}, {}) capabilities={}",
                index, range[0], range[1], ready.server_id
            );
            control
        })
        .collect::<Vec<_>>();

    let sequence_id = format!("{label}-sequence");
    let mut cut = prefill(&mut controls[0], &sequence_id, PROMPT, None, limits);
    for control in controls.iter_mut().skip(1) {
        cut = prefill(control, &sequence_id, "", Some(cut), limits);
    }
    // The first stage owns the native KV state and starts a decode with no
    // inbound cut-set. Only the output of that decode crosses to the next
    // stage. Feeding the final prefill cut back into stage 0 mixes stage-local
    // tensors with the original stage's graph and makes llama_decode reject
    // the batch (status -3).
    let mut outcomes = Vec::with_capacity(SAMPLED_STEPS);
    for _ in 0..SAMPLED_STEPS {
        let mut full_decode = decode(&mut controls[0], &sequence_id, None, limits);
        for control in controls.iter_mut().skip(1) {
            full_decode = decode(control, &sequence_id, Some(full_decode), limits);
        }
        outcomes.push(
            full_decode
                .outcome
                .clone()
                .expect("tail stage returned sampled outcome metadata"),
        );
    }
    for control in &mut controls {
        unload_and_reap(control);
    }
    outcomes
}

fn prefill(
    control: &mut ProcessServerControl,
    sequence_id: &str,
    prompt: &str,
    cut_set: Option<SequencePayload>,
    limits: ProtocolLimits,
) -> SequencePayload {
    let sequence = cut_set.unwrap_or_else(|| SequencePayload {
        sequence_id: sequence_id.to_owned(),
        descriptors: Vec::new(),
        payloads: Vec::new(),
        n_tokens: None,
        prompt: Some(prompt.to_owned()),
        initial_tokens: None,
        options: r#"{"temperature":0,"seed":1}"#.into(),
        position: Some(0),
        outcome: None,
    });
    let body = HopPayload {
        phase: HopPhase::Prefill,
        sequences: vec![sequence],
        legacy: false,
    }
    .encode(limits)
    .expect("encode prefill HOP");
    let response = control
        .request(Frame::new(Operation::Hop, body).expect("create prefill HOP"))
        .expect("prefill HOP response");
    assert_eq!(response.header.operation, Operation::HopResult, "prefill response: {:?}", response.body);
    let result = HopPayload::decode(&response.body, limits).expect("decode prefill HOP result");
    result
        .sequences
        .into_iter()
        .next()
        .expect("prefill HOP sequence result")
}

fn decode(
    control: &mut ProcessServerControl,
    sequence_id: &str,
    input: Option<SequencePayload>,
    limits: ProtocolLimits,
) -> SequencePayload {
    let input = input.unwrap_or_else(|| SequencePayload {
        sequence_id: sequence_id.to_owned(),
        descriptors: Vec::new(),
        payloads: Vec::new(),
        n_tokens: None,
        prompt: None,
        initial_tokens: None,
        options: String::new(),
        position: Some(0),
        outcome: None,
    });
    let body = HopPayload {
        phase: HopPhase::Decode,
        sequences: vec![SequencePayload {
            sequence_id: sequence_id.to_owned(),
            descriptors: input.descriptors,
            payloads: input.payloads,
            n_tokens: Some(1),
            prompt: None,
            initial_tokens: None,
            options: r#"{"temperature":0,"seed":1}"#.into(),
            position: Some(0),
            outcome: None,
        }],
        legacy: false,
    }
    .encode(limits)
    .expect("encode decode HOP");
    let response = control
        .request(Frame::new(Operation::Hop, body).expect("create decode HOP"))
        .expect("decode HOP response");
    assert_eq!(
        response.header.operation,
        Operation::HopResult,
        "decode response body: {}",
        String::from_utf8_lossy(&response.body)
    );
    HopPayload::decode(&response.body, limits)
        .expect("decode HOP_RESULT")
        .sequences
        .into_iter()
        .next()
        .expect("decode HOP sequence result")
}

#[derive(Debug)]
struct Comparison {
    values: usize,
    max_abs: f32,
    max_relative: f32,
}

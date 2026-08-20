fn prefill_pipeline(
    stages: &mut [ProcessServerControl],
    sequence_id: &str,
    prompt: &str,
    limits: ProtocolLimits,
) -> Vec<SequencePayload> {
    let mut input = SequencePayload {
        sequence_id: sequence_id.to_owned(),
        descriptors: Vec::new(),
        payloads: Vec::new(),
        n_tokens: None,
        prompt: Some(prompt.to_owned()),
        initial_tokens: None,
        options: String::new(),
        position: Some(0),
        outcome: None,
    };
    let mut cuts = Vec::with_capacity(stages.len());
    for (index, stage) in stages.iter_mut().enumerate() {
        let body = HopPayload {
            phase: HopPhase::Prefill,
            sequences: vec![input],
            legacy: false,
        }
        .encode(limits)
        .unwrap_or_else(|error| panic!("encode stage {index} prefill: {error}"));
        let response = stage
            .request(Frame::new(Operation::Hop, body).expect("create prefill HOP"))
            .expect("prefill HOP response");
        assert_operation(&response, Operation::HopResult);
        input = HopPayload::decode(&response.body, limits)
            .expect("decode prefill HOP result")
            .sequences
            .into_iter()
            .next()
            .expect("stage returned no prefill sequence");
        assert_eq!(input.sequence_id, sequence_id);
        assert!(!input.descriptors.is_empty(), "stage {index} returned no cut-set descriptors");
        assert_eq!(input.descriptors.len(), input.payloads.len());
        assert!(input.n_tokens.unwrap_or(0) > 0);
        assert!(input.outcome.is_none());
        cuts.push(input.clone());
    }
    cuts
}

fn save_all(
    stages: &mut [ProcessServerControl],
    boundaries: &[i32],
    sequence_id: &str,
    cache_key: &str,
    model_identity: &str,
    root: &Path,
    limits: ProtocolLimits,
) -> Vec<KvResult> {
    stages
        .iter_mut()
        .enumerate()
        .map(|(index, stage)| {
            let request = kv_request(
                Operation::KvSave,
                sequence_id,
                cache_key,
                model_identity,
                boundaries[index],
                boundaries[index + 1],
                limits,
            );
            let response = stage.request(request).expect("KV_SAVE response");
            assert_operation(&response, Operation::KvResult);
            let result = KvResult::decode(&response.body, limits).expect("decode KV_SAVE result");
            assert_eq!(result.sequence_id, sequence_id);
            assert_eq!(result.cache_key, cache_key);
            let path = root.join(format!("stage-{index}")).join(format!("{cache_key}.lkv"));
            assert!(path.is_file(), "stage {index} did not publish its KV file");
            result
        })
        .collect()
}

fn restore_all(
    stages: &mut [ProcessServerControl],
    boundaries: &[i32],
    sequence_id: &str,
    cache_key: &str,
    model_identity: &str,
    saved: &[KvResult],
    limits: ProtocolLimits,
) {
    for (index, stage) in stages.iter_mut().enumerate() {
        let request = kv_request(
            Operation::KvRestore,
            sequence_id,
            cache_key,
            model_identity,
            boundaries[index],
            boundaries[index + 1],
            limits,
        );
        let response = stage.request(request).expect("KV_RESTORE response");
        assert_operation(&response, Operation::KvResult);
        let result = KvResult::decode(&response.body, limits).expect("decode KV_RESTORE result");
        assert_eq!(&result, &saved[index], "stage {index} restore metadata differs");
    }
}

fn kv_request(
    operation: Operation,
    sequence_id: &str,
    cache_key: &str,
    model_identity: &str,
    stage_begin: i32,
    stage_end: i32,
    limits: ProtocolLimits,
) -> Frame {
    let payload = KvPayload {
        sequence_id: sequence_id.to_owned(),
        cache_key: cache_key.to_owned(),
        model_identity: model_identity.to_owned(),
        stage_begin,
        stage_end,
        flags: 0,
        expected_checksum: String::new(),
        operation_id: format!("kv-{cache_key}"),
    };
    Frame::new(
        operation,
        payload.encode(limits).expect("encode KV request"),
    )
    .expect("create KV request")
}

fn launch_stage(
    binary: &Path,
    model: &Path,
    begin: i32,
    end: i32,
    kv_root: &Path,
    index: usize,
) -> ProcessServerControl {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve stage endpoint");
    let endpoint = listener.local_addr().expect("read stage endpoint");
    drop(listener);
    let model_text = model.to_string_lossy();
    let plan = format!(
        "--model {} --layer-begin {begin} --layer-end {end} --n-seq-max 1 \
         --ctx-size 512 --flash-attn 0 --temp 0 --seed 1 --model-identity {} \
         --kv-root {} {}",
        quote_plan_value(&model_text),
        quote_plan_value(&model_text),
        quote_plan_value(&kv_root.to_string_lossy()),
        env::var("P4_STAGED_LLAMA_PLAN_EXTRA_ARGS").unwrap_or_default(),
    );
    let mut launch = ServerLaunch::new(binary.to_owned(), endpoint, plan.into_bytes());
    launch.args = vec![
        OsString::from("--port"),
        endpoint.port().to_string().into(),
        OsString::from("--bind"),
        OsString::from("127.0.0.1"),
    ];
    launch.ready_timeout = Duration::from_secs(180);
    launch.io_timeout = Duration::from_secs(120);
    launch.environment = runtime_environment(binary, index);
    ProcessServerControl::new(launch)
}

fn wait_ready(control: &mut ProcessServerControl) -> p4_llamacpp_staged_adapter::process::ReadyInfo {
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        match control.wait_ready(deadline) {
            Ok(Some(ready)) => return ready,
            Ok(None) if Instant::now() >= deadline => panic!("stage server readiness timeout"),
            Ok(None) => std::thread::yield_now(),
            Err(error) => panic!("stage server readiness failed: {error}"),
        }
    }
}

fn unload_and_reap(control: &mut ProcessServerControl) {
    let response = control
        .request(Frame::new(Operation::Unload, Vec::new()).expect("create UNLOAD"))
        .expect("UNLOAD response");
    assert_operation(&response, Operation::Unload);
    assert_eq!(response.body, b"UNLOADED");
    control.shutdown().expect("reap stage server");
    assert!(control.pid().is_none(), "stage server child was not reaped");
}

fn assert_operation(response: &Frame, expected: Operation) {
    assert_eq!(
        response.header.operation,
        expected,
        "staged server returned {:?}: {}",
        response.header.operation,
        String::from_utf8_lossy(&response.body)
    );
}

fn boundaries_from_environment(default: &[i32]) -> Vec<i32> {
    let Some(value) = env::var_os(BOUNDARIES_ENV) else {
        return default.to_vec();
    };
    value
        .to_string_lossy()
        .split(',')
        .map(|part| part.trim().parse().expect("invalid KV boundary"))
        .collect()
}

fn runtime_environment(binary: &Path, index: usize) -> Vec<(OsString, OsString)> {
    let mut paths = vec![binary.parent().unwrap_or(binary).to_path_buf()];
    if let Some(parent) = binary.parent().and_then(Path::parent) {
        paths.push(parent.join("bin").join("Release"));
    }
    if let Some(path) = env::var_os("PATH") {
        paths.extend(env::split_paths(&path));
    }
    let mut environment = vec![(
        OsString::from("PATH"),
        env::join_paths(paths).expect("compose staged server PATH"),
    )];
    let name = format!("P4_STAGED_LLAMA_CUDA_VISIBLE_DEVICES_STAGE{index}");
    if let Some(device) = env::var_os(name) {
        environment.push((OsString::from("CUDA_VISIBLE_DEVICES"), device));
    }
    environment
}

fn required_file(name: &str, description: &str) -> Option<PathBuf> {
    let Some(path) = env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from) else {
        println!("SKIP: set {name} to the {description} path");
        return None;
    };
    if path.is_file() {
        Some(path)
    } else {
        println!("SKIP: {description} is not a file: {}", path.display());
        None
    }
}

fn quote_plan_value(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

struct TemporaryRoot {
    path: PathBuf,
}

impl TemporaryRoot {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let path = env::temp_dir().join(format!("p4-staged-{label}-kv-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).expect("create staged KV root");
        Self { path }
    }
}

impl Drop for TemporaryRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

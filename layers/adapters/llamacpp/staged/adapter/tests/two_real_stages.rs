use p4_llamacpp_staged_adapter::process::{ProcessServerControl, ServerControl, ServerLaunch};
use p4_llamacpp_staged_adapter::{
    Frame, HopPayload, HopPhase, Operation, PROTOCOL_REVISION, ProtocolLimits, SequencePayload,
};
use std::env;
use std::ffi::OsString;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SERVER_ENV: &str = "P4_STAGED_LLAMA_SERVER_BINARY";
const MODEL_ENV: &str = "P4_STAGED_LLAMA_MODEL";
const SPLIT_LAYER_ENV: &str = "P4_STAGED_LLAMA_SPLIT_LAYER";
const LAYER_END_ENV: &str = "P4_STAGED_LLAMA_LAYER_END";
const DEFAULT_MODEL: &str = r"S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf";
const PROMPT: &str = "Reply with one short word: hello";
const READY_TIMEOUT_ENV: &str = "P4_STAGED_LLAMA_READY_TIMEOUT_SECS";

#[test]
#[ignore = "real two-stage llama.cpp E2E; set P4_STAGED_LLAMA_SERVER_BINARY and pass --ignored"]
fn two_real_stages_forward_prefill_cut_set() {
    let Some(binary) = file_from_env(SERVER_ENV, "staged C++ server") else {
        return;
    };
    let model = env::var_os(MODEL_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MODEL));
    if !model.is_file() {
        println!("SKIP: model is not a file: {}", model.display());
        return;
    }

    let limits = ProtocolLimits::default();
    let split_layer = env_i32(SPLIT_LAYER_ENV, 14);
    let layer_end = env_i32(LAYER_END_ENV, 28);
    assert!(split_layer > 0 && split_layer < layer_end);
    let mut stage0 = launch_stage(
        &binary,
        &model,
        0,
        split_layer,
        "P4_STAGED_LLAMA_CUDA_VISIBLE_DEVICES_STAGE0",
    );
    let mut stage1 = launch_stage(
        &binary,
        &model,
        split_layer,
        layer_end,
        "P4_STAGED_LLAMA_CUDA_VISIBLE_DEVICES_STAGE1",
    );
    stage0.start().expect("start stage0");
    stage1.start().expect("start stage1");
    let ready0 = wait_ready(&mut stage0);
    let ready1 = wait_ready(&mut stage1);
    assert_eq!(ready0.protocol_revision, PROTOCOL_REVISION);
    assert_eq!(ready1.protocol_revision, PROTOCOL_REVISION);

    let sequence_ids = ["two-real-stages-a", "two-real-stages-b"];
    let prompt = HopPayload {
        phase: HopPhase::Prefill,
        sequences: sequence_ids
            .iter()
            .enumerate()
            .map(|(index, sequence_id)| SequencePayload {
                sequence_id: (*sequence_id).into(),
                descriptors: Vec::new(),
                payloads: Vec::new(),
                n_tokens: None,
                prompt: Some(format!("{PROMPT} sequence {index}")),
                initial_tokens: None,
                options: String::new(),
                position: Some(0),
                outcome: None,
            })
            .collect(),
        legacy: false,
    }
    .encode(limits)
    .expect("encode stage0 prefill");
    let stage0_response = request(&mut stage0, Operation::Hop, prompt);
    assert_eq!(stage0_response.header.operation, Operation::HopResult);
    let stage0_result =
        HopPayload::decode(&stage0_response.body, limits).expect("decode stage0 result");
    assert_eq!(stage0_result.sequences.len(), sequence_ids.len());
    let mut cut_sets = stage0_result.sequences;
    for (sequence_id, cut_set) in sequence_ids.iter().zip(&cut_sets) {
        assert_eq!(cut_set.sequence_id, *sequence_id);
        assert!(
            !cut_set.descriptors.is_empty(),
            "stage0 returned no cut set"
        );
        assert_eq!(cut_set.descriptors.len(), cut_set.payloads.len());
        assert!(cut_set.n_tokens.expect("stage0 returned no n_tokens") > 0);
        assert!(cut_set.prompt.is_none());
    }
    let n_tokens = cut_sets[0].n_tokens.expect("stage0 returned no n_tokens");

    let stage1_input = HopPayload {
        phase: HopPhase::Prefill,
        sequences: std::mem::take(&mut cut_sets),
        legacy: false,
    }
    .encode(limits)
    .expect("encode exact stage0 cut set");
    let started = Instant::now();
    let stage1_response = request(&mut stage1, Operation::Hop, stage1_input);
    let elapsed = started.elapsed();
    assert_eq!(stage1_response.header.operation, Operation::HopResult);
    assert!(
        elapsed < Duration::from_secs(60),
        "stage1 prefill did not respond promptly: {elapsed:?}"
    );
    let stage1_result =
        HopPayload::decode(&stage1_response.body, limits).expect("decode stage1 result");
    assert_eq!(stage1_result.sequences.len(), sequence_ids.len());
    for (sequence_id, stage1_sequence) in sequence_ids.iter().zip(&stage1_result.sequences) {
        assert_eq!(stage1_sequence.sequence_id, *sequence_id);
        assert!(
            stage1_sequence.outcome.is_none(),
            "prefill must not produce a sampled token outcome"
        );
    }
    println!(
        "REAL_TWO_STAGE_MULTI_SEQUENCE sequences={} n_tokens={n_tokens} stage1_elapsed={elapsed:?} descriptors={}",
        stage1_result.sequences.len(),
        stage1_result.sequences[0].descriptors.len()
    );

    unload_and_reap(&mut stage0);
    unload_and_reap(&mut stage1);
}

fn launch_stage(
    binary: &Path,
    model: &Path,
    begin: i32,
    end: i32,
    device_environment: &str,
) -> ProcessServerControl {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve stage endpoint");
    let endpoint = listener.local_addr().expect("read stage endpoint");
    drop(listener);
    let model_text = model.to_string_lossy();
    let plan = format!(
        "--model {} --layer-begin {begin} --layer-end {end} --n-seq-max 2 \
         --ctx-size 512 --temp 0 --seed 1 --model-identity {} {}",
        quote_plan_value(&model_text),
        quote_plan_value(&model_text),
        env::var("P4_STAGED_LLAMA_PLAN_EXTRA_ARGS").unwrap_or_default(),
    );
    let mut launch = ServerLaunch::new(binary, endpoint, plan.into_bytes());
    launch.args = vec![
        OsString::from("--port"),
        endpoint.port().to_string().into(),
        OsString::from("--bind"),
        OsString::from("127.0.0.1"),
    ];
    launch.ready_timeout = ready_timeout();
    launch.io_timeout = Duration::from_secs(120);
    launch.environment = runtime_environment(binary, device_environment);
    ProcessServerControl::new(launch)
}

fn wait_ready(
    control: &mut ProcessServerControl,
) -> p4_llamacpp_staged_adapter::process::ReadyInfo {
    let deadline = Instant::now() + ready_timeout();
    loop {
        match control.wait_ready(deadline) {
            Ok(Some(ready)) => return ready,
            Ok(None) if Instant::now() >= deadline => panic!("stage server readiness timeout"),
            Ok(None) => std::thread::yield_now(),
            Err(error) => panic!("stage server readiness failed: {error}"),
        }
    }
}

fn request(control: &mut ProcessServerControl, operation: Operation, body: Vec<u8>) -> Frame {
    control
        .request(Frame::new(operation, body).expect("create request frame"))
        .unwrap_or_else(|error| panic!("{operation:?} failed: {error}"))
}

fn unload_and_reap(control: &mut ProcessServerControl) {
    let response = request(control, Operation::Unload, Vec::new());
    assert_eq!(response.header.operation, Operation::Unload);
    assert_eq!(response.body, b"UNLOADED");
    control.shutdown().expect("reap stage server");
    assert!(control.pid().is_none(), "stage server child was not reaped");
}

fn file_from_env(name: &str, description: &str) -> Option<PathBuf> {
    let Some(path) = env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
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

fn env_i32(name: &str, default: i32) -> i32 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn ready_timeout() -> Duration {
    let seconds = env::var(READY_TIMEOUT_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(180);
    Duration::from_secs(seconds)
}

fn runtime_environment(binary: &Path, device_environment: &str) -> Vec<(OsString, OsString)> {
    let mut paths = Vec::new();
    if let Some(parent) = binary.parent() {
        paths.push(parent.to_path_buf());
        if let Some(root) = parent.parent() {
            paths.push(root.join("bin").join("Release"));
        }
    }
    if let Some(path) = env::var_os("PATH") {
        paths.extend(env::split_paths(&path));
    }
    let mut environment = vec![(
        OsString::from("PATH"),
        env::join_paths(paths).expect("compose PATH"),
    )];
    if let Some(device_mask) = env::var_os(device_environment) {
        environment.push((OsString::from("CUDA_VISIBLE_DEVICES"), device_mask));
    }
    environment
}

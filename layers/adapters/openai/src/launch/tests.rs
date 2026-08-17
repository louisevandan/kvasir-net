use super::arguments;
use crate::flavour::Flavour;
use crate::plan::Plan;

fn composed(plan: &str) -> Vec<String> {
    let plan = Plan::parse(plan).expect("parsed");
    let start = plan.start.clone().expect("a start");
    arguments(Flavour::LlamaCpp, &plan, &start).expect("composed")
}

fn pair<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|found| found == flag)
        .and_then(|at| args.get(at + 1))
        .map(String::as_str)
}

const FRONT: &str = r#"{
    "endpoint":"127.0.0.1:18090","model":"m","device":"CUDA0","vram_gb":11,
    "workers":["192.168.0.29:52003","192.168.0.29:52004","127.0.0.1:50052"],
    "start":{"binary":"llama-server.exe","weights":"T:/m.gguf",
             "context":20480,"slots":16,"batch":2048,"ubatch":256}
}"#;

#[test]
fn the_plans_intent_becomes_this_backends_names() {
    let args = composed(FRONT);
    assert_eq!(pair(&args, "-m"), Some("T:/m.gguf"));
    assert_eq!(pair(&args, "-c"), Some("20480"));
    assert_eq!(pair(&args, "--parallel"), Some("16"));
    assert_eq!(pair(&args, "-b"), Some("2048"));
    assert_eq!(pair(&args, "-ub"), Some("256"));
    assert_eq!(pair(&args, "--port"), Some("18090"));
}

/// The shares held elsewhere become the devices to reach, and the devices to
/// use are named rather than left to the server.
///
/// Left to itself a front takes every local card as well as the remote ones,
/// which on a machine that also holds a worker means using the same card twice.
#[test]
fn the_workers_become_devices_and_the_device_list_is_explicit() {
    let args = composed(FRONT);
    assert_eq!(
        pair(&args, "--rpc"),
        Some("192.168.0.29:52003,192.168.0.29:52004,127.0.0.1:50052")
    );
    assert_eq!(pair(&args, "-dev"), Some("CUDA0,RPC0,RPC1,RPC2"));
}

/// Every layer on a device, always.
///
/// Left to itself the server puts what does not fit on the CPU, and a remote
/// share cannot reach a tensor in host memory — the worker refuses the graph
/// with an invalid pointer rather than running slowly. It cost four load
/// attempts to find that.
#[test]
fn nothing_is_left_for_the_host_to_hold() {
    assert_eq!(pair(&composed(FRONT), "-ngl"), Some("999"));
}

/// A worker is the other binary and takes only its device.
#[test]
fn a_worker_is_started_as_a_share_rather_than_a_server() {
    let args = composed(
        r#"{"role":"worker","endpoint":"0.0.0.0:52003","device":"CUDA1","vram_gb":23,
            "start":{"binary":"ggml-rpc-server.exe","weights":"unused",
                     "context":1,"slots":1,"batch":1,"ubatch":1}}"#,
    );
    assert_eq!(pair(&args, "-p"), Some("52003"));
    assert_eq!(pair(&args, "-d"), Some("CUDA1"));
    assert!(
        !args.iter().any(|arg| arg == "--parallel"),
        "a share serves nothing, so it has no slots: {args:?}"
    );
}

/// A single card with nothing held elsewhere.
#[test]
fn one_device_and_no_workers_names_just_the_device() {
    let args = composed(
        r#"{"endpoint":"127.0.0.1:18090","device":"CUDA0",
            "start":{"binary":"b","weights":"w","context":4096,"slots":4,
                     "batch":512,"ubatch":128}}"#,
    );
    assert_eq!(pair(&args, "-dev"), Some("CUDA0"));
    assert!(!args.iter().any(|arg| arg == "--rpc"));
}

/// What this build cannot start, it says it cannot start.
///
/// Neither vLLM nor SGLang runs on the machines this was written against, so
/// composing a command line for them would be a guess presented as knowledge.
/// Attaching to a server already there is what a plan without a `start` does,
/// and that is what those two get.
#[test]
fn a_backend_this_adapter_cannot_start_refuses_rather_than_guesses() {
    let plan = Plan::parse(FRONT).expect("parsed");
    let start = plan.start.clone().expect("a start");
    for flavour in [Flavour::Vllm, Flavour::Sglang] {
        let refused = arguments(flavour, &plan, &start).unwrap_err();
        assert!(refused.contains("cannot start a server"), "{refused}");
        assert!(refused.contains(flavour.name()), "{refused}");
    }
}

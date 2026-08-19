fn launch_stage(
    binary: &Path,
    model: &Path,
    begin: i32,
    end: i32,
) -> ProcessServerControl {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve stage endpoint");
    let endpoint = listener.local_addr().expect("read stage endpoint");
    drop(listener);
    let model_text = model.to_string_lossy();
    // Equality compares the numerical result, not two different attention
    // kernels. With AUTO, the full graph may select Flash Attention while a
    // partial graph disables it because its stage-local device assignment is
    // different. That is an execution-policy difference, not a cut-set test.
    // Pin the policy so the full and staged paths exercise the same kernels.
    let plan = format!(
        "--model {} --layer-begin {begin} --layer-end {end} --n-seq-max 1 \
         --ctx-size 512 --flash-attn 0 --temp 0 --seed 1 --model-identity {}",
        quote_plan_value(&model_text),
        quote_plan_value(&model_text),
    );
    let mut launch = ServerLaunch::new(binary.to_owned(), endpoint, plan.into_bytes());
    launch.args = vec![
        "--port".into(),
        endpoint.port().to_string().into(),
        "--bind".into(),
        "127.0.0.1".into(),
    ];
    launch.ready_timeout = Duration::from_secs(180);
    launch.io_timeout = Duration::from_secs(120);
    launch.environment = runtime_environment(binary);
    ProcessServerControl::new(launch)
}

fn wait_ready(
    control: &mut ProcessServerControl,
) -> p4_llamacpp_staged_adapter::process::ReadyInfo {
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
    assert_eq!(response.header.operation, Operation::Unload);
    assert_eq!(response.body, b"UNLOADED");
    control.shutdown().expect("reap stage server");
    assert!(control.pid().is_none(), "stage server child was not reaped");
}

fn required_file(name: &str, description: &str) -> Option<PathBuf> {
    let Some(path) = env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        println!("SKIP: set {name} to the {description} path");
        return None;
    };
    if path.is_file() {
        return Some(path);
    }
    println!("SKIP: {description} is not a file: {}", path.display());
    None
}

fn env_i32(name: &str, default: i32) -> i32 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn quote_plan_value(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

fn runtime_environment(binary: &Path) -> Vec<(OsString, OsString)> {
    let mut paths = vec![binary.to_path_buf()];
    if let Some(parent) = binary.parent() {
        paths[0] = parent.to_path_buf();
        if let Some(root) = parent.parent() {
            paths.push(root.join("bin").join("Release"));
        }
    }
    if let Some(path) = env::var_os("PATH") {
        paths.extend(env::split_paths(&path));
    }
    vec![(
        OsString::from("PATH"),
        env::join_paths(paths).expect("compose PATH"),
    )]
}

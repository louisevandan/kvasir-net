use p4_llamacpp_staged_adapter::PROTOCOL_REVISION;
use p4_llamacpp_staged_adapter::process::{ProcessServerControl, ServerControl, ServerLaunch};
use std::env;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn staged_server_binary() -> Option<PathBuf> {
    if let Some(path) = env::var_os("P4_STAGED_SERVER_BINARY") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    [
        root.join("../../../../../../../.cache/staged-server-build/Release/p4_staged_server.exe"),
        root.join("../../../../../../../.cache/staged-server-build/Release/p4_staged_server"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

#[test]
#[ignore = "requires a built C++ stage server; set P4_STAGED_SERVER_BINARY to override the default artifact"]
fn process_control_can_run_the_real_cpp_stage_server_binary() {
    let binary = staged_server_binary()
        .expect("build .cache/staged-server-build or set P4_STAGED_SERVER_BINARY");

    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve stage-server endpoint");
    let endpoint = listener.local_addr().expect("read stage-server endpoint");
    drop(listener);

    let mut launch = ServerLaunch::new(binary, endpoint, b"integration-test-plan".to_vec());
    launch.args = vec![
        "--port".into(),
        endpoint.port().to_string().into(),
        "--bind".into(),
        "127.0.0.1".into(),
    ];
    launch.ready_timeout = Duration::from_secs(5);
    launch.io_timeout = Duration::from_secs(2);

    let mut control = ProcessServerControl::new(launch);
    control.start().expect("start real C++ stage server");
    let ready = control
        .wait_ready(Instant::now() + Duration::from_secs(5))
        .expect("complete HELLO exchange")
        .expect("C++ stage server becomes ready");

    assert_eq!(ready.protocol_revision, PROTOCOL_REVISION);
    assert!(ready.server_id.starts_with("READY;"), "{ready:?}");

    control
        .shutdown()
        .expect("UNLOAD and reap C++ stage server");
    assert!(control.pid().is_none());
}

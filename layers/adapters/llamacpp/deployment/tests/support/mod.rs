//! Test-only support for `cross_wire.rs`: spawning the llama-path's real v2
//! submission-stream server as a child `node` process, and a `Sink` that
//! collects everything it raises for polling assertions.

use p4_adapter::deployment::{DeploymentEvent, Sink};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One running instance of `apps/llama`'s cross-wire-fixture.ts: the real
/// `attachRingInferenceStream` server, wired to a deterministic fake
/// backend, listening on an ephemeral loopback port.
pub struct Fixture {
    child: Child,
    pub addr: SocketAddr,
    pub deployment_id: String,
    pub deployment_generation: u64,
}

impl Fixture {
    /// Spawns the fixture via `node --import tsx`, the same loader
    /// `apps/llama`'s own `npm test` uses to run its `.ts` tests directly --
    /// no separate build step, and the same `llama_domain` package-export
    /// resolution `npm test` already depends on. Blocks until the fixture's
    /// `READY ...` line has been read from its stdout.
    pub fn spawn() -> Self {
        let apps_llama = apps_llama_dir();
        let script = fixture_script();
        assert!(
            script.exists(),
            "cross-wire-fixture.ts not found at {}",
            script.display()
        );
        let mut child = Command::new("node")
            .args([
                "--import",
                "tsx",
                script.to_str().expect("utf8 path"),
                "--port=0",
            ])
            .current_dir(&apps_llama)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn `node --import tsx cross-wire-fixture.ts` (is Node.js on PATH?)");

        let stdout = child.stdout.take().expect("piped stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = reader.read_line(&mut line).expect("read fixture stdout");
            if bytes == 0 {
                let _ = child.kill();
                panic!("cross-wire fixture exited before printing READY");
            }
            if line.starts_with("READY ") {
                break;
            }
        }
        let fields = parse_ready_line(&line);

        // Keep reading after READY. The fixture logs a line per connection
        // and per socket error, and a stdout pipe nobody drains fills up --
        // at which point the fixture's own `console.log` raises EPIPE and
        // takes the server process down mid-test. The failure looks exactly
        // like the server crashing on the traffic under test, which is a
        // long way from where it actually is.
        std::thread::spawn(move || {
            let mut discard = String::new();
            while reader.read_line(&mut discard).unwrap_or(0) > 0 {
                discard.clear();
            }
        });

        Self {
            child,
            addr: format!("127.0.0.1:{}", fields.port).parse().expect("addr"),
            deployment_id: fields.deployment_id,
            deployment_generation: fields.deployment_generation,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct ReadyLine {
    port: u16,
    deployment_id: String,
    deployment_generation: u64,
}

fn parse_ready_line(line: &str) -> ReadyLine {
    let mut port = None;
    let mut deployment_id = None;
    let mut deployment_generation = None;
    for field in line.split_whitespace().skip(1) {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        match key {
            "port" => port = value.parse().ok(),
            "deployment_id" => deployment_id = Some(value.to_string()),
            "deployment_generation" => deployment_generation = value.parse().ok(),
            _ => {}
        }
    }
    ReadyLine {
        port: port.unwrap_or_else(|| panic!("READY line carries port=: {line:?}")),
        deployment_id: deployment_id
            .unwrap_or_else(|| panic!("READY line carries deployment_id=: {line:?}")),
        deployment_generation: deployment_generation
            .unwrap_or_else(|| panic!("READY line carries deployment_generation=: {line:?}")),
    }
}

/// This crate's manifest dir is `apps/p4/layers/adapters/llamacpp/deployment`
/// -- six `..` reaches the repo root (deployment, llamacpp, adapters,
/// layers, p4, apps), and `apps/llama` sits directly under that.
/// The fixture script inside the *other* repository.
pub fn fixture_script() -> PathBuf {
    apps_llama_dir().join("src/server/pipeline-runtime-manager/submission-v2/cross-wire-fixture.ts")
}

// These targets are built only under the `cross-wire-fixture` feature, which
// is the caller saying the `apps/llama` checkout is present. So a missing
// checkout is a failure here, not a skip: the alternative - returning early
// from the body - is counted by libtest as a pass, and five tests that never
// executed were reported inside a total of 839 passing.

fn apps_llama_dir() -> PathBuf {
    // Deliberately not canonicalized: on Windows, `canonicalize()` returns a
    // `\\?\`-prefixed extended path, which `node`'s own path handling does
    // not resolve the same way `std::process::Command` does, and passing one
    // as the fixture script's argument fails before `node` ever gets far
    // enough to print a useful error. A plain path with `..` components
    // resolves correctly both as `current_dir` and as a joined file path --
    // the OS handles `..` lexically either way.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../../../../../../apps/llama")
}

/// A [`Sink`] that just remembers every event it is handed, in order, for a
/// test to poll and assert against.
///
/// `#[allow(dead_code)]` here and below: this file is `mod`-included by
/// three separate integration-test binaries (`cross_wire.rs`,
/// `broker_registry.rs`, `agent_relay.rs`), each recompiling it on its own,
/// and no single binary uses every item -- `agent_relay.rs` drives its own
/// `Agent`-level `Duties` instead of this sink. A shared fixture module is
/// not dead code; per-binary unused-item warnings on it are just an
/// artifact of how `mod support;` works across several `tests/*.rs` files.
#[allow(dead_code)]
pub struct CollectingSink(Mutex<Vec<DeploymentEvent>>);

#[allow(dead_code)]
impl CollectingSink {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(Vec::new())))
    }

    pub fn snapshot(&self) -> Vec<DeploymentEvent> {
        self.0.lock().expect("sink lock").clone()
    }

    /// Polls `snapshot()` against `predicate` until it is true, or panics
    /// once `timeout` has elapsed.
    pub fn wait_for(&self, timeout: Duration, predicate: impl Fn(&[DeploymentEvent]) -> bool) {
        let deadline = Instant::now() + timeout;
        loop {
            let events = self.snapshot();
            if predicate(&events) {
                return;
            }
            if Instant::now() > deadline {
                panic!("condition never became true within {timeout:?}; events so far: {events:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Sink for CollectingSink {
    fn raise(&self, event: DeploymentEvent) {
        self.0.lock().expect("sink lock").push(event);
    }
}

/// The backend-neutral request P4 hands to a deployment client. The client
/// must turn this into llama's chat request before it reaches the fixture.
#[allow(dead_code)]
pub fn neutral_request() -> Value {
    json!({
        "prompt": "hi",
        "max_tokens": 64,
        "options": "{\"temperature\":0,\"top_p\":1,\"top_k\":0,\"seed\":-1}"
    })
}

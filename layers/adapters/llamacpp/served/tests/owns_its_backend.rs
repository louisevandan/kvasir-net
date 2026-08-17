//! A node owns the process behind it, and lets go of it.
//!
//! This is the property that closes the gap between what a plan declares and
//! what is actually running. Attaching to whatever a person last started leaves
//! a plan free to claim eleven gibibytes of a card while the server on it holds
//! fifteen, and nothing in the protocol can tell — the plan is the only record
//! of intent and nothing ever checks it against a process.
//!
//! Tested against the operating system's own long-running and short-running
//! commands rather than against a backend. Nothing here needs llama.cpp: what
//! is fragile is the waiting and the killing, and those are the same whatever
//! the process turns out to be.

use p4_llamacpp_served::endpoint::Endpoint;
use p4_llamacpp_served::launch::process::{Ready, Running};
use std::time::{Duration, Instant};

/// A command that stays up for a minute, on whichever machine this is.
fn sleeper() -> (&'static str, Vec<String>) {
    #[cfg(windows)]
    {
        // `ping` rather than `timeout`, which needs a console this process is
        // started without and refuses redirected input.
        (
            "ping.exe",
            vec!["-n".into(), "60".into(), "127.0.0.1".into()],
        )
    }
    #[cfg(not(windows))]
    {
        ("sleep", vec!["60".into()])
    }
}

/// A command that gives up at once, with a status worth reporting.
fn exiter() -> (&'static str, Vec<String>) {
    #[cfg(windows)]
    {
        ("cmd.exe", vec!["/c".into(), "exit 7".into()])
    }
    #[cfg(not(windows))]
    {
        ("sh", vec!["-c".into(), "exit 7".into()])
    }
}

/// Somewhere nothing is listening, so `WhenItAnswers` can never be satisfied.
fn nowhere() -> Endpoint {
    Endpoint::parse("127.0.0.1:1").expect("an address")
}

/// A share is ready when it has not exited, because nothing can be asked of it.
///
/// The first thing tried was one question for both halves of a distributed
/// load, and it does not work: a share holds weights over llama.cpp's own RPC
/// protocol and serves no HTTP at all, so waiting for it to answer `/v1/models`
/// waits out the whole patience and then reports a healthy worker as a failure.
/// A worker load would have failed on every correct deployment.
#[test]
fn a_share_is_ready_once_it_has_not_exited() {
    let (binary, arguments) = sleeper();
    let running = Running::start(
        binary,
        &arguments,
        &nowhere(),
        Ready::WhenItHasNotExited(Duration::from_millis(300)),
        Duration::from_secs(30),
        |_, _| {},
    )
    .expect("a process that stays up is ready");
    assert!(running.pid() > 0, "it names the process it started");
}

/// A share that gave up during its settle is not called started.
#[test]
fn a_share_that_exits_while_settling_is_a_failure() {
    let (binary, arguments) = exiter();
    let refused = Running::start(
        binary,
        &arguments,
        &nowhere(),
        Ready::WhenItHasNotExited(Duration::from_secs(3)),
        Duration::from_secs(30),
        |_, _| {},
    )
    .err()
    .expect("a process that exited is not a share");
    assert!(refused.contains("exited while loading"), "{refused}");
}

/// A backend that exits says so, rather than being waited out.
///
/// The patience here is minutes because reading seventy gibibytes takes
/// minutes. Spending all of it on a process that gave up in the first second —
/// a missing weights file, a device that is not there — would turn a clear
/// failure into a load that appears to hang, which is the harder thing to
/// diagnose and the one an operator is least able to interpret.
#[test]
fn a_backend_that_gives_up_is_reported_rather_than_waited_out() {
    let (binary, arguments) = exiter();
    let began = Instant::now();
    let refused = Running::start(
        binary,
        &arguments,
        &nowhere(),
        Ready::WhenItAnswers,
        Duration::from_secs(600),
        |_, _| {},
    )
    .err()
    .expect("a process that exited never answers");
    assert!(refused.contains("exited while loading"), "{refused}");
    assert!(
        began.elapsed() < Duration::from_secs(30),
        "it waited {:?} of a ten-minute patience on a process that was already \
         gone",
        began.elapsed()
    );
}

/// Letting go of it kills it.
///
/// The whole point of the node holding the child. A backend left behind holds
/// the whole of a card, and the next load then cannot fit into it — reported by
/// llama.cpp as a memory error, which names the symptom and not the cause.
#[test]
fn letting_go_of_a_backend_kills_it() {
    let (binary, arguments) = sleeper();
    let running = Running::start(
        binary,
        &arguments,
        &nowhere(),
        Ready::WhenItHasNotExited(Duration::from_millis(300)),
        Duration::from_secs(30),
        |_, _| {},
    )
    .expect("started");
    let pid = running.pid();
    assert!(alive(pid), "it is running before being let go of");
    drop(running);
    assert!(!alive(pid), "pid {pid} outlived the node that started it");
}

/// Asked of the operating system rather than of our own record of it, because
/// our own record is the thing under test.
#[cfg(windows)]
fn alive(pid: u32) -> bool {
    let listed = std::process::Command::new("tasklist.exe")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&listed.stdout).contains(&pid.to_string())
}

#[cfg(not(windows))]
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .expect("kill -0")
        .success()
}

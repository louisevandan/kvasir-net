//! Starting the backend a load asked for, and stopping it when the load is let
//! go of.
//!
//! A node owns the lifetime of the thing behind it. Anything else leaves the
//! placement in whatever command line a person last typed, which is exactly
//! where it was: a plan could declare eleven gibibytes on a card and the server
//! could be holding fifteen, and nothing in the protocol would know. What a
//! load establishes now is a process that exists because this load exists.
//!
//! Separated from composing the arguments because the arguments are a pure
//! function of a plan and this is not — this waits, kills, and can fail for
//! reasons that have nothing to do with what was asked for.

use crate::endpoint::Endpoint;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// What counts as this process having started.
///
/// Two answers, because the two halves of a distributed load are two different
/// programs. Asking one question of both was the first thing tried and it does
/// not work: a share holds weights over llama.cpp's own RPC protocol and serves
/// no HTTP at all, so waiting for it to answer `/v1/models` waits out the whole
/// patience and then reports a healthy worker as a failure.
pub enum Ready {
    /// It answers HTTP once it holds the weights. The honest signal, used
    /// wherever there is one.
    WhenItAnswers,
    /// Nothing can be asked of it, so all that is established is that it did
    /// not immediately give up.
    ///
    /// A share cannot be probed even in principle: one already serving a front
    /// refuses further connections, and that refusal is byte-identical to an
    /// empty port — a probe that cannot tell "held" from "absent" fails on
    /// exactly the healthy deployment. What proves a share is held is the front
    /// that reaches across it, which will not start against an RPC device it
    /// cannot reach. So this waits the stated moment, checks the process is
    /// still alive, and leaves the proof where the evidence actually is.
    WhenItHasNotExited(Duration),
}

/// A backend this node started, which it will stop.
pub struct Running {
    child: Child,
}

impl Running {
    /// Starts the process and waits until it is ready, or says why not.
    ///
    /// Readiness is asked repeatedly rather than once: a server holding tens of
    /// gibibytes reads them before it listens, and on these machines that is
    /// minutes rather than seconds. `patience` is how long that may take, and
    /// it is the caller's number because only the caller knows how large the
    /// weights are and where they are coming from.
    pub fn start(
        binary: &str,
        arguments: &[String],
        endpoint: &Endpoint,
        ready: Ready,
        patience: Duration,
        mut progress: impl FnMut(u32, String),
    ) -> Result<Self, String> {
        let mut command = Command::new(binary);
        command
            .args(arguments)
            // Inherited output would interleave a backend's log with the
            // agent's own, and the agent's is a protocol surface.
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        windowless(&mut command);
        let child = command
            .spawn()
            .map_err(|error| format!("cannot start {binary}: {error}"))?;
        let mut running = Self { child };

        let began = Instant::now();
        let settle = match ready {
            Ready::WhenItAnswers => patience,
            Ready::WhenItHasNotExited(settle) => settle.min(patience),
        };
        let mut told = 0;
        while began.elapsed() < settle {
            // A process that has already given up will never answer, and
            // waiting out the patience to say so wastes the whole of it.
            running.still_alive(binary)?;
            if matches!(ready, Ready::WhenItAnswers) && endpoint.get("/v1/models").is_ok() {
                return Ok(running);
            }
            let waited = began.elapsed().as_secs() as u32;
            if waited >= told + 30 {
                told = waited;
                progress(50, format!("{binary} has been loading for {waited}s"));
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        match ready {
            // The settle elapsed without it dying, which is the whole of what
            // can be established here.
            Ready::WhenItHasNotExited(_) => {
                running.still_alive(binary)?;
                Ok(running)
            }
            Ready::WhenItAnswers => Err(format!(
                "{binary} did not answer within {patience:?}; it is still \
                 starting or it will not start"
            )),
        }
    }

    /// Which process this is, for whoever has to look.
    ///
    /// Reported rather than kept private because it is the one thing that lets
    /// somebody outside P4 join what the protocol says to what the machine
    /// shows: a node claiming a card and a process holding one are otherwise
    /// two facts with nothing connecting them.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Fails with what the process exited with, if it has.
    fn still_alive(&mut self, binary: &str) -> Result<(), String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Err(format!("{binary} exited while loading: {status}")),
            Ok(None) => Ok(()),
            Err(error) => Err(format!("cannot watch {binary}: {error}")),
        }
    }
}

/// Starts the backend without a console of its own.
///
/// A node may hold several backends and a machine several nodes, so a window
/// per process turns a working fleet into a desktop full of empty terminals —
/// which is also a window a person can close, taking the backend with it.
/// Nothing is lost by hiding it: the output is already discarded, and what an
/// operator needs is in the agent's own report.
#[cfg(windows)]
fn windowless(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

/// Nothing to do: a process started here has no console to begin with.
#[cfg(not(windows))]
fn windowless(_command: &mut Command) {}

impl Drop for Running {
    fn drop(&mut self) {
        // Killed rather than asked. There is no graceful stop on this surface,
        // and a backend left behind holds the whole of a card — which the next
        // load then cannot fit into, with nothing to say why.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

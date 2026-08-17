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

/// A backend this node started, which it will stop.
pub struct Running {
    child: Child,
}

impl Running {
    /// Starts the process and waits until it answers, or says why not.
    ///
    /// `ready` is asked repeatedly rather than once: a server holding tens of
    /// gibibytes reads them before it listens, and on these machines that is
    /// minutes rather than seconds. `patience` is how long that may take, and
    /// it is the caller's number because only the caller knows how large the
    /// weights are and where they are coming from.
    pub fn start(
        binary: &str,
        arguments: &[String],
        endpoint: &Endpoint,
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
        let mut told = 0;
        while began.elapsed() < patience {
            // A process that has already given up will never answer, and
            // waiting out the patience to say so wastes the whole of it.
            match running.child.try_wait() {
                Ok(Some(status)) => {
                    return Err(format!("{binary} exited while loading: {status}"));
                }
                Ok(None) => {}
                Err(error) => return Err(format!("cannot watch {binary}: {error}")),
            }
            if endpoint.get("/v1/models").is_ok() {
                return Ok(running);
            }
            let waited = began.elapsed().as_secs() as u32;
            if waited >= told + 30 {
                told = waited;
                progress(50, format!("{binary} has been loading for {waited}s"));
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        Err(format!(
            "{binary} did not answer within {:?}; it is still starting or it \
             will not start",
            patience
        ))
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

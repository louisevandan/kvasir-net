//! What a load has to get right, and what an unload has to let go of.
//!
//! Its own file because it changes for different reasons than the rest of the
//! adapter. Driving a completion is a function of one HTTP surface and moves
//! when that surface moves — which is close to never, since three servers
//! implement it. This is process lifetime and placement: it moves when a
//! backend gains a flag, when a machine gains a card, or when someone changes
//! their mind about who is responsible for a running server.
//!
//! ## Why the node owns the process
//!
//! Attaching to whatever a person last started leaves a plan free to claim
//! eleven gibibytes of a card while the server on it holds fifteen, and nothing
//! in the protocol able to tell: the plan is the only record of intent and
//! nothing ever checks it against a process. A plan carrying a `start` closes
//! that — the process exists because this load exists, and stops existing when
//! the load does.

use super::{Served, chat};
use crate::launch::{
    self,
    process::{Ready, Running},
};
use crate::plan::{Plan, Role, Start};
use p4_adapter::{Allocation, Event, EventSink};
use std::sync::atomic::Ordering;
use std::time::Duration;

impl Served {
    pub(super) fn load(&self, deployment: String, plan: &str, events: &dyn EventSink) {
        let parsed = match self.establish(&deployment, plan, events) {
            Ok(parsed) => parsed,
            Err(detail) => {
                // A load that did not finish leaves nothing behind. Whatever
                // was started for it is killed here rather than left holding a
                // card, because the next attempt would then fail to fit into a
                // card that the failed attempt is still occupying — and it
                // would fail with a memory error, which names the symptom and
                // not this.
                self.release();
                return events.raise(Event::Failed {
                    deployment,
                    sequence: None,
                    detail,
                });
            }
        };

        let allocations = vec![Allocation {
            category: parsed.share(),
            // The declared claim, not a measurement. The server owns its own
            // memory and does not report a reservation through this surface,
            // and a figure invented here would be worse than a declared one.
            bytes: parsed.vram_gb.unwrap_or(0) * 1024 * 1024 * 1024,
        }];
        *self.plan.lock().expect("plan lock") = Some(parsed);
        events.raise(Event::LoadProgress {
            deployment: deployment.clone(),
            stage: 0,
            percent: 100,
            detail: "share held".into(),
        });
        events.raise(Event::Loaded {
            deployment,
            generation: self.generation.fetch_add(1, Ordering::SeqCst) + 1,
            allocations,
        });
    }

    /// Everything a load has to get right, in the order it has to happen.
    ///
    /// Separated from `load` so that every way it can fail arrives at one
    /// place. It used to raise `Failed` from five points and return, and each
    /// of those is now somewhere a started backend has to be killed — a thing
    /// that is easy to add at four of five sites and worth not relying on.
    fn establish(
        &self,
        deployment: &str,
        plan: &str,
        events: &dyn EventSink,
    ) -> Result<Plan, String> {
        let mut parsed = Plan::parse(plan)?;
        if let Some(start) = parsed.start.clone() {
            self.bring_up(deployment, &parsed, &start, events)?;
        }

        // Reaching the backend is the rest of the load. Where this node started
        // it, that has already been established by waiting for it; where the
        // plan named an endpoint and nothing else, this is the whole of what a
        // load can establish, and it is the thing that fails.
        events.raise(Event::LoadProgress {
            deployment: deployment.to_owned(),
            stage: 0,
            percent: 50,
            detail: format!(
                "reaching {} at {}:{}",
                parsed.share(),
                parsed.endpoint.host,
                parsed.endpoint.port
            ),
        });

        // Only a front is asked anything. A worker speaks llama.cpp's RPC
        // protocol rather than HTTP, and the one question TCP could answer —
        // is something listening — it cannot answer usefully: an RPC worker
        // serving a front refuses further connections, and a refusal from a
        // busy worker is byte-identical to a refusal from an empty port. A
        // probe that cannot tell "held" from "absent" is worse than none,
        // because it fails on exactly the healthy deployment.
        //
        // What proves the shares are held is the front. llama.cpp will not
        // start against an RPC device it cannot reach, and will not answer a
        // token across one that died, so a front that serves is a deployment
        // whose workers are present — verified where the evidence actually is.
        if parsed.role == Role::Front {
            let listed = parsed
                .endpoint
                .get("/v1/models")
                .map_err(|error| format!("{} not reachable: {error}", parsed.share()))?;
            // The answer is read rather than discarded only where it has to be.
            // vLLM matches a request's model against what it serves and answers
            // 404 to anything else, so a plan that did not name one would fail
            // on the first inference instead of on the load — which is the
            // failure worth moving, because a load is where an operator is
            // still watching.
            if self.flavour.insists_on_the_model_name() && !parsed.names_the_model {
                let served = chat::first_model(&listed).ok_or_else(|| {
                    format!(
                        "{} lists no model and the plan names none, so a request \
                         would be refused as a model that does not exist",
                        self.flavour.name()
                    )
                })?;
                events.raise(Event::LoadProgress {
                    deployment: deployment.to_owned(),
                    stage: 0,
                    percent: 75,
                    detail: format!("serving {served}, which the plan did not name"),
                });
                parsed.model = served;
            }
        }
        for (index, worker) in parsed.workers.iter().enumerate() {
            events.raise(Event::LoadProgress {
                deployment: deployment.to_owned(),
                stage: (index + 1) as u32,
                percent: 100,
                detail: format!("share held at {}:{}", worker.host, worker.port),
            });
        }
        Ok(parsed)
    }

    /// Starts the backend this plan asked for, and waits for it.
    ///
    /// This is what makes the placement true rather than declared. Attaching to
    /// whatever a person last started leaves a plan free to claim eleven
    /// gibibytes of a card while the server on it holds fifteen, with nothing
    /// in the protocol able to tell — the plan is the only record of intent and
    /// nothing ever checks it against a process.
    fn bring_up(
        &self,
        deployment: &str,
        plan: &Plan,
        start: &Start,
        events: &dyn EventSink,
    ) -> Result<(), String> {
        // Before anything is started, because the card is what is scarce: a
        // previous backend still holding it means the new one cannot fit, and
        // llama.cpp reports that as a memory error rather than as a reload.
        self.release();

        let arguments = launch::arguments(self.flavour, plan, start)?;
        events.raise(Event::LoadProgress {
            deployment: deployment.to_owned(),
            stage: 0,
            percent: 25,
            // The whole command line, once, where a caller elsewhere can read
            // it. This was the single most useful line while getting a
            // distributed load to work and it existed only in a shell's
            // scrollback: which devices, which shares, how wide a batch.
            detail: format!("starting {} {}", start.binary, arguments.join(" ")),
        });

        let ready = match plan.role {
            Role::Front => Ready::WhenItAnswers,
            // A share serves no HTTP, so there is nothing to wait for it to
            // answer. See `Ready` for why it cannot be probed either.
            Role::Worker => Ready::WhenItHasNotExited(Duration::from_secs(5)),
        };
        let running = Running::start(
            &start.binary,
            &arguments,
            &plan.endpoint,
            ready,
            start.patience,
            |percent, detail| {
                events.raise(Event::LoadProgress {
                    deployment: deployment.to_owned(),
                    stage: 0,
                    percent,
                    detail,
                });
            },
        )?;
        *self.backend.lock().expect("backend lock") = Some(running);
        self.started.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Lets go of a backend this node started, killing it.
    ///
    /// Does nothing when the plan attached to a server it did not start, which
    /// is the point of the distinction: a node kills what it owns and never
    /// what it borrowed.
    pub(super) fn release(&self) {
        if self.backend.lock().expect("backend lock").take().is_some() {
            self.stopped.fetch_add(1, Ordering::Relaxed);
        }
    }

}

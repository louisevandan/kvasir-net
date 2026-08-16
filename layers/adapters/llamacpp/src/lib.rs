//! llama.cpp, as one self-contained node.
//!
//! `Distribution::Internal`: the backend holds the whole model and presents one
//! entry point, so a chain over it is one link and a lap is a decode step on
//! the same node. The staged shape — a model split across machines with P4
//! owning the boundary — is the other adapter, and shares none of this file
//! except the interface.
//!
//! What is llama.cpp-specific here is small: which endpoint to reach and what a
//! plan means. Everything about the conversation is the OpenAI-compatible
//! surface, which is why vLLM and SGLang are the same adapter with a different
//! process to start.
//!
//! ## The plan
//!
//! Opaque above the boundary, read here. A JSON object:
//!
//! ```json
//! { "endpoint": "127.0.0.1:8080", "model": "qwen", "patience_ms": 120000 }
//! ```
//!
//! `endpoint` is required — this adapter attaches to a server rather than
//! starting one, because process supervision on each operating system is a
//! solved problem that belongs to whatever already does it on that machine,
//! and an adapter that forked a GPU process would own restarts, logs and
//! zombies for no gain.

pub mod chat;
pub mod endpoint;
pub mod plan;
pub mod session;

use p4_adapter::{Adapter, Distribution, Event, EventSink, Hop, Outcome, Phase, Work};
use plan::Plan;
use session::{Next, Session};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct LlamaCpp {
    /// Where the model is served from, once a load has said so.
    plan: Mutex<Option<Plan>>,
    generation: AtomicU64,
    /// One open completion per sequence, which is that sequence's state.
    sessions: Mutex<HashMap<String, Session>>,
}

impl Default for LlamaCpp {
    fn default() -> Self {
        Self::new()
    }
}

impl LlamaCpp {
    pub fn new() -> Self {
        Self {
            plan: Mutex::new(None),
            generation: AtomicU64::new(0),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn load(&self, deployment: String, plan: &str, events: &dyn EventSink) {
        let parsed = match Plan::parse(plan) {
            Ok(parsed) => parsed,
            Err(detail) => {
                return events.raise(Event::Failed {
                    deployment,
                    sequence: None,
                    detail,
                });
            }
        };
        // Reaching the backend is the load. Nothing is materialised here — the
        // server already holds the weights — so what a load establishes is
        // that it is there and answering, which is the thing that fails.
        events.raise(Event::LoadProgress {
            deployment: deployment.clone(),
            stage: 0,
            percent: 50,
            detail: format!("reaching {}:{}", parsed.endpoint.host, parsed.endpoint.port),
        });
        if let Err(error) = parsed.endpoint.get("/v1/models") {
            return events.raise(Event::Failed {
                deployment,
                sequence: None,
                detail: format!("backend not reachable: {error}"),
            });
        }
        *self.plan.lock().expect("plan lock") = Some(parsed);
        events.raise(Event::LoadProgress {
            deployment: deployment.clone(),
            stage: 0,
            percent: 100,
            detail: "backend answering".into(),
        });
        events.raise(Event::Loaded {
            deployment,
            generation: self.generation.fetch_add(1, Ordering::SeqCst) + 1,
            // The server owns its own memory and does not report a reservation
            // through this surface. Declaring a figure we did not measure would
            // be worse than declaring none.
            allocations: Vec::new(),
        });
    }

    fn hop(&self, hop: Hop, events: &dyn EventSink) {
        let Some(plan) = self.plan.lock().expect("plan lock").clone() else {
            return events.raise(Event::Failed {
                deployment: hop.deployment,
                sequence: None,
                detail: "no plan: this node was never loaded".into(),
            });
        };
        let mut outcomes = Vec::with_capacity(hop.sequences.len());
        for sequence in &hop.sequences {
            outcomes.push(self.advance(&plan, sequence, hop.phase, events, &hop.deployment));
        }
        events.raise(Event::HopComplete {
            deployment: hop.deployment,
            outcomes: outcomes.into_iter().flatten().collect(),
        });
    }

    /// One sequence, one token.
    ///
    /// A prefill opens the stream; every later hop takes the next token from
    /// it. `None` means the sequence has already been reported failed.
    fn advance(
        &self,
        plan: &Plan,
        sequence: &p4_adapter::Sequence,
        phase: Phase,
        events: &dyn EventSink,
        deployment: &str,
    ) -> Option<Outcome> {
        if phase == Phase::Prefill {
            let prompt = sequence.prompt.clone().unwrap_or_default();
            match Session::start(
                &plan.endpoint,
                &plan.model,
                &prompt,
                sequence.remaining.max(1),
                &sequence.options,
            ) {
                Ok(session) => {
                    self.sessions
                        .lock()
                        .expect("sessions lock")
                        .insert(sequence.sequence.clone(), session);
                }
                Err(detail) => {
                    events.raise(Event::Failed {
                        deployment: deployment.to_owned(),
                        sequence: Some(sequence.sequence.clone()),
                        detail,
                    });
                    return None;
                }
            }
        }

        let mut sessions = self.sessions.lock().expect("sessions lock");
        let Some(session) = sessions.get_mut(&sequence.sequence) else {
            events.raise(Event::Failed {
                deployment: deployment.to_owned(),
                sequence: Some(sequence.sequence.clone()),
                detail: "no open stream: this sequence never prefilled".into(),
            });
            return None;
        };
        match session.token(plan.patience) {
            Next::Token { text, position } => Some(Outcome {
                sequence: sequence.sequence.clone(),
                text,
                position,
                stop: None,
            }),
            Next::Done(reason) => {
                sessions.remove(&sequence.sequence);
                Some(Outcome {
                    sequence: sequence.sequence.clone(),
                    text: String::new(),
                    position: sequence.position,
                    stop: Some(reason),
                })
            }
            Next::Failed(detail) => {
                sessions.remove(&sequence.sequence);
                events.raise(Event::Failed {
                    deployment: deployment.to_owned(),
                    sequence: Some(sequence.sequence.clone()),
                    detail,
                });
                None
            }
        }
    }
}

impl Adapter for LlamaCpp {
    fn distribution(&self) -> Distribution {
        Distribution::Internal
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        match work {
            Work::Load(load) => self.load(load.deployment, &load.plan, events),
            Work::Unload(unload) => {
                // The server keeps its weights; what this node holds is the
                // conversations, and those are what a release lets go.
                self.plan.lock().expect("plan lock").take();
                self.sessions.lock().expect("sessions lock").clear();
                events.raise(Event::Unloaded {
                    deployment: unload.deployment,
                })
            }
            Work::Hop(hop) => self.hop(hop, events),
            Work::Cache(cache) => events.raise(Event::Failed {
                deployment: cache.deployment,
                sequence: Some(cache.sequence),
                // Said plainly rather than answered with a shrug. llama.cpp can
                // save sequence state — `llama_state_seq_save_file` — but not
                // through the OpenAI surface this adapter speaks, so the
                // capability belongs to the adapter that drives the process
                // directly. A caller must be able to tell "not here" from
                // "done", or it will believe a conversation was saved.
                detail: "this backend surface cannot persist sequence state".into(),
            }),
        }
    }
}

//! Stock `llama-server`, as one self-contained node.
//!
//! This is llama.cpp's adapter for the shape llama.cpp already supports:
//! one process holding the whole model behind one completions endpoint. The
//! other shape is `../staged/`, which splits a model across machines with P4
//! owning the boundary and needs internals llama.cpp does not expose. Two
//! arrangements of one backend, which is why they are folders under it.
//!
//! `Distribution::Internal`: the backend holds the whole model and presents one
//! entry point, so a chain over it is one link and a lap is a decode step on
//! the same node.
//!
//! ## Why vLLM and SGLang are registered from llama.cpp's folder
//!
//! Because all three copied the same HTTP from OpenAI — a model list and a
//! streamed chat completion — and that is the whole coupling. Writing it three
//! times would be three copies of one file diverging.
//!
//! It lived in a folder called `openai/` for exactly that reason and the name
//! was wrong: it read as though the agent offered an OpenAI-compatible API,
//! which it does not and will not. OUTER is a bidirectional socket; a service
//! API is OUTER's concern and no part of this layer's. What the folder actually
//! held was llama.cpp's adapter, so it is filed under llama.cpp.
//!
//! Two things differ per backend, both in `flavour`. vLLM refuses a model name
//! it does not serve, so a load against it asks what it is holding. And only
//! llama.cpp can be *started* here — `launch` composes its flags and nothing
//! else's, because knowing what a placement becomes on the command line is
//! knowledge about one server. The other two attach to something already
//! running, which is what any plan without a `start` does.
//!
//! ## The plan
//!
//! Opaque above the boundary, read here. A JSON object:
//!
//! ```json
//! { "endpoint": "127.0.0.1:8080", "model": "qwen", "patience_ms": 120000 }
//! ```
//!
//! `endpoint` is always required: it is where this node's backend answers,
//! whether this node started it or found it. Adding `start` makes the node own
//! that process — brought up at load, killed at unload — which is what closes
//! the gap between a plan declaring eleven gibibytes on a card and a server
//! somebody else launched holding fifteen.

pub mod chat;
pub mod endpoint;
pub mod flavour;
pub mod launch;
pub mod plan;
pub mod report;
pub mod session;

use flavour::Flavour;
use p4_adapter::{Adapter, Allocation, Distribution, Event, EventSink, Hop, Outcome, Phase, Work};
use plan::{Plan, Role};
use session::{Next, Session};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Served {
    /// Which server is behind the surface. Read at load, never on the wire.
    flavour: Flavour,
    /// Where the model is served from, once a load has said so.
    plan: Mutex<Option<Plan>>,
    generation: AtomicU64,
    /// One open completion per sequence, which is that sequence's state.
    sessions: Mutex<HashMap<String, Session>>,
    /// What has happened at the socket, for whoever asks what this backend is
    /// doing. Every one of these was needed to diagnose something this session
    /// and none of them was visible: they came from the machine's socket table
    /// and the server's own log, which nobody elsewhere can read.
    opened: AtomicU64,
    reopened: AtomicU64,
    refused: AtomicU64,
    finished: AtomicU64,
}

impl Served {
    pub fn new(flavour: Flavour) -> Self {
        Self {
            flavour,
            plan: Mutex::new(None),
            generation: AtomicU64::new(0),
            sessions: Mutex::new(HashMap::new()),
            opened: AtomicU64::new(0),
            reopened: AtomicU64::new(0),
            refused: AtomicU64::new(0),
            finished: AtomicU64::new(0),
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
        let mut parsed = parsed;
        if parsed.role == Role::Front {
            let listed = match parsed.endpoint.get("/v1/models") {
                Ok(listed) => listed,
                Err(error) => {
                    return events.raise(Event::Failed {
                        deployment,
                        sequence: None,
                        detail: format!("{} not reachable: {error}", parsed.share()),
                    });
                }
            };
            // The answer is read rather than discarded only where it has to be.
            // vLLM matches a request's model against what it serves and answers
            // 404 to anything else, so a plan that did not name one would fail
            // on the first inference instead of on the load — which is the
            // failure worth moving, because a load is where an operator is
            // still watching.
            if self.flavour.insists_on_the_model_name() && !parsed.names_the_model {
                match chat::first_model(&listed) {
                    Some(served) => {
                        events.raise(Event::LoadProgress {
                            deployment: deployment.clone(),
                            stage: 0,
                            percent: 75,
                            detail: format!("serving {served}, which the plan did not name"),
                        });
                        parsed.model = served;
                    }
                    None => {
                        return events.raise(Event::Failed {
                            deployment,
                            sequence: None,
                            detail: format!(
                                "{} lists no model and the plan names none, so a request \
                                 would be refused as a model that does not exist",
                                self.flavour.name()
                            ),
                        });
                    }
                }
            }
        }
        for (index, worker) in parsed.workers.iter().enumerate() {
            events.raise(Event::LoadProgress {
                deployment: deployment.clone(),
                stage: (index + 1) as u32,
                percent: 100,
                detail: format!("share held at {}:{}", worker.host, worker.port),
            });
        }

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

    fn hop(&self, hop: Hop, events: &dyn EventSink) {
        let Some(plan) = self.plan.lock().expect("plan lock").clone() else {
            return events.raise(Event::Failed {
                deployment: hop.deployment,
                sequence: None,
                detail: "no plan: this node was never loaded".into(),
            });
        };
        if plan.role == Role::Worker {
            // Said rather than silently accepted. A worker holds a share and
            // has no completions surface; work sent here was addressed to the
            // wrong half of the deployment, and a caller that could not tell
            // would wait for tokens that were never going to come.
            return events.raise(Event::Failed {
                deployment: hop.deployment,
                sequence: hop.sequences.first().map(|s| s.sequence.clone()),
                detail: format!(
                    "this node holds {} and does not serve; address the front",
                    plan.share()
                ),
            });
        }
        let refused = match hop.phase {
            Phase::Prefill => self.open(&plan, &hop, events),
            Phase::Decode => HashSet::new(),
        };
        let mut outcomes = Vec::with_capacity(hop.sequences.len());
        for sequence in &hop.sequences {
            if refused.contains(&sequence.sequence) {
                continue;
            }
            outcomes.push(self.advance(&plan, sequence, events, &hop.deployment));
        }
        events.raise(Event::HopComplete {
            deployment: hop.deployment,
            outcomes: outcomes.into_iter().flatten().collect(),
        });
    }

    /// Opens every prefill in the window at once, and names the ones that
    /// could not open.
    ///
    /// Together rather than one after another, because a window produces
    /// nothing until the whole of it has been dispatched: opening in sequence
    /// made the first token of a window arrive after the *sum* of its
    /// prefills, so a window of sixty-three spent a minute and a half silent
    /// with both cards idle, and a caller watching for progress gave up before
    /// a single token existed. Opened together, that wait is the slowest one
    /// instead of all of them.
    ///
    /// A thread per sequence, bounded by the window, which the declared
    /// ceiling already bounds. Each does one blocking request and ends.
    fn open(&self, plan: &Plan, hop: &Hop, events: &dyn EventSink) -> HashSet<String> {
        let opened: Vec<(String, Result<Session, String>)> = std::thread::scope(|scope| {
            let threads: Vec<_> = hop
                .sequences
                .iter()
                .map(|sequence| {
                    scope.spawn(move || {
                        let prompt = sequence.prompt.clone().unwrap_or_default();
                        (
                            sequence.sequence.clone(),
                            Session::start(
                                &plan.endpoint,
                                &plan.model,
                                &prompt,
                                sequence.remaining.max(1),
                                &sequence.options,
                            ),
                        )
                    })
                })
                .collect();
            threads
                .into_iter()
                .map(|thread| thread.join().expect("a prefill thread panicked"))
                .collect()
        });

        // Raised here rather than inside the threads: an event sink crosses
        // threads safely, but reporting from the one place keeps the order a
        // caller sees the same as the order it asked in.
        let mut refused = HashSet::new();
        let mut sessions = self.sessions.lock().expect("sessions lock");
        for (sequence, outcome) in opened {
            match outcome {
                Ok(session) => {
                    self.opened.fetch_add(1, Ordering::Relaxed);
                    if session.was_retried() {
                        self.reopened.fetch_add(1, Ordering::Relaxed);
                    }
                    sessions.insert(sequence, session);
                }
                Err(detail) => {
                    self.refused.fetch_add(1, Ordering::Relaxed);
                    refused.insert(sequence.clone());
                    events.raise(Event::Failed {
                        deployment: hop.deployment.clone(),
                        sequence: Some(sequence),
                        detail,
                    });
                }
            }
        }
        refused
    }

    /// One sequence, one token.
    ///
    /// The stream was opened by `open`; every hop takes the next token from
    /// it. `None` means the sequence has already been reported failed.
    fn advance(
        &self,
        plan: &Plan,
        sequence: &p4_adapter::Sequence,
        events: &dyn EventSink,
        deployment: &str,
    ) -> Option<Outcome> {
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
                self.finished.fetch_add(1, Ordering::Relaxed);
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

impl Adapter for Served {
    fn distribution(&self) -> Distribution {
        Distribution::Internal
    }

    fn report(&self) -> String {
        self.state()
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

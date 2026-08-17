//! A concrete adapter with no backend.
//!
//! It implements the whole adapter interface against arithmetic, so a run
//! exercises P4 — framing, routing, admission, windows, cancellation,
//! deadlines — with nothing underneath that could be at fault. When something
//! goes wrong against this adapter, there is no GPU to blame.
//!
//! What it reproduces is the workload as measured, not a generic delay: a load
//! spread over stages, hop cost belonging to a chain position, prefill dearer
//! than a lap, and a window it reports rather than invents.

pub mod cache;
pub mod profile;
mod runtime;

use p4_adapter::{Allocation, Distribution, Event, EventSink, Hop, Outcome, Phase};
use profile::{Fault, Profile};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// How far a sequence has got: this turn, and over its life.
///
/// Two numbers because they answer different questions. The turn decides
/// when this request stops; the lifetime is how much state there is, which is
/// what a persisted copy costs.
#[derive(Clone, Copy, Debug, Default)]
struct Progress {
    turn: u32,
    lifetime: u32,
}

pub struct Mock {
    profile: Profile,
    distribution: Distribution,
    /// Chain position this node sits at. Cost belongs to the position, so the
    /// adapter has to be told which one it is playing.
    position: usize,
    /// Whether this is the end of the chain. Only the end holds logits, so
    /// only the end produces a token or declares a sequence finished.
    terminal: bool,
    generation: AtomicU64,
    /// Every window width this adapter was given, so a test can prove the node
    /// batched rather than serialised.
    widths: Mutex<Vec<usize>>,
    /// Nanoseconds this node had nothing in the adapter between two hops.
    ///
    /// Only the gaps between hops, so the ramp before the first and the drain
    /// after the last are excluded: what is being asked is whether a stage
    /// rests while work is queued behind it, not whether a chain fills and
    /// empties. Busy over busy-plus-idle is the utilisation a card would show.
    idle: AtomicU64,
    /// When the last hop ended, for measuring that gap.
    rested: Mutex<Option<std::time::Instant>>,
    /// Nanoseconds spent inside a hop, so a chain can be asked the question the
    /// old runtime measured: stage compute over wall clock. Below one, the
    /// stages took turns; above it, they worked at the same time. It is the
    /// only number that tells pipelining from a queue.
    busy: AtomicU64,
    /// Hops in flight. Must never exceed one for a deployment: a node starts
    /// the next only when it sees the previous end.
    running: AtomicUsize,
    peak_running: AtomicUsize,
    /// Tokens produced so far, per sequence.
    ///
    /// A backend remembers this; the request does not carry it back down. The
    /// KV a sequence occupies lives here too, which is the same reason: state
    /// belongs to whoever is holding the sequence open.
    produced: Mutex<HashMap<String, Progress>>,
    /// Sequences whose state has been written somewhere durable, and how big
    /// each copy is.
    ///
    /// A real backend has a file or a store; what stands in for it here is a
    /// map, because the only thing above the boundary can observe is that the
    /// state survived being freed and came back. Sizes are derived from the
    /// progress the sequence had made, so a longer conversation persists to a
    /// larger copy — the proportion a caller reasons about.
    persisted: Mutex<HashMap<String, u64>>,
    /// The opaque plan and generation options the adapter actually received.
    /// These are observability evidence, never inputs to P4 scheduling.
    loaded: Mutex<Option<String>>,
    plans: Mutex<Vec<String>>,
    options: Mutex<Vec<String>>,
}

impl Mock {
    /// A stage that is not the end of its chain. It advances its layer range
    /// and produces no token, because logits exist only at the end.
    pub fn staged(position: usize, profile: Profile) -> Self {
        Self::new(Distribution::Staged, position, false, profile)
    }

    /// The last stage of a chain. This is where generation lands, so this is
    /// the only stage that counts tokens and decides a sequence is finished.
    pub fn terminal(position: usize, profile: Profile) -> Self {
        Self::new(Distribution::Staged, position, true, profile)
    }

    /// A backend that spreads a model itself, the way vLLM and SGLang do. Its
    /// chain is one node long, so it is both the leading and the last stage.
    pub fn internal(profile: Profile) -> Self {
        Self::new(Distribution::Internal, 0, true, profile)
    }

    fn new(distribution: Distribution, position: usize, terminal: bool, profile: Profile) -> Self {
        Self {
            profile,
            distribution,
            position,
            terminal,
            generation: AtomicU64::new(0),
            widths: Mutex::new(Vec::new()),
            busy: AtomicU64::new(0),
            idle: AtomicU64::new(0),
            rested: Mutex::new(None),
            running: AtomicUsize::new(0),
            peak_running: AtomicUsize::new(0),
            produced: Mutex::new(HashMap::new()),
            persisted: Mutex::new(HashMap::new()),
            loaded: Mutex::new(None),
            plans: Mutex::new(Vec::new()),
            options: Mutex::new(Vec::new()),
        }
    }

    /// How long this adapter spent inside hops.
    ///
    /// The numerator of the question a chain has to answer: stage compute over
    /// wall clock. One stage can never exceed the wall; a chain of three that
    /// overlaps properly approaches three times it, and a chain that takes
    /// turns stays at one however much work is queued behind it.
    pub fn busy(&self) -> std::time::Duration {
        std::time::Duration::from_nanos(self.busy.load(Ordering::Relaxed))
    }

    /// How long this adapter had nothing to do between hops.
    ///
    /// Beside `busy`, this is the utilisation of one stage: a chain that never
    /// rests has an idle near zero however long its queue is.
    pub fn idle(&self) -> std::time::Duration {
        std::time::Duration::from_nanos(self.idle.load(Ordering::Relaxed))
    }

    /// Widths of every hop this adapter ran.
    pub fn widths(&self) -> Vec<usize> {
        self.widths.lock().expect("width log lock").clone()
    }

    /// The most hops this adapter ever had in flight at once. Anything above
    /// one means a node started work beside work.
    pub fn peak_concurrent_hops(&self) -> usize {
        self.peak_running.load(Ordering::SeqCst)
    }

    fn load(&self, load: p4_adapter::Load, events: &dyn EventSink) {
        let deployment = load.deployment.clone();
        if load.artifact.is_empty() || load.plan.trim().is_empty() {
            events.raise(Event::Failed {
                deployment,
                sequence: None,
                detail: "mock adapter requires artifact and opaque load plan".into(),
            });
            return;
        }
        *self.loaded.lock().expect("loaded lock") = Some(load.artifact);
        self.plans.lock().expect("plan log lock").push(load.plan);
        let stages = self.profile.stages.max(1);
        let step = self.profile.stage_cost();
        for stage in 0..stages {
            spin(step);
            events.raise(Event::LoadProgress {
                deployment: deployment.clone(),
                stage,
                percent: (stage + 1) * 100 / stages,
                detail: "mock stage advancing".into(),
            });
        }
        if self.profile.fault == Fault::Load {
            events.raise(Event::Failed {
                deployment,
                sequence: None,
                detail: "mock deployment was asked to fail its load".into(),
            });
            return;
        }
        events.raise(Event::Loaded {
            generation: self.generation.fetch_add(1, Ordering::SeqCst) + 1,
            allocations: (0..stages)
                .map(|stage| Allocation {
                    category: format!("stage{stage}.declared_reservation"),
                    bytes: self.profile.reserved_per_stage,
                })
                .collect(),
            deployment,
        });
    }

    fn hop(&self, hop: Hop, events: &dyn EventSink) {
        let began = std::time::Instant::now();
        let first_sequence = hop.sequences.first().map(|s| s.sequence.clone());
        if let Some(ended) = self.rested.lock().expect("rest lock").take() {
            self.idle.fetch_add(
                began.duration_since(ended).as_nanos() as u64,
                Ordering::Relaxed,
            );
        }
        self.widths
            .lock()
            .expect("width log lock")
            .push(hop.width());
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak_running.fetch_max(now, Ordering::SeqCst);

        let mut valid = Vec::with_capacity(hop.sequences.len());
        for sequence in hop.sequences {
            self.options
                .lock()
                .expect("options log lock")
                .push(sequence.options.clone());
            if !is_json_object(&sequence.options) {
                events.raise(Event::Failed {
                    deployment: hop.deployment.clone(),
                    sequence: Some(sequence.sequence),
                    detail: "mock adapter rejected non-object generation options".into(),
                });
            } else {
                valid.push(sequence);
            }
        }
        if valid.is_empty() {
            self.running.fetch_sub(1, Ordering::SeqCst);
            return;
        }

        if self.profile.fault == Fault::Silence {
            // Never answers. The node stays busy until its deadline or a
            // cancellation ends the work, which is the point of this fault.
            return;
        }
        spin(
            self.profile
                .hop_cost(self.position, hop.phase == Phase::Prefill),
        );
        self.running.fetch_sub(1, Ordering::SeqCst);
        // Recorded where the time was actually spent, not around the whole
        // call: what a chain is asked afterwards is how much of the wall clock
        // its stages were computing, and bookkeeping is not computing.
        self.busy
            .fetch_add(began.elapsed().as_nanos() as u64, Ordering::Relaxed);
        *self.rested.lock().expect("rest lock") = Some(std::time::Instant::now());

        if self.profile.fault == Fault::Hop {
            events.raise(Event::Failed {
                deployment: hop.deployment,
                sequence: first_sequence,
                detail: "mock deployment was asked to fail its hops".into(),
            });
            return;
        }
        events.raise(Event::HopComplete {
            outcomes: valid
                .iter()
                .map(|sequence| self.outcome(sequence))
                .collect(),
            deployment: hop.deployment,
        });
    }

    /// Counts a token for this sequence and decides whether it is finished.
    ///
    /// The count is kept here rather than read from the request each lap,
    /// because a request does not carry its own progress back down — a backend
    /// holding a sequence open is what knows how far it has got.
    fn outcome(&self, sequence: &p4_adapter::Sequence) -> Outcome {
        // Every stage holds this sequence's attention state for its own layer
        // range — that is what pipeline parallelism is — so every stage counts
        // it as resident. Only the last one counts tokens.
        self.produced
            .lock()
            .expect("sequence progress lock")
            .entry(sequence.sequence.clone())
            .and_modify(|value| value.lifetime += 1)
            .or_insert(Progress {
                turn: 0,
                lifetime: 1,
            });

        if !self.terminal {
            // A middle stage advanced its share and has nothing to say about
            // the token. Counting here would make an n-stage chain produce n
            // tokens per lap.
            return Outcome {
                sequence: sequence.sequence.clone(),
                text: String::new(),
                position: sequence.position,
                stop: None,
            };
        }
        let mut produced = self.produced.lock().expect("sequence progress lock");
        // The lifetime was already counted above, for every stage. Here only
        // the turn advances, because only the last stage produces tokens.
        let progress = produced
            .entry(sequence.sequence.clone())
            .and_modify(|value| value.turn += 1)
            .or_insert(Progress {
                turn: 1,
                lifetime: 1,
            });
        let requested = sequence.remaining;
        let finished = requested == 0 || progress.turn > requested;
        let position = if finished {
            progress.turn.saturating_sub(1)
        } else {
            progress.turn
        };
        if finished {
            // The turn is over; the sequence is not. A backend keeps a
            // conversation's state against its id until something tells it to
            // let go — removing it here would make the state unpersistable the
            // moment it became worth persisting.
            progress.turn = 0;
        }
        Outcome {
            sequence: sequence.sequence.clone(),
            text: if finished {
                String::new()
            } else {
                format!("{}#{position} ", sequence.sequence)
            },
            position,
            stop: finished.then(|| "stop".to_string()),
        }
    }
}

impl Mock {
    /// What this adapter has written down, for a test that wants to check the
    /// state really left memory rather than being copied beside it.
    pub fn persisted(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .persisted
            .lock()
            .expect("persisted")
            .keys()
            .cloned()
            .collect();
        ids.sort();
        ids
    }

    /// Sequences currently resident.
    pub fn resident(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .produced
            .lock()
            .expect("produced")
            .keys()
            .cloned()
            .collect();
        ids.sort();
        ids
    }

    pub fn plans(&self) -> Vec<String> {
        self.plans.lock().expect("plan log lock").clone()
    }

    pub fn options_seen(&self) -> Vec<String> {
        self.options.lock().expect("options log lock").clone()
    }
}

fn is_json_object(value: &str) -> bool {
    let value = value.trim();
    value.starts_with('{') && value.ends_with('}')
}

/// Burns the declared time without a runtime.
///
/// The adapter interface is a procedure and must not require an async context
/// to honour a duration; a fleet run drives this from wherever the node runs.
fn spin(duration: std::time::Duration) {
    if duration.is_zero() {
        return;
    }
    std::thread::sleep(duration);
}

#[cfg(test)]
mod tests;

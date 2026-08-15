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

pub mod profile;

use p4_adapter::{Adapter, Allocation, Distribution, Event, EventSink, Hop, Outcome, Phase, Work};
use profile::{Fault, Profile};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

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
    /// Hops in flight. Must never exceed one for a deployment: a node starts
    /// the next only when it sees the previous end.
    running: AtomicUsize,
    peak_running: AtomicUsize,
    /// Tokens produced so far, per sequence.
    ///
    /// A backend remembers this; the request does not carry it back down. The
    /// KV a sequence occupies lives here too, which is the same reason: state
    /// belongs to whoever is holding the sequence open.
    produced: Mutex<HashMap<String, u32>>,
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
            running: AtomicUsize::new(0),
            peak_running: AtomicUsize::new(0),
            produced: Mutex::new(HashMap::new()),
        }
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

    fn load(&self, deployment: String, events: &dyn EventSink) {
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
        self.widths
            .lock()
            .expect("width log lock")
            .push(hop.width());
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak_running.fetch_max(now, Ordering::SeqCst);

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

        if self.profile.fault == Fault::Hop {
            events.raise(Event::Failed {
                deployment: hop.deployment,
                sequence: hop.sequences.first().map(|s| s.sequence.clone()),
                detail: "mock deployment was asked to fail its hops".into(),
            });
            return;
        }
        events.raise(Event::HopComplete {
            outcomes: hop
                .sequences
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
        let count = produced
            .entry(sequence.sequence.clone())
            .and_modify(|value| *value += 1)
            .or_insert(1);
        let finished = *count >= sequence.remaining.max(1);
        let position = *count;
        if finished {
            produced.remove(&sequence.sequence);
        }
        Outcome {
            sequence: sequence.sequence.clone(),
            text: format!("{}#{position} ", sequence.sequence),
            position,
            stop: finished.then(|| "stop".to_string()),
        }
    }
}

impl Adapter for Mock {
    fn distribution(&self) -> Distribution {
        self.distribution
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        match work {
            Work::Load(load) => self.load(load.deployment, events),
            Work::Unload(unload) => events.raise(Event::Unloaded {
                deployment: unload.deployment,
            }),
            Work::Hop(hop) => self.hop(hop, events),
        }
    }
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

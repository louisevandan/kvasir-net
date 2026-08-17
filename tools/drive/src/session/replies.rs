//! What came back, per route.
//!
//! Separate from the steps that ask, because it changes for a different reason:
//! this file follows the reply vocabulary, while its parent follows the shape
//! of a run. It is also the only thing here touched by another thread — every
//! reply lands on the agent's own task — so keeping the shared state in one
//! file is what makes "who holds which lock" a question with a short answer.

use super::watch::Peaks;
use p4_agent_core::agent::{Agent, Duties};
use p4_protocol::frame::Frame;
use p4_service::message::Reply;
use p4_service::message::wire::decode_reply;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};

/// What a route produced.
#[derive(Default, Clone, Debug)]
pub struct Stream {
    pub tokens: Vec<u32>,
    /// What the tokens actually said, kept so a run can show an answer rather
    /// than only count one. Four passing verdicts are equally consistent with
    /// every token being empty, which is a failure a real backend has already
    /// produced twice here.
    pub text: String,
    pub done: Option<u32>,
    pub failed: Option<String>,
    pub progress: usize,
    pub bound: bool,
    pub released: bool,
    pub accepted: bool,
}

impl Stream {
    pub fn is_finished(&self) -> bool {
        self.done.is_some() || self.failed.is_some()
    }

    /// Whether the tokens arrived in the order they were produced. The one
    /// thing a caller cannot check any other way.
    pub fn is_ordered(&self) -> bool {
        self.tokens.windows(2).all(|pair| pair[0] < pair[1])
    }
}

#[derive(Default, Clone)]
pub struct Replies {
    pub(super) streams: Arc<Mutex<HashMap<String, Stream>>>,
    /// Counted as replies land, so waiting on progress never has to walk the
    /// streams. Polling by cloning them held the same lock the recording path
    /// needs, and got slower as the tokens it was counting accumulated — the
    /// measurement starving the thing it measured.
    pub(super) finished: Arc<AtomicUsize>,
    pub(super) bound: Arc<AtomicUsize>,
    pub(super) accepted: Arc<AtomicUsize>,
    /// Discovery replies are separate from inference streams so a preflight
    /// can verify every selected agent before creating nodes.
    pub(super) models: Arc<Mutex<HashMap<String, Reply>>>,
    /// Every reply that is progress. What waiting is bounded by: a driver
    /// cannot know how fast a backend is, but it can tell a deployment that is
    /// slow from one that has stopped, and only the second is worth giving up
    /// on. Answers to the driver's own questions are excluded — counting them
    /// would let a dead deployment look busy because we were still asking it
    /// how it was doing.
    pub(super) events: Arc<AtomicUsize>,
    /// The deepest the queues got while the run was in flight.
    pub(super) peaks: Peaks,
}

impl Duties for Replies {
    fn handle(&self, frame: Frame, _: &Arc<Agent>) {
        let Ok(reply) = decode_reply(&frame.body) else {
            return;
        };
        // Facts about a machine and what an agent is doing: each is asked for
        // deliberately, and neither belongs to a route. They are handled here
        // rather than filed under one, because filing them would invent a
        // route that produced nothing and count it among the run's.
        match &reply {
            Reply::Status { snapshot } => return self.peaks.observe(snapshot),
            Reply::Machine { .. } => return,
            Reply::Model { .. } => {
                self.models
                    .lock()
                    .expect("model lock")
                    .insert(frame.envelope.route.clone(), reply);
                return;
            }
            _ => {}
        }
        self.events.fetch_add(1, SeqCst);
        let mut streams = self.streams.lock().expect("reply lock");
        let stream = streams.entry(frame.envelope.route.clone()).or_default();
        match reply {
            Reply::Token { index, text } => {
                stream.tokens.push(index);
                stream.text.push_str(&text);
            }
            Reply::Done { generated, .. } => {
                stream.done = Some(generated);
                self.finished.fetch_add(1, SeqCst);
            }
            Reply::Failed { detail } => {
                stream.failed = Some(detail);
                self.finished.fetch_add(1, SeqCst);
            }
            Reply::Progress { .. } => stream.progress += 1,
            Reply::Bound { .. } => {
                stream.bound = true;
                self.bound.fetch_add(1, SeqCst);
            }
            Reply::Released => stream.released = true,
            Reply::Accepted { .. } => {
                stream.accepted = true;
                self.accepted.fetch_add(1, SeqCst);
            }
            // What became of a cached sequence is asked for deliberately and
            // read where it was asked for, not here.
            Reply::Cached { .. } => {}
            // Returned above. Left as a quiet arm rather than a panic: this
            // runs on the agent's own task, where an unexpected reply must not
            // be able to take the driver down.
            Reply::Machine { .. } | Reply::Model { .. } | Reply::Status { .. } => {}
        }
    }
}

/// What a run is allowed to say afterwards.
#[derive(Default, Debug)]
pub struct Outcome {
    /// How many tokens each unfinished route managed before it stopped. The
    /// shape of this says where a stall is: all zero means work never
    /// started, all near the target means a terminal was lost.
    pub stalled: Vec<usize>,
    pub completed: usize,
    pub failed: usize,
    /// What the first failure said. A count of failures without one of their
    /// reasons is the shape of report that sends an operator to the logs of
    /// every machine in the chain, when the answer was already in hand.
    pub why: Option<String>,
    pub unanswered: usize,
    pub out_of_order: usize,
    pub tokens: usize,
    pub routes: usize,
    /// One answer, in full. Evidence rather than a verdict: a mock's is
    /// simulated and says nothing, and a real backend's is the only thing that
    /// distinguishes tokens from empty strings that were counted.
    pub sample: String,
    /// The driver stopped waiting because nothing was arriving any more. Said
    /// separately from the verdicts, because "we stopped watching" and "the
    /// deployment stopped working" are different claims, and only the second
    /// is about the thing under test.
    pub quiet: bool,
    /// The deepest a node's own queue got, the most it ever had inside the
    /// adapter at once, the deepest any main-queue lane got, and how many
    /// times these were asked for. All four are needed together: a ceiling
    /// held is only meaningful beside a backlog that existed, and both are
    /// only meaningful if anybody looked.
    pub node_depth: usize,
    pub running: usize,
    pub lane: usize,
    pub samples: usize,
}

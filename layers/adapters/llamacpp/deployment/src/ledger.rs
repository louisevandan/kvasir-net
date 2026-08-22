//! Per-submission bookkeeping on this client's side of the connection:
//! dedup, generation fencing, `Produced` ordinal continuity, and what a
//! reconnect needs to replay.
//!
//! This is deliberately not the "coordinator's `SubmissionId` ledger with
//! bounded terminal tombstone" `SEALED-CONTRACT.md` §5 asks for -- that one
//! lives in `apps/llama`'s coordinator (the "llama 경로" allowlist row) and
//! is authoritative. This ledger exists only so the client itself never
//! sends a submission twice, never hands a caller a stale-generation or
//! out-of-order event, and knows what to resend after a reconnect. Nothing
//! Terminated submissions are kept as tombstones, capped and evicted
//! oldest-first ([`TOMBSTONE_CAP`]). They have to be kept at all because a
//! resend of an id this client already settled must not start a second
//! execution, and they have to be capped because a run that never stops
//! would otherwise grow this map for as long as it lasts -- forty sessions
//! an hour is a leak with a slow fuse, not a bounded working set.

use crate::contract::{Event, Generation, RejectReason, SubmissionId, Submit};
use std::collections::{HashMap, VecDeque};

/// How many settled submissions stay remembered.
///
/// Only large enough that a resend of something recent is still recognised;
/// beyond it the backend's own dedup is authoritative anyway
/// (`SEALED-CONTRACT.md` §5), so forgetting the oldest costs nothing this
/// client is responsible for.
pub const TOMBSTONE_CAP: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmissionState {
    /// `Submit` sent, no `Accepted`/`Rejected` seen yet.
    Pending,
    /// `Accepted` seen; `Produced*` may follow.
    Accepted,
    /// Terminal: `Rejected` or `Settled` seen. No further event is admitted.
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    generation: Generation,
    submit: Submit,
    state: SubmissionState,
    next_ordinal: u64,
    /// A cancel this client accepted but may not have got onto the wire.
    ///
    /// A cancel that fails its socket write is not a cancel the caller can
    /// retry -- it has already returned. Holding the intent here until the
    /// submission terminates is what lets a reconnect resend it; without
    /// this the request keeps running against a caller who asked it to stop.
    cancel_requested: bool,
}

/// What a caller should do with an event it fed the ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Genuinely new information; the caller should forward it.
    Apply,
    /// The submission belongs to a generation this ledger has since moved
    /// past. Refused per `SEALED-CONTRACT.md` §1: "이전 deployment_generation의
    /// 결과는 무조건 stale이다."
    StaleGeneration,
    /// Never submitted through this ledger, or already reaped.
    Unknown,
    /// A second terminal event for a submission that already had one.
    AlreadySettled,
    /// A `Produced` whose ordinal does not continue the sequence this ledger
    /// has already accepted.
    OutOfOrder { expected: u64, got: u64 },
}

/// Whether `begin` actually started a new submission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    New,
    AlreadyKnown,
}

pub struct Ledger {
    generation: Generation,
    entries: HashMap<SubmissionId, Entry>,
    /// Terminated ids in the order they terminated, so the oldest is the
    /// one evicted when the cap is reached.
    tombstones: VecDeque<SubmissionId>,
}

impl Ledger {
    pub fn new(generation: Generation) -> Self {
        Self {
            generation,
            entries: HashMap::new(),
            tombstones: VecDeque::new(),
        }
    }

    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Moves the ledger to a new deployment generation. Existing entries are
    /// left in place (a stale-generation event about them must still resolve
    /// to `StaleGeneration`, not `Unknown`, so a caller can tell "this was
    /// real and is now moot" from "this was never real") but are never
    /// replayed on reconnect once they are no longer current -- see
    /// `in_flight_for_replay`.
    pub fn advance_generation(&mut self, generation: Generation) {
        self.generation = generation;
    }

    /// Registers a new submission, or reports that this `submission_id` is
    /// already known so the caller does not send it twice. This is the
    /// client-side half of "같은 submission_id 재전송은 새 실행을 만들지
    /// 않는다": the server's dedup is authoritative, but a client that never
    /// resends in the first place needs no server round trip to prove it.
    pub fn begin(&mut self, submit: Submit) -> Admission {
        if self.entries.contains_key(&submit.submission_id) {
            return Admission::AlreadyKnown;
        }
        self.entries.insert(
            submit.submission_id.clone(),
            Entry {
                generation: submit.deployment_generation,
                submit,
                state: SubmissionState::Pending,
                next_ordinal: 0,
                cancel_requested: false,
            },
        );
        Admission::New
    }

    pub fn state_of(&self, submission_id: &str) -> Option<SubmissionState> {
        self.entries.get(submission_id).map(|entry| entry.state)
    }

    /// Validates one event against everything this ledger knows and, if it
    /// is genuine, advances the submission's recorded state. Returns the
    /// verdict either way; the caller (the client's event-dispatch loop)
    /// decides what to do with a refusal, but it must never forward one.
    pub fn apply(&mut self, event: &Event) -> Verdict {
        let submission_id = event.submission_id().to_owned();
        let Some(entry) = self.entries.get_mut(&submission_id) else {
            return Verdict::Unknown;
        };
        if entry.generation != self.generation {
            return Verdict::StaleGeneration;
        }
        if entry.state == SubmissionState::Done {
            return Verdict::AlreadySettled;
        }
        match event {
            Event::Accepted(_) => {
                entry.state = SubmissionState::Accepted;
                Verdict::Apply
            }
            // `Full` is the one rejection that is not terminal. The backend
            // refuses it before recording anything (`coordinator.ts` never
            // calls `ledger.begin` ahead of that check), so the submission
            // never started and this id is free again. Keeping the entry
            // would make the caller's retry `AlreadyKnown` -- nothing would
            // reach the wire, no further event would ever arrive, and the
            // request would hang forever on a rejection that meant "later".
            Event::Rejected(rejected) if rejected.reason == RejectReason::Full => {
                self.entries.remove(&submission_id);
                Verdict::Apply
            }
            Event::Rejected(_) => {
                entry.state = SubmissionState::Done;
                self.entomb(submission_id);
                Verdict::Apply
            }
            Event::Produced(produced) => {
                if produced.event_ordinal != entry.next_ordinal {
                    return Verdict::OutOfOrder {
                        expected: entry.next_ordinal,
                        got: produced.event_ordinal,
                    };
                }
                entry.next_ordinal += 1;
                Verdict::Apply
            }
            Event::Settled(_) => {
                entry.state = SubmissionState::Done;
                self.entomb(submission_id);
                Verdict::Apply
            }
        }
    }

    /// How many submissions this ledger is holding, live and tombstoned
    /// together -- the number that must not grow without limit.
    pub fn tracked(&self) -> usize {
        self.entries.len()
    }

    /// Records a submission as terminated and evicts the oldest tombstone
    /// once the cap is reached, dropping its entry with it.
    ///
    /// An evicted id is simply forgotten: a later event about it resolves to
    /// `Unknown` rather than `AlreadySettled`, and a resend of it is admitted
    /// as new. Both are the backend's to settle by then, and both are
    /// preferable to a map that only ever grows.
    fn entomb(&mut self, submission_id: SubmissionId) {
        self.tombstones.push_back(submission_id);
        while self.tombstones.len() > TOMBSTONE_CAP {
            if let Some(oldest) = self.tombstones.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }

    /// Every submission of the *current* generation this ledger has sent but
    /// has not yet seen a terminal event for. A reconnect resends exactly
    /// these, in a stable order, and nothing from a superseded generation --
    /// resending those would only manufacture a `StaleGeneration` refusal on
    /// whatever came back.
    /// Records that this submission should be cancelled, so a reconnect can
    /// resend the cancel that never made it onto the wire.
    pub fn request_cancel(&mut self, submission_id: &str) {
        if let Some(entry) = self.entries.get_mut(submission_id) {
            entry.cancel_requested = true;
        }
    }

    /// Submissions whose cancel is still owed to the backend.
    pub fn cancels_for_replay(&self) -> Vec<SubmissionId> {
        let mut ids: Vec<SubmissionId> = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry.cancel_requested
                    && entry.generation == self.generation
                    && entry.state != SubmissionState::Done
            })
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    }

    pub fn in_flight_for_replay(&self) -> Vec<Submit> {
        let mut submissions: Vec<&Entry> = self
            .entries
            .values()
            .filter(|entry| {
                entry.generation == self.generation && entry.state != SubmissionState::Done
            })
            .collect();
        submissions.sort_by(|a, b| a.submit.submission_id.cmp(&b.submit.submission_id));
        submissions
            .into_iter()
            .map(|entry| entry.submit.clone())
            .collect()
    }

    #[cfg(test)]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests;

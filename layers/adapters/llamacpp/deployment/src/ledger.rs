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
//! outside this process treats it as authoritative.
//!
//! Terminated submissions are kept as tombstones, capped and evicted
//! oldest-first ([`TOMBSTONE_CAP`]). They have to be kept at all because a
//! resend of an id this client already settled must not start a second
//! execution, and they have to be capped because a run that never stops
//! would otherwise grow this map for as long as it lasts -- forty sessions
//! an hour is a leak with a slow fuse, not a bounded working set.

use crate::contract::{
    Event, Generation, RejectReason, Rejected, SettleReason, Settled, SubmissionId, Submit,
};
use std::collections::{HashMap, VecDeque};

mod replay;

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
    generated_tokens: u32,
    /// Exact terminal result replayed if P4 resubmits a recently completed id.
    terminal: Option<Event>,
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
    /// A terminal event was applied. The pump must release one outstanding
    /// submission permit after forwarding it.
    Terminal,
    /// The backend reported ordinary capacity pressure. The llama client,
    /// not P4, owns the delayed resend and does not forward this yet.
    RetryFull,
    /// A replay repeated information already admitted on the previous
    /// connection. It is valid on the wire but must not be raised twice.
    Duplicate,
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
    /// The event kind is impossible in the submission's current state.
    ProtocolViolation,
}

/// Whether `begin` actually started a new submission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admission {
    New,
    AlreadyKnown,
    /// The id is a retained tombstone; raise its exact terminal result rather
    /// than accepting a second execution or leaving the new caller waiting.
    Replay(Event),
    StaleGeneration,
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
    pub fn advance_generation(&mut self, generation: Generation) -> Vec<Event> {
        if generation <= self.generation {
            return Vec::new();
        }
        self.generation = generation;
        // Entries of a superseded generation become tombstones rather than
        // staying live. They still have to be *findable* -- an event about
        // one must resolve to `StaleGeneration`, not `Unknown`, so a caller
        // can tell "this was real and is now moot" from "this was never
        // real" -- but as live entries they fell outside the cap entirely,
        // and a deployment reloaded often enough grew this map for as long
        // as the process lived.
        let superseded: Vec<SubmissionId> = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry.generation != generation && entry.state != SubmissionState::Done
            })
            .map(|(submission_id, _)| submission_id.clone())
            .collect();
        let mut terminals = Vec::with_capacity(superseded.len());
        for submission_id in superseded {
            if let Some(entry) = self.entries.get_mut(&submission_id) {
                let terminal = match entry.state {
                    SubmissionState::Pending => Event::Rejected(Rejected {
                        submission_id: submission_id.clone(),
                        reason: RejectReason::DeploymentClosed,
                    }),
                    SubmissionState::Accepted => Event::Settled(Settled {
                        submission_id: submission_id.clone(),
                        reason: SettleReason::Error,
                        generated_tokens: entry.generated_tokens,
                    }),
                    SubmissionState::Done => unreachable!("filtered above"),
                };
                entry.state = SubmissionState::Done;
                entry.terminal = Some(terminal.clone());
                terminals.push(terminal);
            }
            self.entomb(submission_id);
        }
        terminals
    }

    /// Registers a new submission, or reports that this `submission_id` is
    /// already known so the caller does not send it twice. This is the
    /// client-side half of "같은 submission_id 재전송은 새 실행을 만들지
    /// 않는다": the server's dedup is authoritative, but a client that never
    /// resends in the first place needs no server round trip to prove it.
    pub fn begin(&mut self, submit: Submit) -> Admission {
        if let Some(entry) = self.entries.get(&submit.submission_id) {
            return if entry.state == SubmissionState::Done {
                Admission::Replay(
                    entry
                        .terminal
                        .clone()
                        .expect("every tombstone retains its terminal event"),
                )
            } else {
                Admission::AlreadyKnown
            };
        }
        if submit.deployment_generation != self.generation {
            return Admission::StaleGeneration;
        }
        self.entries.insert(
            submit.submission_id.clone(),
            Entry {
                generation: submit.deployment_generation,
                submit,
                state: SubmissionState::Pending,
                next_ordinal: 0,
                generated_tokens: 0,
                terminal: None,
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
            Event::Accepted(_) if entry.state == SubmissionState::Accepted => Verdict::Duplicate,
            Event::Accepted(_) if entry.state == SubmissionState::Pending => {
                entry.state = SubmissionState::Accepted;
                Verdict::Apply
            }
            Event::Accepted(_) => Verdict::ProtocolViolation,
            // `Full` is the one rejection that is not terminal. The backend
            // refuses it before recording anything (`coordinator.ts` never
            // calls `ledger.begin` ahead of that check), so the submission
            // never started and this id is free again. Keeping the entry
            // would make the caller's retry `AlreadyKnown` -- nothing would
            // reach the wire, no further event would ever arrive, and the
            // request would hang forever on a rejection that meant "later".
            Event::Rejected(rejected)
                if rejected.reason == RejectReason::Full
                    && entry.state == SubmissionState::Pending =>
            {
                Verdict::RetryFull
            }
            Event::Rejected(_) if entry.state == SubmissionState::Pending => {
                entry.state = SubmissionState::Done;
                entry.terminal = Some(event.clone());
                self.entomb(submission_id);
                Verdict::Terminal
            }
            Event::Rejected(_) => Verdict::ProtocolViolation,
            Event::Produced(produced) if entry.state == SubmissionState::Accepted => {
                // The server's reconnect journal replays from its retained
                // head, so an already-delivered prefix is expected after a
                // socket replacement. Suppress that prefix, but keep a gap
                // fatal: only ordinals below the next expected one are safe.
                if produced.event_ordinal < entry.next_ordinal {
                    return Verdict::Duplicate;
                }
                if produced.event_ordinal > entry.next_ordinal {
                    return Verdict::OutOfOrder {
                        expected: entry.next_ordinal,
                        got: produced.event_ordinal,
                    };
                }
                entry.next_ordinal += 1;
                entry.generated_tokens = produced.generated_tokens;
                Verdict::Apply
            }
            Event::Produced(_) => Verdict::ProtocolViolation,
            Event::Settled(_) if entry.state == SubmissionState::Accepted => {
                entry.state = SubmissionState::Done;
                entry.terminal = Some(event.clone());
                self.entomb(submission_id);
                Verdict::Terminal
            }
            Event::Settled(_) => Verdict::ProtocolViolation,
        }
    }

    /// The original command retained for an adapter-local Full retry.
    pub fn submission_for_retry(&self, submission_id: &str) -> Option<Submit> {
        self.entries.get(submission_id).and_then(|entry| {
            (entry.generation == self.generation && entry.state == SubmissionState::Pending)
                .then(|| entry.submit.clone())
        })
    }

    /// Turns an expired Full retry into the rejection P4 may relay as a
    /// terminal refusal. Returns false if the submission already moved on.
    pub fn finish_full(&mut self, submission_id: &str) -> bool {
        let Some(entry) = self.entries.get_mut(submission_id) else {
            return false;
        };
        if entry.state != SubmissionState::Pending {
            return false;
        }
        entry.state = SubmissionState::Done;
        entry.terminal = Some(Event::Rejected(Rejected {
            submission_id: submission_id.to_string(),
            reason: RejectReason::Full,
        }));
        self.entomb(submission_id.to_string());
        true
    }

    /// Converts malformed backend sequencing into one explicit terminal event.
    /// A partial accepted response settles as an error; a request that never
    /// reached Accepted is refused as invalid.
    pub fn fail_protocol(&mut self, submission_id: &str) -> Option<Event> {
        let entry = self.entries.get_mut(submission_id)?;
        if entry.state == SubmissionState::Done || entry.generation != self.generation {
            return None;
        }
        let terminal = match entry.state {
            SubmissionState::Pending => Event::Rejected(Rejected {
                submission_id: submission_id.to_owned(),
                reason: RejectReason::Invalid,
            }),
            SubmissionState::Accepted => Event::Settled(Settled {
                submission_id: submission_id.to_owned(),
                reason: SettleReason::Error,
                generated_tokens: entry.generated_tokens,
            }),
            SubmissionState::Done => unreachable!("checked above"),
        };
        entry.state = SubmissionState::Done;
        entry.terminal = Some(terminal.clone());
        self.entomb(submission_id.to_owned());
        Some(terminal)
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

    /// Records that this submission should be cancelled, so a reconnect can
    /// resend the cancel that never made it onto the wire.
    pub fn request_cancel(&mut self, submission_id: &str) {
        if let Some(entry) = self.entries.get_mut(submission_id) {
            entry.cancel_requested = true;
        }
    }

    #[cfg(test)]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests;

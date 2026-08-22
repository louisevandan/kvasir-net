//! The single-owner pump: one thread that owns the writer and the ledger
//! together, so reconnect, replay, cancel, inbound-event dispatch and
//! shutdown are all steps of the *same* serial loop rather than operations
//! racing each other across threads.
//!
//! Everything that needs to agree with everything else -- "is this
//! submission already known," "which writer is current," "what does a
//! reconnect need to resend" -- is decided by whichever [`PumpEvent`] this
//! loop is holding at the moment, and nothing else touches the writer or the
//! ledger while it holds it. `try_submit`/`cancel` (`super::DeploymentClient`)
//! never reach past [`PumpHandle`]; the reader thread this module also spawns
//! never reaches past its bounded inbound permit. Both funnel into the one
//! channel this loop reads.

mod control;
mod full_retry;
mod handle;
mod reader;

use super::permits::Permits;
use crate::contract::{Command, Event, Generation, RejectReason, Rejected, SubmissionId, Submit};
use crate::ledger::{Admission, Ledger, Verdict};
use crate::transport::{TransportFactory, TransportWriter};
use control::ControlMailbox;
use full_retry::{FullRetries, deadline_expired};
pub(crate) use handle::PumpHandle;
use p4_adapter::deployment::Sink;
use reader::spawn_reader;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// How many outstanding commands/events the pump will hold before a caller's
/// `try_submit`/`cancel` is refused rather than queued. Sized generously
/// above any realistic number of submissions one deployment carries
/// in flight at once (`SEALED-CONTRACT.md` never sizes this; a P4 agent
/// process is one deployment's worth of traffic, not a multi-tenant
/// server) -- reached only under genuine overload, at which point refusing
/// promptly is exactly what keeps the caller's thread from blocking on a
/// socket that is not draining.
pub(crate) const COMMAND_QUEUE_BOUND: usize = 256;

/// Inbound events the reader may hand over before it has to wait.
///
/// Deep enough that an ordinary burst of tokens never touches it, and
/// finite so a consumer slower than the socket ends up slowing the socket
/// rather than filling memory. See `super::permits` for why this is a
/// permit count rather than a channel bound.
pub(crate) const INBOUND_BOUND: usize = 1024;

// The std channel itself has no capacity primitive, so each producer has a
// separate structural bound: live submissions are counted above, inbound
// events consume `INBOUND_BOUND` permits, and controls are coalesced in a
// bounded mailbox behind one `ControlsReady` wake-up. This preserves cancel
// and generation intent without letting any producer grow memory without a
// bound.

pub(crate) enum PumpEvent {
    Submit { submit: Submit, inserted_live: bool },
    ControlsReady,
    Inbound { epoch: u64, event: Event },
    ConnectionLost { epoch: u64 },
    Shutdown,
}

/// The pump's authoritative writer and ledger state, touched by exactly one
/// thread: the one running [`Pump::run`]. The shared `live` set is only a
/// bounded admission index for cancel; it never decides wire or ledger state.
struct Pump {
    factory: Arc<dyn TransportFactory>,
    sink: Arc<dyn Sink>,
    writer: Option<Box<dyn TransportWriter>>,
    ledger: Ledger,
    backoff: Duration,
    closed: Arc<AtomicBool>,
    reconnects: Arc<AtomicU64>,
    full_retry_count: Arc<AtomicU64>,
    link_epoch: u64,
    outstanding_submissions: Arc<AtomicUsize>,
    /// Shared with `DeploymentClient`, so a generation learned on a
    /// reconnect is what the next caller stamps rather than the value this
    /// process started with.
    current_generation: Arc<AtomicU64>,
    sender: Sender<PumpEvent>,
    reader_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// Returned one at a time as inbound events are dealt with, which is
    /// what lets the reader block instead of buffering without limit.
    inbound: Arc<Permits>,
    /// One due time per Full submission. Kept on this single-owner thread,
    /// so adapter backpressure cannot create a task or thread per retry.
    full_retries: FullRetries,
    controls: Arc<ControlMailbox>,
    /// Submission ids accepted at the public client boundary and not yet
    /// terminal. The control mailbox consults the same set so unknown cancel
    /// floods cannot displace a real cancel from its bounded set.
    live: Arc<Mutex<HashSet<SubmissionId>>>,
}

impl Pump {
    fn run(mut self, receiver: Receiver<PumpEvent>) {
        loop {
            let event = match self.next_retry_wait() {
                Some(wait) => match receiver.recv_timeout(wait) {
                    Ok(event) => event,
                    Err(RecvTimeoutError::Timeout) => {
                        self.retry_due_full();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        self.inbound.close();
                        return;
                    }
                },
                None => match receiver.recv() {
                    Ok(event) => event,
                    Err(_) => {
                        self.inbound.close();
                        return;
                    }
                },
            };
            match event {
                PumpEvent::Shutdown => {
                    self.writer = None;
                    self.inbound.close();
                    return;
                }
                PumpEvent::Submit {
                    submit,
                    inserted_live,
                } => {
                    self.handle_submit(submit, inserted_live);
                }
                PumpEvent::ControlsReady => {
                    let batch = self.controls.drain();
                    if let Some(generation) = batch.generation {
                        self.advance_generation(generation);
                    }
                    for submission_id in batch.cancels {
                        self.handle_cancel(submission_id);
                    }
                }
                PumpEvent::Inbound { epoch, event } => {
                    // Returned for a stale event too. A permit is a slot in
                    // the hand-off, not a statement about the event's
                    // worth; keeping one back for every frame that arrived
                    // late would shrink the bound a little at a time until
                    // the reader never ran again.
                    self.inbound.release();
                    // A reader this pump has already replaced can still be
                    // holding a decoded event. Its epoch says which link it
                    // came from; anything but the current one is answering
                    // about a connection that no longer exists.
                    if epoch == self.link_epoch {
                        self.handle_inbound(event);
                    }
                }
                PumpEvent::ConnectionLost { epoch } => {
                    if epoch != self.link_epoch {
                        // A stale report from a connection this pump already
                        // replaced -- e.g. a write failure triggered a
                        // reconnect inline before this generation's own
                        // reader noticed the same thing. Nothing to do.
                        continue;
                    }
                    if self.closed.load(Ordering::SeqCst) {
                        return;
                    }
                    self.reconnect();
                }
            }
        }
    }

    fn handle_submit(&mut self, submit: Submit, inserted_live: bool) {
        let submission_id = submit.submission_id.clone();
        match self.ledger.begin(submit.clone()) {
            Admission::New if !self.try_write(Command::Submit(submit)) => self.reconnect(),
            Admission::New => {}
            Admission::AlreadyKnown => {
                self.release_outstanding();
                if inserted_live {
                    self.remove_live(&submission_id);
                }
            }
            Admission::Replay(terminal) => {
                self.release_submission(&submission_id);
                self.sink.raise(terminal);
            }
            Admission::StaleGeneration => {
                self.release_submission(&submission_id);
                self.sink.raise(Event::Rejected(Rejected {
                    submission_id,
                    reason: RejectReason::DeploymentClosed,
                }));
            }
        }
    }

    fn handle_cancel(&mut self, submission_id: SubmissionId) {
        // Recorded before the write, so a write that fails still leaves the
        // intent somewhere a reconnect can find it.
        self.ledger.request_cancel(&submission_id);
        let command = Command::Cancel(crate::contract::Cancel { submission_id });
        if !self.try_write(command) {
            self.reconnect();
        }
    }

    fn handle_inbound(&mut self, event: Event) {
        match self.ledger.apply(&event) {
            Verdict::Apply => {
                if matches!(&event, Event::Accepted(_)) {
                    self.full_retries.remove(event.submission_id());
                }
                self.sink.raise(event);
            }
            Verdict::Terminal => {
                self.full_retries.remove(event.submission_id());
                self.release_submission(event.submission_id());
                self.sink.raise(event);
            }
            Verdict::RetryFull => self.handle_full(event),
            Verdict::ProtocolViolation | Verdict::OutOfOrder { .. } => {
                self.terminalize_protocol(event.submission_id());
            }
            Verdict::StaleGeneration
            | Verdict::Unknown
            | Verdict::Duplicate
            | Verdict::AlreadySettled => {}
        }
    }

    fn handle_full(&mut self, event: Event) {
        let submission_id = event.submission_id().clone();
        let Some(submit) = self.ledger.submission_for_retry(&submission_id) else {
            self.terminalize_protocol(&submission_id);
            return;
        };
        if deadline_expired(submit.deadline_unix_ms) {
            if self.ledger.finish_full(&submission_id) {
                self.release_submission(&submission_id);
                self.sink.raise(event);
            }
            return;
        }
        self.full_retries.schedule(submission_id);
        self.full_retry_count.fetch_add(1, Ordering::Relaxed);
    }

    fn next_retry_wait(&self) -> Option<Duration> {
        self.full_retries.next_wait()
    }

    fn retry_due_full(&mut self) {
        for submission_id in self.full_retries.take_due() {
            let Some(submit) = self.ledger.submission_for_retry(&submission_id) else {
                self.terminalize_protocol(&submission_id);
                continue;
            };
            if deadline_expired(submit.deadline_unix_ms) {
                if self.ledger.finish_full(&submission_id) {
                    self.release_submission(&submission_id);
                    self.sink.raise(Event::Rejected(Rejected {
                        submission_id,
                        reason: RejectReason::Full,
                    }));
                }
            } else if !self.try_write(Command::Submit(submit)) {
                self.reconnect();
            }
        }
    }

    fn advance_generation(&mut self, generation: Generation) {
        if generation <= self.ledger.generation() {
            return;
        }
        for terminal in self.ledger.advance_generation(generation) {
            self.full_retries.remove(terminal.submission_id());
            self.release_submission(terminal.submission_id());
            self.sink.raise(terminal);
        }
        self.current_generation.store(generation, Ordering::SeqCst);
    }

    fn release_outstanding(&self) {
        self.outstanding_submissions.fetch_sub(1, Ordering::SeqCst);
    }

    fn release_submission(&self, submission_id: &str) {
        if self.remove_live(submission_id) {
            self.release_outstanding();
        }
    }

    fn remove_live(&self, submission_id: &str) -> bool {
        self.live
            .lock()
            .expect("live submissions lock")
            .remove(submission_id)
    }

    fn terminalize_protocol(&mut self, submission_id: &str) {
        self.full_retries.remove(submission_id);
        let terminal = self.ledger.fail_protocol(submission_id).unwrap_or_else(|| {
            Event::Rejected(Rejected {
                submission_id: submission_id.to_owned(),
                reason: RejectReason::Invalid,
            })
        });
        self.release_submission(submission_id);
        self.sink.raise(terminal);
    }

    /// Attempts one write over the current connection (or none, if there
    /// isn't one). Returns whether it landed; on failure the now-dead
    /// writer is dropped, but reconnecting is left to the caller -- a
    /// top-level dispatch (`handle_submit`/`handle_cancel`) reconnects
    /// immediately, while replay inside `reconnect` itself instead loops
    /// back to connect again rather than recursing.
    fn try_write(&mut self, command: Command) -> bool {
        let Some(writer) = self.writer.as_mut() else {
            return false;
        };
        if writer.send(&command).is_ok() {
            true
        } else {
            self.writer = None;
            false
        }
    }

    /// Retries until a new connection exists or the pump is closed, replays
    /// every submission the ledger still considers in flight for the
    /// current generation over it, then hands the reader thread off to a
    /// fresh one for that connection. Runs entirely inside this loop: no
    /// other `PumpEvent` is processed while this is happening, so a
    /// `Submit` that arrives mid-reconnect simply waits its turn in the
    /// channel and is handled -- by `handle_submit`, over the *new* writer
    /// -- the moment this returns. There is no window where it is recorded
    /// but has nowhere to go.
    fn reconnect(&mut self) {
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return;
            }
            match self.factory.connect() {
                Ok((writer, reader)) => {
                    self.reconnects.fetch_add(1, Ordering::SeqCst);
                    self.writer = Some(writer);
                    self.link_epoch += 1;
                    // Before the replay, not after. A deployment reloads
                    // while this client is away and the handshake is the
                    // only place that says so; adopting it first keeps the
                    // replay from resending work under a generation the
                    // backend has moved past -- which it would answer
                    // `deployment_closed` for, for ever.
                    if let Some(reported) = self.factory.reported_generation() {
                        self.advance_generation(reported);
                    }
                    let replay = self.ledger.in_flight_for_replay();
                    let mut replay_landed = true;
                    for submit in replay {
                        if !self.try_write(Command::Submit(submit)) {
                            replay_landed = false;
                            break;
                        }
                    }
                    // A cancel that never reached the old connection is
                    // still owed. Replayed after the submissions so the
                    // backend has something to cancel by the time it lands.
                    if replay_landed {
                        for submission_id in self.ledger.cancels_for_replay() {
                            if !self.try_write(Command::Cancel(crate::contract::Cancel {
                                submission_id,
                            })) {
                                replay_landed = false;
                                break;
                            }
                        }
                    }
                    if !replay_landed {
                        // This connection died before replay finished --
                        // loop back and connect again instead of handing a
                        // reader off for a writer that is already gone.
                        continue;
                    }
                    *self.reader_thread.lock().expect("reader thread lock") = Some(spawn_reader(
                        self.sender.clone(),
                        reader,
                        self.link_epoch,
                        Arc::clone(&self.inbound),
                    ));
                    return;
                }
                Err(_) => thread::sleep(self.backoff),
            }
        }
    }
}

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
//! never reach past [`PumpHandle::enqueue`]/[`PumpHandle::enqueue_lossy`];
//! the reader thread this module also spawns never reaches past
//! [`PumpHandle::deliver`]. Both funnel into the one channel this loop reads.

use super::permits::Permits;
use crate::contract::{Command, Event, Generation, SubmissionId, Submit};
use crate::ledger::{Admission, Ledger, Verdict};
use crate::transport::{TransportFactory, TransportReader, TransportWriter};
use p4_adapter::deployment::Sink;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
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

// The bound above counts *submissions* only, and the channel itself is
// unbounded, because control and data cannot share a limit.
//
// A submission refused under load is backpressure and the caller is told
// so. A cancel refused under load is a request that keeps running against
// its caller's wishes, and a dropped generation advance leaves this client
// fencing against a value the backend has already moved past -- neither is
// something a queue depth may decide. Sharing one bounded channel made the
// depth decide it, so submissions are counted here and control never is.

pub(crate) enum PumpEvent {
    Submit(Submit),
    Cancel(SubmissionId),
    Inbound { epoch: u64, event: Event },
    ConnectionLost { epoch: u64 },
    AdvanceGeneration(Generation),
    Shutdown,
}

/// What `DeploymentClient` holds to talk to the pump thread. Cloning the
/// sender is how the reader thread(s) this module spawns get their own
/// handle without sharing a lock with `try_submit`/`cancel`'s callers.
pub(crate) struct PumpHandle {
    sender: Sender<PumpEvent>,
    /// Queued submissions, so the bound applies to them alone.
    queued_submissions: Arc<AtomicUsize>,
    // Only ever joined from `#[cfg(test)]` code (`join_for_test`) -- a
    // running client has no need to wait for its own background threads.
    #[allow(dead_code)]
    pump_thread: Mutex<Option<JoinHandle<()>>>,
    #[allow(dead_code)]
    reader_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    closed: Arc<AtomicBool>,
    reconnects: Arc<AtomicU64>,
    /// Held here only so `close` can release a reader blocked on it.
    inbound: Arc<Permits>,
}

impl PumpHandle {
    /// Starts the pump thread and the first reader thread, and returns the
    /// handle a `DeploymentClient` holds for the rest of its life.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        factory: Arc<dyn TransportFactory>,
        sink: Arc<dyn Sink>,
        writer: Box<dyn TransportWriter>,
        reader: Box<dyn TransportReader>,
        generation: Generation,
        backoff: Duration,
    ) -> Self {
        let (sender, receiver) = channel();
        let queued_submissions = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicBool::new(false));
        let reconnects = Arc::new(AtomicU64::new(0));
        let reader_thread = Arc::new(Mutex::new(None));
        let inbound = Arc::new(Permits::new(INBOUND_BOUND));

        let pump = Pump {
            queued_submissions: Arc::clone(&queued_submissions),
            factory,
            sink,
            writer: Some(writer),
            ledger: Ledger::new(generation),
            backoff,
            closed: closed.clone(),
            reconnects: reconnects.clone(),
            link_epoch: 0,
            sender: sender.clone(),
            reader_thread: reader_thread.clone(),
            inbound: Arc::clone(&inbound),
        };
        let pump_thread = thread::spawn(move || pump.run(receiver));

        *reader_thread.lock().expect("reader thread lock") = Some(spawn_reader(
            sender.clone(),
            reader,
            0,
            Arc::clone(&inbound),
        ));

        Self {
            sender,
            queued_submissions,
            pump_thread: Mutex::new(Some(pump_thread)),
            reader_thread,
            closed,
            reconnects,
            inbound,
        }
    }

    /// Non-blocking: a caller on P4's calling thread never waits on this.
    /// Full is the one refusal this raises; it never touches the writer.
    pub(crate) fn enqueue(&self, event: PumpEvent) -> Result<(), ()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(());
        }
        // Conditional increment has to be one operation: a load followed by
        // a separate add lets several callers all read 255 and all pass.
        if self
            .queued_submissions
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |queued| {
                (queued < COMMAND_QUEUE_BOUND).then_some(queued + 1)
            })
            .is_err()
        {
            return Err(());
        }
        if self.sender.send(event).is_err() {
            self.queued_submissions.fetch_sub(1, Ordering::SeqCst);
            return Err(());
        }
        Ok(())
    }

    /// Best-effort: used for `cancel`, which is infallible by contract, and
    /// for `advance_generation`, which nothing here needs to refuse -- a
    /// full queue drops it rather than blocking the caller.
    /// Control commands are never refused for depth. They are not counted
    /// against `COMMAND_QUEUE_BOUND` and the channel they use is unbounded,
    /// so the only way one is lost is a pump that has already stopped.
    pub(crate) fn enqueue_control(&self, event: PumpEvent) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let _ = self.sender.send(event);
    }

    pub(crate) fn reconnect_count(&self) -> u64 {
        self.reconnects.load(Ordering::SeqCst)
    }

    /// Stops accepting new work and asks the pump to shut down. Does not
    /// forcibly interrupt a `recv()` already blocked on the current
    /// connection -- see `DeploymentClient::close`'s own doc.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        // Before the shutdown, not after: a reader waiting for a permit the
        // pump will now never return would never see the socket close.
        self.inbound.close();
        let _ = self.sender.send(PumpEvent::Shutdown);
    }

    #[cfg(test)]
    pub(crate) fn join_for_test(&self) {
        if let Some(handle) = self.pump_thread.lock().expect("pump thread lock").take() {
            handle.join().expect("pump thread panicked");
        }
        if let Some(handle) = self
            .reader_thread
            .lock()
            .expect("reader thread lock")
            .take()
        {
            handle.join().expect("reader thread panicked");
        }
    }
}

/// The pump's own state, touched by exactly one thread: the one running
/// [`Pump::run`]. No field here is behind a `Mutex` because nothing else is
/// ever allowed to reach it.
struct Pump {
    factory: Arc<dyn TransportFactory>,
    sink: Arc<dyn Sink>,
    writer: Option<Box<dyn TransportWriter>>,
    ledger: Ledger,
    backoff: Duration,
    closed: Arc<AtomicBool>,
    reconnects: Arc<AtomicU64>,
    link_epoch: u64,
    queued_submissions: Arc<AtomicUsize>,
    sender: Sender<PumpEvent>,
    reader_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// Returned one at a time as inbound events are dealt with, which is
    /// what lets the reader block instead of buffering without limit.
    inbound: Arc<Permits>,
}

impl Pump {
    fn run(mut self, receiver: Receiver<PumpEvent>) {
        loop {
            let Ok(event) = receiver.recv() else {
                // Every sender is gone, so no permit will ever come back.
                self.inbound.close();
                return;
            };
            match event {
                PumpEvent::Shutdown => {
                    self.writer = None;
                    self.inbound.close();
                    return;
                }
                PumpEvent::Submit(submit) => {
                    self.queued_submissions.fetch_sub(1, Ordering::SeqCst);
                    self.handle_submit(submit);
                }
                PumpEvent::Cancel(submission_id) => self.handle_cancel(submission_id),
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
                PumpEvent::AdvanceGeneration(generation) => {
                    self.ledger.advance_generation(generation);
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

    fn handle_submit(&mut self, submit: Submit) {
        if self.ledger.begin(submit.clone()) == Admission::New
            && !self.try_write(Command::Submit(submit))
        {
            self.reconnect();
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
        if self.ledger.apply(&event) == Verdict::Apply {
            self.sink.raise(event);
        }
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

/// Reads one connection until it ends, forwarding every event into the
/// pump's own channel, then reports the loss (tagged with the epoch this
/// reader was spawned for) and exits. Spawned fresh by the pump on every
/// connect/reconnect rather than looping across connections itself -- the
/// pump is what decides whether and how to get a new one.
fn spawn_reader(
    sender: Sender<PumpEvent>,
    mut reader: Box<dyn TransportReader>,
    epoch: u64,
    inbound: Arc<Permits>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        loop {
            match reader.recv() {
                Ok(Some(event)) => {
                    // Taken before the hand-off and returned by the pump
                    // once the event is dealt with. When the pump falls
                    // behind this blocks, `recv` above stops draining the
                    // socket, and the backend feels it -- which is the
                    // whole point of bounding this at all.
                    if !inbound.acquire() {
                        return;
                    }
                    if sender.send(PumpEvent::Inbound { epoch, event }).is_err() {
                        inbound.release();
                        return;
                    }
                }
                Ok(None) | Err(_) => {
                    let _ = sender.send(PumpEvent::ConnectionLost { epoch });
                    return;
                }
            }
        }
    })
}

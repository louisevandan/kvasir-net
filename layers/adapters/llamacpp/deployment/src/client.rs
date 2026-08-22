//! The thin client: one persistent connection to `apps/llama`'s submission
//! stream, held across every submission rather than opened per request, with
//! reconnect and generation fencing handled by a single-owner pump thread
//! ([`pump`]) built on top of [`crate::ledger::Ledger`].
//!
//! What this file does not do, on purpose: it does not compute batches, walk
//! a rank list, interpret hidden state, or know what `Hop`/`Prefill`/`Decode`
//! mean, and it does not bridge to `p4_adapter::Work`/`Hop`/`Adapter` at all
//! -- the P4 broker calls [`DeploymentClient`] directly through the
//! `p4_adapter::deployment::Client` trait it implements below. `try_submit`
//! and `cancel` never touch the writer or the ledger themselves: both just
//! hand a [`pump::PumpEvent`] to the pump thread's bounded queue and return.
//! Reconnect, replay, cancel-delivery and shutdown all happen as steps of
//! that one thread's serial loop -- see `pump`'s own doc for why that is
//! what closes the reconnect-loses-a-submission race this crate used to
//! have.

mod permits;
mod pump;

use crate::contract::{DeploymentId, EnqueueError, Generation, SubmissionId, Submit};
use crate::transport::TransportFactory;
use p4_adapter::deployment::{Client as DeploymentClientTrait, Sink};
use pump::{PumpEvent, PumpHandle};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub struct DeploymentClient {
    deployment_id: DeploymentId,
    current_generation: Arc<AtomicU64>,
    pump: PumpHandle,
}

impl DeploymentClient {
    /// Opens the connection and starts the pump thread (and its first
    /// reader thread) that own it for the client's whole lifetime. `sink` is
    /// handed over exactly once, here -- every event this client ever
    /// admits, for every submission it ever handles, is raised on this one
    /// `Arc` for as long as the client lives. A failure to connect here is a
    /// failure to establish the one connection this client will hold --
    /// there is nothing to reconnect yet, so it is returned rather than
    /// retried.
    pub fn connect(
        factory: Arc<dyn TransportFactory>,
        sink: Arc<dyn Sink>,
        deployment_id: DeploymentId,
        generation: Generation,
        backoff: Duration,
    ) -> std::io::Result<Arc<Self>> {
        let (writer, reader) = factory.connect()?;
        // What the handshake just reported wins over what the caller
        // guessed: the generation is the backend's to issue, never this
        // process's to invent.
        let generation = factory.reported_generation().unwrap_or(generation);
        let current_generation = Arc::new(AtomicU64::new(generation));
        let pump = PumpHandle::start(
            factory,
            sink,
            writer,
            reader,
            generation,
            backoff,
            Arc::clone(&current_generation),
        );
        Ok(Arc::new(Self {
            deployment_id,
            current_generation,
            pump,
        }))
    }

    pub fn deployment_id(&self) -> &str {
        &self.deployment_id
    }

    /// This client's own idea of the current generation -- what a caller
    /// composing a fresh `Submit` should stamp on it. Read synchronously
    /// off an atomic rather than round-tripping through the pump, so it is
    /// always current the instant `advance_generation` returns.
    pub fn generation(&self) -> Generation {
        self.current_generation.load(Ordering::SeqCst)
    }

    pub fn reconnect_count(&self) -> u64 {
        self.pump.reconnect_count()
    }

    /// Moves the deployment forward. Anything already in flight under the
    /// old generation stays refusable (the ledger's fencing returns
    /// `StaleGeneration`, never `Unknown`, for it) but is no longer replayed
    /// on a future reconnect. The atomic above updates immediately; the
    /// pump's own ledger catches up via the same queue every other command
    /// travels, ahead of anything this call's caller enqueues next -- see
    /// `pump`'s ordering guarantee.
    pub fn advance_generation(&self, generation: Generation) {
        self.current_generation.store(generation, Ordering::SeqCst);
        self.pump
            .enqueue_control(PumpEvent::AdvanceGeneration(generation));
    }

    /// Stops accepting new work. Does not forcibly interrupt a `recv()`
    /// already blocked on the current connection -- for the real `TcpStream`
    /// transport, dropping the writer half does not by itself unblock a
    /// concurrent read on a cloned handle. A caller that needs the
    /// background threads to have fully stopped before returning is
    /// expected to sever the connection first (closing the socket at the
    /// source, as an `Unload` naturally does) and call this after.
    pub fn close(&self) {
        self.pump.close();
    }

    #[cfg(test)]
    fn join_reader_for_test(&self) {
        self.pump.join_for_test();
    }
}

impl DeploymentClientTrait for DeploymentClient {
    /// Enqueues one submission on the pump's bounded queue and returns
    /// immediately -- never blocking on the socket, whether or not it is
    /// draining. `Ok(())` means only "the pump now has this to work with,"
    /// never an admission verdict about what the backend will do with it
    /// (that arrives later, on the sink, as `Accepted`/`Rejected`/...); a
    /// full queue is the one thing that turns this into `Err`.
    fn try_submit(&self, mut submit: Submit) -> Result<(), EnqueueError> {
        // The generation is the backend's to issue, never the caller's to
        // invent, so whatever arrived on `submit` is overwritten here with
        // the one this client actually holds. A caller that stamped its own
        // number would otherwise send work the ledger then fences off as
        // `StaleGeneration` -- the submission reaches the wire and every
        // event about it is silently discarded, which looks like a backend
        // that answers nothing rather than like a disagreement.
        submit.deployment_generation = self.generation();
        self.pump
            .enqueue(PumpEvent::Submit(submit))
            .map_err(|()| EnqueueError("deployment client queue is full or closed".into()))
    }

    /// Ends a submission early. Infallible: a closed client, a full queue,
    /// or a `submission_id` this client never heard of, is silently a no-op
    /// -- there is nothing left for a caller to do with an error here, and
    /// the contract itself treats cancel of an unknown/settled submission as
    /// a no-op (`SEALED-CONTRACT.md` §2).
    fn cancel(&self, submission_id: SubmissionId) {
        self.pump.enqueue_control(PumpEvent::Cancel(submission_id));
    }
}

#[cfg(test)]
mod reconnect_tests;
#[cfg(test)]
mod tests;

use super::control::ControlMailbox;
use super::full_retry::FullRetries;
use super::reader::spawn_reader;
use super::{COMMAND_QUEUE_BOUND, INBOUND_BOUND, Pump, PumpEvent};
use crate::client::permits::Permits;
use crate::contract::{Generation, SubmissionId};
use crate::ledger::Ledger;
use crate::transport::{TransportFactory, TransportReader, TransportWriter};
use p4_adapter::deployment::Sink;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub(crate) struct PumpHandle {
    sender: std::sync::mpsc::Sender<PumpEvent>,
    outstanding: Arc<AtomicUsize>,
    #[allow(dead_code)]
    pump_thread: Mutex<Option<JoinHandle<()>>>,
    #[allow(dead_code)]
    reader_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    closed: Arc<AtomicBool>,
    reconnects: Arc<AtomicU64>,
    full_retries: Arc<AtomicU64>,
    inbound: Arc<Permits>,
    controls: Arc<ControlMailbox>,
    live: Arc<Mutex<HashSet<SubmissionId>>>,
}

impl PumpHandle {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        factory: Arc<dyn TransportFactory>,
        sink: Arc<dyn Sink>,
        writer: Box<dyn TransportWriter>,
        reader: Box<dyn TransportReader>,
        generation: Generation,
        backoff: Duration,
        current_generation: Arc<AtomicU64>,
    ) -> Self {
        let (sender, receiver) = channel();
        let outstanding = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicBool::new(false));
        let reconnects = Arc::new(AtomicU64::new(0));
        let full_retries = Arc::new(AtomicU64::new(0));
        let reader_thread = Arc::new(Mutex::new(None));
        let inbound = Arc::new(Permits::new(INBOUND_BOUND));
        let live = Arc::new(Mutex::new(HashSet::new()));
        let controls = Arc::new(ControlMailbox::new(sender.clone(), Arc::clone(&live)));
        let pump = Pump {
            outstanding_submissions: Arc::clone(&outstanding),
            factory,
            sink,
            writer: Some(writer),
            ledger: Ledger::new(generation),
            backoff,
            closed: Arc::clone(&closed),
            reconnects: Arc::clone(&reconnects),
            full_retry_count: Arc::clone(&full_retries),
            link_epoch: 0,
            current_generation,
            sender: sender.clone(),
            reader_thread: Arc::clone(&reader_thread),
            inbound: Arc::clone(&inbound),
            full_retries: FullRetries::default(),
            controls: Arc::clone(&controls),
            live: Arc::clone(&live),
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
            outstanding,
            pump_thread: Mutex::new(Some(pump_thread)),
            reader_thread,
            closed,
            reconnects,
            full_retries,
            inbound,
            controls,
            live,
        }
    }

    pub(crate) fn enqueue_submit(&self, submit: crate::contract::Submit) -> Result<(), ()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(());
        }
        if self
            .outstanding
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                (count < COMMAND_QUEUE_BOUND).then_some(count + 1)
            })
            .is_err()
        {
            return Err(());
        }
        let submission_id = submit.submission_id.clone();
        let inserted_live = self
            .live
            .lock()
            .expect("live submissions lock")
            .insert(submission_id.clone());
        if self
            .sender
            .send(PumpEvent::Submit {
                submit,
                inserted_live,
            })
            .is_err()
        {
            self.outstanding.fetch_sub(1, Ordering::SeqCst);
            if inserted_live {
                // The pump never saw this id, so it cannot remove it.
                // There is no live submission behind this bookkeeping row.
                self.live
                    .lock()
                    .expect("live submissions lock")
                    .remove(&submission_id);
            }
            return Err(());
        }
        Ok(())
    }

    pub(crate) fn enqueue_cancel(&self, submission_id: SubmissionId) {
        if !self.closed.load(Ordering::SeqCst) {
            self.controls.cancel(submission_id);
        }
    }

    pub(crate) fn enqueue_generation(&self, generation: Generation) {
        if !self.closed.load(Ordering::SeqCst) {
            self.controls.advance_generation(generation);
        }
    }

    pub(crate) fn reconnect_count(&self) -> u64 {
        self.reconnects.load(Ordering::SeqCst)
    }

    pub(crate) fn full_retry_count(&self) -> u64 {
        self.full_retries.load(Ordering::SeqCst)
    }

    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
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

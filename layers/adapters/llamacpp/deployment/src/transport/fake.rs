//! An in-memory transport for tests. No socket, no `apps/llama` process, no
//! GPU -- this is what makes the client's concurrency, backpressure, dedup,
//! fencing, and reconnect behaviour provable in `cargo test`.
//!
//! A test calls `FakeFactory::queue_success()` or `queue_failure()` once per
//! connection attempt it expects the client to make, in order. Each
//! `queue_success()` returns a `FakeLinkHandle` the test keeps: it can push
//! events into that specific connection, sever it (`disconnect`,
//! `fail_with`), and read back exactly what the client sent over it.

use super::{TransportFactory, TransportReader, TransportWriter};
use crate::contract::{Command, Event};
use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

type Delivery = io::Result<Option<Event>>;

pub struct FakeFactory {
    outcomes: Mutex<VecDeque<Outcome>>,
    reported_generation: Mutex<Option<u64>>,
}

enum Outcome {
    Fail(String),
    Succeed {
        sent: Arc<Mutex<Vec<Command>>>,
        fail_writes: Arc<AtomicBool>,
        write_gate: Arc<(Mutex<bool>, Condvar)>,
        rx: Receiver<Delivery>,
        generation: Option<u64>,
    },
}

impl FakeFactory {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            outcomes: Mutex::new(VecDeque::new()),
            reported_generation: Mutex::new(None),
        })
    }

    /// Queues the next `connect()` call to succeed, returning a handle the
    /// test uses to drive that specific connection.
    pub fn queue_success(&self) -> FakeLinkHandle {
        self.queue_success_with_generation(None)
    }

    pub fn queue_success_reporting(&self, generation: u64) -> FakeLinkHandle {
        self.queue_success_with_generation(Some(generation))
    }

    fn queue_success_with_generation(&self, generation: Option<u64>) -> FakeLinkHandle {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let fail_writes = Arc::new(AtomicBool::new(false));
        let write_gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (tx, rx) = mpsc::channel();
        self.outcomes
            .lock()
            .expect("outcomes lock")
            .push_back(Outcome::Succeed {
                sent: sent.clone(),
                fail_writes: fail_writes.clone(),
                write_gate: write_gate.clone(),
                rx,
                generation,
            });
        FakeLinkHandle {
            sent,
            fail_writes,
            write_gate,
            tx,
        }
    }

    /// Queues the next `connect()` call to fail with the given detail.
    pub fn queue_failure(&self, detail: impl Into<String>) {
        self.outcomes
            .lock()
            .expect("outcomes lock")
            .push_back(Outcome::Fail(detail.into()));
    }

    /// How many queued outcomes were never consumed by a `connect()` call.
    pub fn remaining(&self) -> usize {
        self.outcomes.lock().expect("outcomes lock").len()
    }
}

impl TransportFactory for FakeFactory {
    fn connect(&self) -> io::Result<(Box<dyn TransportWriter>, Box<dyn TransportReader>)> {
        let outcome = self
            .outcomes
            .lock()
            .expect("outcomes lock")
            .pop_front()
            .ok_or_else(|| {
                io::Error::other("fake factory: connect() called with nothing queued")
            })?;
        match outcome {
            Outcome::Fail(detail) => Err(io::Error::other(detail)),
            Outcome::Succeed {
                sent,
                fail_writes,
                write_gate,
                rx,
                generation,
            } => {
                *self
                    .reported_generation
                    .lock()
                    .expect("reported generation lock") = generation;
                Ok((
                    Box::new(FakeWriter {
                        sent,
                        fail_writes,
                        write_gate,
                    }) as Box<dyn TransportWriter>,
                    Box::new(FakeReader { rx }) as Box<dyn TransportReader>,
                ))
            }
        }
    }

    fn reported_generation(&self) -> Option<u64> {
        *self
            .reported_generation
            .lock()
            .expect("reported generation lock")
    }
}

/// The test-side handle to one connection a `FakeFactory` handed out.
#[derive(Clone)]
pub struct FakeLinkHandle {
    sent: Arc<Mutex<Vec<Command>>>,
    fail_writes: Arc<AtomicBool>,
    write_gate: Arc<(Mutex<bool>, Condvar)>,
    tx: Sender<Delivery>,
}

impl FakeLinkHandle {
    pub fn push_event(&self, event: Event) {
        let _ = self.tx.send(Ok(Some(event)));
    }

    /// Simulates the server closing the socket cleanly.
    pub fn disconnect(&self) {
        let _ = self.tx.send(Ok(None));
    }

    /// Simulates a transport-level failure -- a reset connection, a broken
    /// pipe -- as distinct from `disconnect`'s clean close. Neither of these
    /// is a `Rejected { reason: Full }`; that distinction is what
    /// `client::tests::socket_loss_is_never_mistaken_for_full` pins.
    pub fn fail_with(&self, detail: impl Into<String>) {
        let _ = self.tx.send(Err(io::Error::other(detail.into())));
    }

    /// Makes every subsequent `send` on this connection fail, to test what
    /// happens when a submit or cancel cannot reach an already-open socket.
    pub fn fail_writes(&self) {
        self.fail_writes.store(true, Ordering::SeqCst);
    }

    /// Makes every subsequent `send` on this connection block until
    /// [`Self::unblock_writes`] is called -- simulating a socket that is not
    /// draining, so a test can prove `try_submit` never waits on it.
    pub fn block_writes(&self) {
        let (gate, _) = &*self.write_gate;
        *gate.lock().expect("write gate lock") = true;
    }

    pub fn unblock_writes(&self) {
        let (gate, condvar) = &*self.write_gate;
        *gate.lock().expect("write gate lock") = false;
        condvar.notify_all();
    }

    pub fn sent(&self) -> Vec<Command> {
        self.sent.lock().expect("sent lock").clone()
    }
}

struct FakeWriter {
    sent: Arc<Mutex<Vec<Command>>>,
    fail_writes: Arc<AtomicBool>,
    write_gate: Arc<(Mutex<bool>, Condvar)>,
}

impl TransportWriter for FakeWriter {
    fn send(&mut self, command: &Command) -> io::Result<()> {
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err(io::Error::other("fake transport: write failed"));
        }
        let (gate, condvar) = &*self.write_gate;
        let mut blocked = gate.lock().expect("write gate lock");
        while *blocked {
            blocked = condvar.wait(blocked).expect("write gate wait");
        }
        drop(blocked);
        self.sent.lock().expect("sent lock").push(command.clone());
        Ok(())
    }
}

struct FakeReader {
    rx: Receiver<Delivery>,
}

impl TransportReader for FakeReader {
    fn recv(&mut self) -> io::Result<Option<Event>> {
        match self.rx.recv() {
            Ok(delivery) => delivery,
            // The handle (and every clone of its sender) was dropped without
            // an explicit `disconnect()`/`fail_with()` -- treat that the same
            // as a clean close, matching what a dropped real socket looks
            // like to `TcpReader`.
            Err(_) => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{Accepted, RejectReason, Rejected, SettleReason, Settled};

    #[test]
    fn queued_success_delivers_pushed_events_in_order() {
        let factory = FakeFactory::new();
        let handle = factory.queue_success();
        let (_writer, mut reader) = factory.connect().expect("connect");
        handle.push_event(Event::Accepted(Accepted {
            submission_id: "s".into(),
        }));
        handle.push_event(Event::Settled(Settled {
            submission_id: "s".into(),
            reason: SettleReason::Stop,
            generated_tokens: 1,
        }));
        assert_eq!(
            reader.recv().unwrap(),
            Some(Event::Accepted(Accepted {
                submission_id: "s".into(),
            }))
        );
        assert_eq!(
            reader.recv().unwrap(),
            Some(Event::Settled(Settled {
                submission_id: "s".into(),
                reason: SettleReason::Stop,
                generated_tokens: 1,
            }))
        );
    }

    #[test]
    fn queued_failure_surfaces_from_connect() {
        let factory = FakeFactory::new();
        factory.queue_failure("no route to host");
        assert!(factory.connect().is_err());
    }

    #[test]
    fn disconnect_is_clean_eof_not_an_error() {
        let factory = FakeFactory::new();
        let handle = factory.queue_success();
        let (_writer, mut reader) = factory.connect().expect("connect");
        handle.disconnect();
        assert_eq!(reader.recv().unwrap(), None);
    }

    #[test]
    fn fail_with_surfaces_as_an_io_error_distinct_from_a_full_rejection() {
        let factory = FakeFactory::new();
        let handle = factory.queue_success();
        let (_writer, mut reader) = factory.connect().expect("connect");
        handle.fail_with("connection reset");
        let error = reader.recv().expect_err("expected an io error");
        assert!(error.to_string().contains("connection reset"));
    }

    #[test]
    fn sent_records_writes_and_fail_writes_stops_them() {
        let factory = FakeFactory::new();
        let handle = factory.queue_success();
        let (mut writer, _reader) = factory.connect().expect("connect");
        writer
            .send(&Command::Cancel(crate::contract::Cancel {
                submission_id: "s".into(),
            }))
            .expect("first send ok");
        assert_eq!(handle.sent().len(), 1);
        handle.fail_writes();
        let result = writer.send(&Command::Cancel(crate::contract::Cancel {
            submission_id: "s".into(),
        }));
        assert!(result.is_err());
        assert_eq!(handle.sent().len(), 1);
    }

    #[test]
    fn full_reject_reason_is_not_confused_with_a_transport_error() {
        let factory = FakeFactory::new();
        let handle = factory.queue_success();
        let (_writer, mut reader) = factory.connect().expect("connect");
        handle.push_event(Event::Rejected(Rejected {
            submission_id: "s".into(),
            reason: RejectReason::Full,
        }));
        let event = reader.recv().unwrap().expect("some event");
        assert_eq!(
            event,
            Event::Rejected(Rejected {
                submission_id: "s".into(),
                reason: RejectReason::Full,
            })
        );
    }
}

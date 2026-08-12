use super::*;
use p4_protocol::Message;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn a_full_batch_dispatches_without_waiting_for_linger() {
    let harness = Harness::new(2, 8, 2, Duration::from_secs(1));
    assert!(harness.scheduler.submit(harness.job("one")).is_ok());
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(harness.started.load(Ordering::SeqCst), 0);
    assert!(harness.scheduler.submit(harness.job("two")).is_ok());
    harness.wait_started(2);
    harness.release();
}

#[test]
fn a_cycle_completion_dispatches_a_partial_batch() {
    let harness = Harness::new(2, 8, 2, Duration::from_secs(1));
    assert!(harness.scheduler.submit(harness.job("one")).is_ok());
    assert!(harness.scheduler.submit(harness.job("two")).is_ok());
    harness.wait_started(2);
    assert!(harness.scheduler.submit(harness.job("partial")).is_ok());
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(harness.started.load(Ordering::SeqCst), 2);
    harness.release();
    harness.wait_started(3);
}

#[test]
fn waiting_capacity_is_bounded_independently_from_active_jobs() {
    let harness = Harness::new(1, 1, 1, Duration::ZERO);
    assert!(harness.scheduler.submit(harness.job("active")).is_ok());
    harness.wait_started(1);
    assert!(harness.scheduler.submit(harness.job("waiting")).is_ok());
    assert!(harness.scheduler.submit(harness.job("rejected")).is_err());
    harness.release();
}

struct Harness {
    scheduler: Scheduler,
    started: Arc<AtomicUsize>,
    gate: Arc<(Mutex<bool>, Condvar)>,
    sender: SyncSender<RoutedMessage>,
}

impl Harness {
    fn new(workers: usize, queued: usize, batch: usize, linger: Duration) -> Self {
        let started = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_started = Arc::clone(&started);
        let worker_gate = Arc::clone(&gate);
        let scheduler = Scheduler::start(
            workers,
            queued,
            batch,
            Arc::new(FixedLinger(linger)),
            Arc::new(move |_| {
                worker_started.fetch_add(1, Ordering::SeqCst);
                let (lock, ready) = &*worker_gate;
                let guard = lock.lock().unwrap();
                drop(ready.wait_while(guard, |open| !*open).unwrap());
            }),
        )
        .unwrap();
        let (sender, _) = std::sync::mpsc::sync_channel(8);
        Self {
            scheduler,
            started,
            gate,
            sender,
        }
    }

    fn job(&self, route: &str) -> Job {
        Job {
            routed: RoutedMessage {
                route_id: route.into(),
                deadline_unix_ms: 0,
                message: Message::Cancel {
                    request_id: route.into(),
                    reason: "test".into(),
                },
            },
            responses: self.sender.clone(),
        }
    }

    fn wait_started(&self, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while self.started.load(Ordering::SeqCst) < expected {
            assert!(
                Instant::now() < deadline,
                "workers did not reach {expected}"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn release(&self) {
        let (lock, ready) = &*self.gate;
        *lock.lock().unwrap() = true;
        ready.notify_all();
    }
}

//! Backend-neutral mailbox tests: no worker, engine, timer or transport proof.
use super::*;
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, EventClass};
use std::sync::Weak;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::Wake;

const HANDSHAKE_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

fn finish<T>(handle: std::thread::JoinHandle<T>) -> T {
    let deadline = std::time::Instant::now() + HANDSHAKE_LIMIT;
    while !handle.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "mailbox test participant failed to terminate"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    handle.join().unwrap()
}

fn event(id: &str) -> Event {
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: id.into(),
            correlation_id: "mailbox-request".into(),
            causation_id: None,
            source: Endpoint::agent(Address::tcp("127.0.0.1", 52101)),
            target: Endpoint::node(Address::tcp("127.0.0.1", 52101), "mock", 1),
            return_route: None,
            class: EventClass::Control,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: Some("neutral-mailbox-test".into()),
            payload_content_type: "application/test".into(),
        },
        payload: vec![0, 1, 255],
    }
}

#[derive(Default)]
struct CountWake(AtomicUsize);

impl Wake for CountWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct ReaderCloseWake {
    mailbox: Weak<CompletionMailbox>,
    calls: AtomicUsize,
    saw_closed: AtomicBool,
}

impl Wake for ReaderCloseWake {
    fn wake(self: Arc<Self>) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.saw_closed.store(
            self.mailbox.upgrade().unwrap().try_take() == Poll::Closed,
            Ordering::SeqCst,
        );
    }
}

#[test]
fn the_last_publisher_drop_wakes_a_pending_reader_after_disconnect() {
    let (publisher, mailbox) = completion_mailbox(1);
    let wake = Arc::new(ReaderCloseWake {
        mailbox: Arc::downgrade(&mailbox),
        calls: AtomicUsize::new(0),
        saw_closed: AtomicBool::new(false),
    });
    let waker = Waker::from(Arc::clone(&wake));
    assert_eq!(
        mailbox.poll_take(&mut Context::from_waker(&waker)),
        TaskPoll::Pending
    );
    drop(publisher);
    assert_eq!(
        wake.calls.load(Ordering::SeqCst),
        1,
        "disconnect must wake a registered reader"
    );
    assert!(
        wake.saw_closed.load(Ordering::SeqCst),
        "reader must observe actual disconnect inside its wake callback"
    );
    assert_eq!(mailbox.try_take(), Poll::Closed);
}

struct ReaderLockProbe {
    lock: Weak<Mutex<Option<Waker>>>,
    ran: AtomicBool,
    locked: AtomicBool,
}

impl Wake for ReaderLockProbe {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ran.store(true, Ordering::SeqCst);
        let lock = self.lock.upgrade().expect("mailbox is alive");
        self.locked
            .store(lock.try_lock().is_err(), Ordering::SeqCst);
    }
}

#[test]
fn a_reader_wake_is_invoked_outside_the_registration_mutex() {
    let (publisher, mailbox) = completion_mailbox(1);
    let probe = Arc::new(ReaderLockProbe {
        lock: Arc::downgrade(&mailbox.waker),
        ran: AtomicBool::new(false),
        locked: AtomicBool::new(false),
    });
    let waker = Waker::from(Arc::clone(&probe));
    assert_eq!(
        mailbox.poll_take(&mut Context::from_waker(&waker)),
        TaskPoll::Pending
    );
    publisher.try_publish(event("one")).unwrap();
    assert!(probe.ran.load(Ordering::SeqCst));
    assert!(
        !probe.locked.load(Ordering::SeqCst),
        "reentrant reader wake must not inherit the registration lock"
    );
}

#[test]
fn drain_wakes_all_persistent_listeners_but_reserves_no_slot() {
    let (publisher, mailbox) = completion_mailbox(1);
    let other = publisher.clone();
    let a = Arc::new(CountWake::default());
    let b = Arc::new(CountWake::default());
    let _a = publisher
        .capacity_listener(&Waker::from(Arc::clone(&a)))
        .unwrap();
    let _b = other
        .capacity_listener(&Waker::from(Arc::clone(&b)))
        .unwrap();
    let first = event("first");
    let held = event("held");
    publisher.try_publish(first.clone()).unwrap();
    assert_eq!(
        publisher.try_publish(held.clone()),
        Err(PublishError::Full(held.clone()))
    );
    assert_eq!(mailbox.try_take(), Poll::Event(first));
    assert_eq!(a.0.load(Ordering::SeqCst), 1);
    assert_eq!(b.0.load(Ordering::SeqCst), 1);
    // An independent producer may consume the newly freed capacity.
    other.try_publish(event("other-wins")).unwrap();
    assert_eq!(
        publisher.try_publish(held.clone()),
        Err(PublishError::Full(held.clone()))
    );
    assert_eq!(mailbox.try_take(), Poll::Event(event("other-wins")));
    assert_eq!(
        a.0.load(Ordering::SeqCst),
        2,
        "listener survives its first notification"
    );
    publisher.try_publish(held.clone()).unwrap();
    assert_eq!(mailbox.try_take(), Poll::Event(held));
    assert_eq!(mailbox.try_take(), Poll::Empty);
    assert_eq!(a.0.load(Ordering::SeqCst), 3);
}

#[test]
fn cancellation_removes_only_its_listener_and_reuses_bounded_capacity() {
    let (publisher, mailbox) = completion_mailbox(1);
    let a = Arc::new(CountWake::default());
    let b = Arc::new(CountWake::default());
    let mut registrations = (0..MAX_CAPACITY_LISTENERS)
        .map(|_| {
            publisher
                .capacity_listener(&Waker::from(Arc::clone(&a)))
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        publisher
            .capacity_listener(&Waker::from(Arc::clone(&b)))
            .unwrap_err(),
        CapacityListenError::Exhausted
    );
    drop(registrations.pop());
    let replacement = publisher
        .capacity_listener(&Waker::from(Arc::clone(&b)))
        .unwrap();
    publisher.try_publish(event("one")).unwrap();
    assert!(matches!(mailbox.try_take(), Poll::Event(_)));
    assert_eq!(a.0.load(Ordering::SeqCst), MAX_CAPACITY_LISTENERS - 1);
    assert_eq!(b.0.load(Ordering::SeqCst), 1);
    drop(registrations);
    drop(replacement);
    assert!(publisher.capacity.lock().unwrap().listeners.is_empty());
    for _ in 0..(MAX_CAPACITY_LISTENERS * 4) {
        drop(
            publisher
                .capacity_listener(&Waker::from(Arc::clone(&b)))
                .unwrap(),
        );
    }
    assert!(
        publisher.capacity.lock().unwrap().listeners.is_empty(),
        "cancelled listeners cannot accumulate"
    );
    publisher.capacity.lock().unwrap().next_id = u64::MAX;
    assert_eq!(
        publisher.capacity_listener(&Waker::from(b)).unwrap_err(),
        CapacityListenError::Exhausted,
        "registration IDs never wrap and remove a different listener"
    );
}

struct ClosedWake {
    publisher: CompletionPublisher,
    calls: AtomicUsize,
    saw_closed: AtomicBool,
}

impl Wake for ClosedWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.saw_closed.store(
            matches!(
                self.publisher.try_publish(event("after-close")),
                Err(PublishError::Closed(_))
            ),
            Ordering::SeqCst,
        );
    }
}

#[test]
fn receiver_drop_disconnects_before_waking_every_capacity_listener() {
    let (publisher, mailbox) = completion_mailbox(1);
    publisher.try_publish(event("buffered")).unwrap();
    let probe = Arc::new(ClosedWake {
        publisher: publisher.clone(),
        calls: AtomicUsize::new(0),
        saw_closed: AtomicBool::new(false),
    });
    let count = Arc::new(CountWake::default());
    let _probe = publisher
        .capacity_listener(&Waker::from(Arc::clone(&probe)))
        .unwrap();
    let _count = publisher
        .capacity_listener(&Waker::from(Arc::clone(&count)))
        .unwrap();
    drop(mailbox);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    assert!(
        probe.saw_closed.load(Ordering::SeqCst),
        "callback must observe actual receiver closure, not Full"
    );
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        publisher
            .capacity_listener(&Waker::from(count))
            .unwrap_err(),
        CapacityListenError::Closed
    );
    assert!(publisher.capacity.lock().unwrap().listeners.is_empty());
}

#[test]
fn publisher_clones_and_buffered_events_keep_their_original_close_semantics() {
    let (publisher, mailbox) = completion_mailbox(2);
    let other = publisher.clone();
    let count = Arc::new(CountWake::default());
    let waker = Waker::from(count);
    assert_eq!(
        mailbox.poll_take(&mut Context::from_waker(&waker)),
        TaskPoll::Pending
    );
    drop(publisher);
    assert_eq!(
        mailbox.poll_take(&mut Context::from_waker(&waker)),
        TaskPoll::Pending
    );
    other.try_publish(event("one")).unwrap();
    other.try_publish(event("two")).unwrap();
    drop(other);
    assert_eq!(mailbox.try_take(), Poll::Event(event("one")));
    assert_eq!(mailbox.try_take(), Poll::Event(event("two")));
    assert_eq!(mailbox.try_take(), Poll::Closed);
}

#[test]
fn reader_rechecks_a_publish_between_empty_read_and_registration() {
    let (publisher, mailbox) = completion_mailbox(1);
    let (start, started) = mpsc::sync_channel(1);
    let (done, finished) = mpsc::sync_channel(1);
    let producer = std::thread::spawn(move || {
        started.recv_timeout(HANDSHAKE_LIMIT).unwrap();
        publisher.try_publish(event("between")).unwrap();
        done.send(()).unwrap();
    });
    let reader = Arc::new(CountWake::default());
    let weak = Arc::downgrade(&reader);
    let waker = Waker::from(reader);
    let result = mailbox.poll_take_before_register(&mut Context::from_waker(&waker), || {
        start.send(()).unwrap();
        finished.recv_timeout(HANDSHAKE_LIMIT).unwrap();
    });
    finish(producer);
    assert_eq!(result, TaskPoll::Ready(Poll::Event(event("between"))));
    drop(waker);
    assert!(
        weak.upgrade().is_none(),
        "Ready(Event) must retire its reader registration"
    );
}

#[test]
fn reader_rechecks_disconnect_between_empty_read_and_registration() {
    let (publisher, mailbox) = completion_mailbox(1);
    let (start, started) = mpsc::sync_channel(1);
    let (done, finished) = mpsc::sync_channel(1);
    let producer = std::thread::spawn(move || {
        started.recv_timeout(HANDSHAKE_LIMIT).unwrap();
        drop(publisher);
        done.send(()).unwrap();
    });
    let reader = Arc::new(CountWake::default());
    let weak = Arc::downgrade(&reader);
    let waker = Waker::from(reader);
    let result = mailbox.poll_take_before_register(&mut Context::from_waker(&waker), || {
        start.send(()).unwrap();
        finished.recv_timeout(HANDSHAKE_LIMIT).unwrap();
    });
    finish(producer);
    assert_eq!(result, TaskPoll::Ready(Poll::Closed));
    drop(waker);
    assert!(
        weak.upgrade().is_none(),
        "Ready(Closed) must retire its reader registration"
    );
}

#[test]
fn simultaneous_drainers_notify_without_duplicate_event_delivery() {
    let (publisher, mailbox) = completion_mailbox(2);
    let wake = Arc::new(CountWake::default());
    let _listener = publisher
        .capacity_listener(&Waker::from(Arc::clone(&wake)))
        .unwrap();
    publisher.try_publish(event("one")).unwrap();
    publisher.try_publish(event("two")).unwrap();
    let mut starts = Vec::new();
    let readers = (0..2)
        .map(|_| {
            let mailbox = Arc::clone(&mailbox);
            let (start, started) = mpsc::sync_channel(1);
            starts.push(start);
            std::thread::spawn(move || {
                started.recv_timeout(HANDSHAKE_LIMIT).unwrap();
                mailbox.try_take()
            })
        })
        .collect::<Vec<_>>();
    for start in starts {
        start.send(()).unwrap();
    }
    let mut delivered = readers
        .into_iter()
        .map(|reader| match finish(reader) {
            Poll::Event(event) => event.envelope.event_id,
            other => panic!("each drainer must receive one event: {other:?}"),
        })
        .collect::<Vec<_>>();
    delivered.sort();
    assert_eq!(delivered, ["one", "two"]);
    assert_eq!(wake.0.load(Ordering::SeqCst), 2);
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

struct ReentrantCapacityWake {
    publisher: CompletionPublisher,
    mailbox: Weak<CompletionMailbox>,
    own: Mutex<Option<CapacityRegistration>>,
    next: Mutex<Option<CapacityRegistration>>,
    replacement: Arc<CountWake>,
    succeeded: AtomicBool,
    locked: AtomicBool,
}

impl Wake for ReentrantCapacityWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let Some(mailbox) = self.mailbox.upgrade() else {
            // During a failed assertion's teardown the receiver is already
            // gone. Its close wake must not cause a second, aborting panic.
            return;
        };
        if self.publisher.capacity.try_lock().is_err() || mailbox.receiver.try_lock().is_err() {
            self.locked.store(true, Ordering::SeqCst);
            return; // The assertion fails rather than deadlocking a mutant.
        }
        drop(self.own.lock().unwrap().take());
        *self.next.lock().unwrap() = Some(
            self.publisher
                .capacity_listener(&Waker::from(Arc::clone(&self.replacement)))
                .unwrap(),
        );
        self.publisher.try_publish(event("reentrant")).unwrap();
        self.succeeded.store(true, Ordering::SeqCst);
    }
}

#[test]
fn capacity_wake_can_cancel_itself_reregister_and_publish() {
    let (publisher, mailbox) = completion_mailbox(1);
    let replacement = Arc::new(CountWake::default());
    let probe = Arc::new(ReentrantCapacityWake {
        publisher: publisher.clone(),
        mailbox: Arc::downgrade(&mailbox),
        own: Mutex::new(None),
        next: Mutex::new(None),
        replacement: Arc::clone(&replacement),
        succeeded: AtomicBool::new(false),
        locked: AtomicBool::new(false),
    });
    *probe.own.lock().unwrap() = Some(
        publisher
            .capacity_listener(&Waker::from(Arc::clone(&probe)))
            .unwrap(),
    );
    publisher.try_publish(event("first")).unwrap();
    assert_eq!(mailbox.try_take(), Poll::Event(event("first")));
    assert!(
        !probe.locked.load(Ordering::SeqCst),
        "capacity wake must not inherit either mailbox mutex"
    );
    assert!(probe.succeeded.load(Ordering::SeqCst));
    assert_eq!(mailbox.try_take(), Poll::Event(event("reentrant")));
    assert_eq!(replacement.0.load(Ordering::SeqCst), 1);
    drop(probe.next.lock().unwrap().take());
    assert!(publisher.capacity.lock().unwrap().listeners.is_empty());
}

#[test]
fn mailbox_drop_releases_the_reader_registration_while_a_publisher_survives() {
    let (publisher, mailbox) = completion_mailbox(1);
    let reader = Arc::new(CountWake::default());
    let weak = Arc::downgrade(&reader);
    let waker = Waker::from(reader);
    assert_eq!(
        mailbox.poll_take(&mut Context::from_waker(&waker)),
        TaskPoll::Pending
    );
    drop(waker);
    assert!(
        weak.upgrade().is_some(),
        "the pending registration initially owns its reader"
    );
    drop(mailbox);
    assert!(
        weak.upgrade().is_none(),
        "a surviving publisher must not retain a departed reader task"
    );
    assert_eq!(
        publisher.try_publish(event("closed")),
        Err(PublishError::Closed(event("closed")))
    );
}

struct RawWakeProbe {
    capacity: Weak<Mutex<CapacityState>>,
    reader: Weak<Mutex<Option<Waker>>>,
    clones: AtomicUsize,
    drops: AtomicUsize,
    locked: AtomicBool,
}

impl RawWakeProbe {
    fn inspect(&self) {
        let capacity_locked = self
            .capacity
            .upgrade()
            .is_some_and(|lock| lock.try_lock().is_err());
        let reader_locked = self
            .reader
            .upgrade()
            .is_some_and(|lock| lock.try_lock().is_err());
        if capacity_locked || reader_locked {
            self.locked.store(true, Ordering::SeqCst);
        }
    }
}

unsafe fn raw_clone(data: *const ()) -> std::task::RawWaker {
    // Each RawWaker owns one Arc reference; cloning adds exactly one reference.
    let probe = unsafe { &*(data.cast::<RawWakeProbe>()) };
    probe.inspect();
    probe.clones.fetch_add(1, Ordering::SeqCst);
    unsafe { Arc::increment_strong_count(data.cast::<RawWakeProbe>()) };
    std::task::RawWaker::new(data, &RAW_VTABLE)
}
unsafe fn raw_drop(data: *const ()) {
    let probe = unsafe { Arc::from_raw(data.cast::<RawWakeProbe>()) };
    probe.inspect();
    probe.drops.fetch_add(1, Ordering::SeqCst);
}
unsafe fn raw_wake(data: *const ()) {
    unsafe { raw_drop(data) };
}
unsafe fn raw_wake_by_ref(data: *const ()) {
    unsafe { &*(data.cast::<RawWakeProbe>()) }.inspect();
}
static RAW_VTABLE: std::task::RawWakerVTable =
    std::task::RawWakerVTable::new(raw_clone, raw_wake, raw_wake_by_ref, raw_drop);

#[test]
fn caller_waker_clone_and_drop_also_run_outside_mailbox_mutexes() {
    let (publisher, mailbox) = completion_mailbox(1);
    let probe = Arc::new(RawWakeProbe {
        capacity: Arc::downgrade(&publisher.capacity),
        reader: Arc::downgrade(&mailbox.waker),
        clones: AtomicUsize::new(0),
        drops: AtomicUsize::new(0),
        locked: AtomicBool::new(false),
    });
    let raw = std::task::RawWaker::new(Arc::into_raw(Arc::clone(&probe)).cast(), &RAW_VTABLE);
    // The vtable above preserves one Arc reference per raw handle.
    let waker = unsafe { Waker::from_raw(raw) };
    let listener = publisher.capacity_listener(&waker).unwrap();
    assert_eq!(
        mailbox.poll_take(&mut Context::from_waker(&waker)),
        TaskPoll::Pending
    );
    assert_eq!(probe.clones.load(Ordering::SeqCst), 2);
    drop(listener);
    let replacement = Waker::from(Arc::new(CountWake::default()));
    assert_eq!(
        mailbox.poll_take(&mut Context::from_waker(&replacement)),
        TaskPoll::Pending
    );
    assert_eq!(probe.drops.load(Ordering::SeqCst), 2);
    drop(waker);
    assert_eq!(probe.drops.load(Ordering::SeqCst), 3);
    assert!(
        !probe.locked.load(Ordering::SeqCst),
        "even clone/drop vtable code must run outside internal locks"
    );
}

struct PanicWake;
impl Wake for PanicWake {
    fn wake(self: Arc<Self>) {
        panic!("caller waker panic");
    }
    fn wake_by_ref(self: &Arc<Self>) {
        panic!("caller waker panic");
    }
}

#[test]
fn a_caller_waker_panic_propagates_without_poisoning_internal_mutexes() {
    let (publisher, mailbox) = completion_mailbox(1);
    let registration = publisher
        .capacity_listener(&Waker::from(Arc::new(PanicWake)))
        .unwrap();
    publisher.try_publish(event("one")).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mailbox.try_take()));
    assert!(
        result.is_err(),
        "this API does not silently accept caller panics"
    );
    assert!(publisher.capacity.lock().is_ok());
    assert!(mailbox.receiver.lock().is_ok());
    assert!(mailbox.waker.lock().is_ok());
    drop(registration);
    // No claim that the event consumed by a panicking call was delivered.
    publisher.try_publish(event("after-panic")).unwrap();
    assert_eq!(mailbox.try_take(), Poll::Event(event("after-panic")));
}

#[test]
fn registering_before_the_attempt_covers_both_sides_of_a_concurrent_drain() {
    for drain_before_attempt in [false, true] {
        let (publisher, mailbox) = completion_mailbox(1);
        publisher.try_publish(event("old")).unwrap();
        let count = Arc::new(CountWake::default());
        let _registration = publisher
            .capacity_listener(&Waker::from(Arc::clone(&count)))
            .unwrap();
        let (start, started) = mpsc::sync_channel(1);
        let (done, finished) = mpsc::sync_channel(1);
        let drainer = std::thread::spawn(move || {
            started.recv_timeout(HANDSHAKE_LIMIT).unwrap();
            assert_eq!(mailbox.try_take(), Poll::Event(event("old")));
            done.send(()).unwrap();
            mailbox
        });
        if drain_before_attempt {
            start.send(()).unwrap();
            finished.recv_timeout(HANDSHAKE_LIMIT).unwrap();
        }
        let candidate = event("candidate");
        let first = publisher.try_publish(candidate.clone());
        if !drain_before_attempt {
            start.send(()).unwrap();
            finished.recv_timeout(HANDSHAKE_LIMIT).unwrap();
        }
        let mailbox = finish(drainer);
        if drain_before_attempt {
            assert_eq!(first, Ok(()));
        } else {
            assert_eq!(first, Err(PublishError::Full(candidate.clone())));
            assert_eq!(
                count.0.load(Ordering::SeqCst),
                1,
                "Full must be paired with the registered drain notification"
            );
            publisher.try_publish(candidate.clone()).unwrap();
        }
        assert_eq!(mailbox.try_take(), Poll::Event(candidate));
    }
}

//! A counting semaphore for the pump's inbound events, and the reason one
//! is needed at all.
//!
//! The pump reads commands and inbound events from a single unbounded
//! channel, which is right for commands -- a cancel that a queue depth
//! refuses is a request that keeps running against its caller's wishes --
//! but wrong for events. The backend pushes a `Produced` per token, and if
//! whatever consumes them is slower than the socket delivers them, an
//! unbounded channel absorbs the difference in memory until the process
//! dies. That is not backpressure; it is a leak with a delay on it.
//!
//! Bounding the channel itself would put commands and events under one
//! limit again. Instead the reader thread takes a permit before it hands an
//! event over and the pump returns it once the event is dealt with, so the
//! reader blocks when the pump falls behind, its `recv()` stops draining
//! the socket, and the TCP window closes on the backend. The pressure ends
//! up where it can actually be acted on.
//!
//! `std` has no semaphore, and pulling one in for this would be a
//! dependency for twenty lines.

use std::sync::{Condvar, Mutex};

pub(crate) struct Permits {
    available: Mutex<State>,
    returned: Condvar,
}

struct State {
    available: usize,
    /// Set when the pump stops. Every waiter is released and every later
    /// `acquire` returns immediately -- a reader blocked here when the pump
    /// exits would otherwise wait for a permit nothing will ever return.
    closed: bool,
}

impl Permits {
    pub(crate) fn new(count: usize) -> Self {
        Self {
            available: Mutex::new(State {
                available: count,
                closed: false,
            }),
            returned: Condvar::new(),
        }
    }

    /// Blocks until a permit is free, or returns `false` once closed.
    ///
    /// Blocking is the point: this is called on the reader thread, between
    /// `recv()` calls, so a reader that waits here is a socket that is not
    /// being drained.
    pub(crate) fn acquire(&self) -> bool {
        let mut state = self.available.lock().expect("permit lock");
        while state.available == 0 && !state.closed {
            state = self.returned.wait(state).expect("permit wait");
        }
        if state.closed {
            return false;
        }
        state.available -= 1;
        true
    }

    pub(crate) fn release(&self) {
        let mut state = self.available.lock().expect("permit lock");
        state.available += 1;
        drop(state);
        self.returned.notify_one();
    }

    pub(crate) fn close(&self) {
        let mut state = self.available.lock().expect("permit lock");
        state.closed = true;
        drop(state);
        self.returned.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn available(&self) -> usize {
        self.available.lock().expect("permit lock").available
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn a_reader_blocks_once_the_permits_are_gone_and_resumes_when_one_returns() {
        let permits = Arc::new(Permits::new(2));
        assert!(permits.acquire());
        assert!(permits.acquire());
        assert_eq!(permits.available(), 0);

        let taken = Arc::new(AtomicUsize::new(0));
        let waiter = {
            let permits = Arc::clone(&permits);
            let taken = Arc::clone(&taken);
            thread::spawn(move || {
                assert!(permits.acquire());
                taken.fetch_add(1, Ordering::SeqCst);
            })
        };

        // Stated rather than assumed: if this thread were not actually
        // blocked, the test below would pass without proving anything.
        thread::sleep(Duration::from_millis(50));
        assert_eq!(
            taken.load(Ordering::SeqCst),
            0,
            "the third acquire must be waiting, not proceeding"
        );

        permits.release();
        waiter.join().expect("the waiter resumes");
        assert_eq!(taken.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn closing_releases_a_blocked_reader_rather_than_stranding_it() {
        let permits = Arc::new(Permits::new(1));
        assert!(permits.acquire());

        let waiter = {
            let permits = Arc::clone(&permits);
            thread::spawn(move || permits.acquire())
        };
        thread::sleep(Duration::from_millis(50));

        permits.close();
        assert!(
            !waiter.join().expect("the waiter returns"),
            "a reader woken by close must be told there is no permit, not given one"
        );
        assert!(!permits.acquire(), "acquiring after close never blocks");
    }
}

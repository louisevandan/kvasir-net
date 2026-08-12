use p4_protocol::RoutedMessage;
use std::collections::VecDeque;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub(crate) struct Job {
    pub(crate) routed: RoutedMessage,
    pub(crate) responses: SyncSender<RoutedMessage>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct BatchSnapshot {
    pub(crate) pending: usize,
    pub(crate) active: usize,
    pub(crate) max_batch: usize,
    pub(crate) max_inflight: usize,
}

pub(crate) trait BatchHeuristic: Send + Sync {
    /// A zero duration dispatches a non-full batch immediately.
    fn partial_wait(&self, snapshot: BatchSnapshot) -> Duration;
}

pub(crate) struct FixedLinger(pub(crate) Duration);

impl BatchHeuristic for FixedLinger {
    fn partial_wait(&self, snapshot: BatchSnapshot) -> Duration {
        let _ = (
            snapshot.pending,
            snapshot.active,
            snapshot.max_batch,
            snapshot.max_inflight,
        );
        self.0
    }
}

pub(crate) struct Scheduler {
    queue: Arc<WorkQueue>,
}

struct WorkQueue {
    max_queued: usize,
    max_inflight: usize,
    max_batch: usize,
    heuristic: Arc<dyn BatchHeuristic>,
    state: Mutex<State>,
    dispatch_ready: Condvar,
    worker_ready: Condvar,
}

#[derive(Default)]
struct State {
    pending: VecDeque<Job>,
    ready: VecDeque<Job>,
    active: usize,
    cycle_epoch: u64,
    observed_cycle: u64,
    first_pending_at: Option<Instant>,
}

impl Scheduler {
    pub(crate) fn start(
        workers: usize,
        max_queued: usize,
        max_batch: usize,
        heuristic: Arc<dyn BatchHeuristic>,
        handler: Arc<dyn Fn(Job) + Send + Sync>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if workers == 0 || max_queued == 0 || max_batch == 0 || max_batch > workers {
            return Err(
                "llama.cpp scheduler requires 1 <= max_batch <= workers and a non-zero queue"
                    .into(),
            );
        }
        let queue = Arc::new(WorkQueue {
            max_queued,
            max_inflight: workers,
            max_batch,
            heuristic,
            state: Mutex::new(State {
                pending: VecDeque::with_capacity(max_queued),
                ready: VecDeque::with_capacity(workers),
                ..State::default()
            }),
            dispatch_ready: Condvar::new(),
            worker_ready: Condvar::new(),
        });
        spawn_dispatcher(Arc::clone(&queue))?;
        for index in 0..workers {
            spawn_worker(index, Arc::clone(&queue), Arc::clone(&handler))?;
        }
        Ok(Self { queue })
    }

    pub(crate) fn submit(&self, job: Job) -> Result<(), Job> {
        self.queue.push(job)
    }

    /// Future backend telemetry may call this at a finer granularity than the
    /// current HTTP-request completion hint.
    #[allow(dead_code)]
    pub(crate) fn notify_backend_cycle(&self) {
        self.queue.cycle_hint(false);
    }
}

fn spawn_dispatcher(queue: Arc<WorkQueue>) -> Result<(), std::io::Error> {
    std::thread::Builder::new()
        .name("p4-llamacpp-batch-dispatch".into())
        .stack_size(512 * 1024)
        .spawn(move || queue.dispatch_loop())?;
    Ok(())
}

fn spawn_worker(
    index: usize,
    queue: Arc<WorkQueue>,
    handler: Arc<dyn Fn(Job) + Send + Sync>,
) -> Result<(), std::io::Error> {
    std::thread::Builder::new()
        .name(format!("p4-llamacpp-{index}"))
        .stack_size(512 * 1024)
        .spawn(move || {
            loop {
                let job = queue.pop_ready();
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(job)));
                queue.cycle_hint(true);
                if result.is_err() {
                    eprintln!("P4_LLAMACPP_WORKER_PANIC worker={index}");
                }
            }
        })?;
    Ok(())
}

impl WorkQueue {
    fn push(&self, job: Job) -> Result<(), Job> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.pending.len() >= self.max_queued {
            return Err(job);
        }
        if state.pending.is_empty() {
            state.first_pending_at = Some(Instant::now());
        }
        state.pending.push_back(job);
        self.dispatch_ready.notify_one();
        Ok(())
    }

    fn dispatch_loop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        loop {
            while state.pending.is_empty() || state.active >= self.max_inflight {
                state = self
                    .dispatch_ready
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
            let capacity = self.max_inflight - state.active;
            let dispatch_limit = self.max_batch.min(capacity);
            let batch_full = state.pending.len() >= self.max_batch;
            let capacity_full = state.pending.len() >= capacity;
            let cycle_ready = state.cycle_epoch > state.observed_cycle;
            if batch_full || capacity_full || cycle_ready {
                if cycle_ready {
                    state.observed_cycle = state.cycle_epoch;
                }
                let trigger = if batch_full {
                    "batch_full"
                } else if cycle_ready {
                    "backend_cycle_hint"
                } else {
                    "capacity_full"
                };
                self.promote(&mut state, dispatch_limit, trigger);
                continue;
            }
            let snapshot = BatchSnapshot {
                pending: state.pending.len(),
                active: state.active,
                max_batch: self.max_batch,
                max_inflight: self.max_inflight,
            };
            let wait = self.heuristic.partial_wait(snapshot);
            let queued_for = state
                .first_pending_at
                .map_or(Duration::ZERO, |at| at.elapsed());
            if wait.is_zero() || queued_for >= wait {
                self.promote(
                    &mut state,
                    dispatch_limit,
                    if wait.is_zero() {
                        "heuristic_immediate"
                    } else {
                        "linger_expired"
                    },
                );
                continue;
            }
            let remaining = wait.saturating_sub(queued_for);
            let (next, _) = self
                .dispatch_ready
                .wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner());
            state = next;
        }
    }

    fn promote(&self, state: &mut State, limit: usize, trigger: &str) {
        let count = limit.min(state.pending.len());
        let queued_ms = state
            .first_pending_at
            .map_or(0, |at| at.elapsed().as_millis());
        for _ in 0..count {
            state
                .ready
                .push_back(state.pending.pop_front().expect("pending batch diverged"));
        }
        state.active += count;
        state.first_pending_at = (!state.pending.is_empty()).then(Instant::now);
        println!(
            "P4_LLAMACPP_BATCH trigger={trigger} size={count} pending_after={} active={} oldest_wait_ms={queued_ms}",
            state.pending.len(),
            state.active
        );
        self.worker_ready.notify_all();
    }

    fn pop_ready(&self) -> Job {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        loop {
            if let Some(job) = state.ready.pop_front() {
                return job;
            }
            state = self
                .worker_ready
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
    }

    fn cycle_hint(&self, completed_execution: bool) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if completed_execution {
            state.active = state.active.saturating_sub(1);
        }
        if !state.pending.is_empty() {
            state.cycle_epoch = state.cycle_epoch.wrapping_add(1);
        }
        self.dispatch_ready.notify_one();
    }
}

#[cfg(test)]
mod tests;

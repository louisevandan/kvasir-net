//! Competing consumers for one bounded lane. The lane deliberately provides
//! no per-correlation ordering; causal handlers enqueue their own successor.

use super::{LaneConfig, TaskContext, TaskHandler, TaskQueue};
use p4_protocol::{QueueClass, TaskEnvelope};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(super) struct Lane {
    state: Arc<LaneState>,
}

struct LaneState {
    pending: Mutex<VecDeque<QueuedTask>>,
    ready: Semaphore,
    items: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
}

struct QueuedTask {
    envelope: TaskEnvelope,
    _item: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
}

impl Lane {
    pub(super) fn new(config: &LaneConfig) -> Self {
        Self {
            state: Arc::new(LaneState {
                pending: Mutex::new(VecDeque::with_capacity(config.items)),
                ready: Semaphore::new(0),
                items: Arc::new(Semaphore::new(config.items)),
                bytes: Arc::new(Semaphore::new(config.bytes)),
            }),
        }
    }

    pub(super) fn push(
        &self,
        envelope: TaskEnvelope,
        bytes: u32,
    ) -> Result<(), (QueueClass, &'static str)> {
        let class = envelope.queue;
        let item = Arc::clone(&self.state.items)
            .try_acquire_owned()
            .map_err(|_| (class, "item queue is full"))?;
        let bytes = Arc::clone(&self.state.bytes)
            .try_acquire_many_owned(bytes)
            .map_err(|_| (class, "byte budget is full"))?;
        self.state
            .pending
            .lock()
            .map_err(|_| (class, "task queue lock is poisoned"))?
            .push_back(QueuedTask {
                envelope,
                _item: item,
                _bytes: bytes,
            });
        self.state.ready.add_permits(1);
        Ok(())
    }

    pub(super) fn spawn(&self, workers: usize, handler: Arc<dyn TaskHandler>, queue: TaskQueue) {
        for _ in 0..workers {
            let state = Arc::clone(&self.state);
            let handler = Arc::clone(&handler);
            let queue = queue.clone();
            tokio::spawn(async move {
                loop {
                    let Ok(ready) = state.ready.acquire().await else {
                        return;
                    };
                    ready.forget();
                    let queued = state
                        .pending
                        .lock()
                        .expect("P4 task queue lock poisoned")
                        .pop_front()
                        .expect("P4 task readiness diverged from pending queue");
                    let context = TaskContext {
                        queue: queue.clone(),
                    };
                    let result = handler.handle(queued.envelope, &context);
                    queue.finish(&result);
                }
            });
        }
    }
}

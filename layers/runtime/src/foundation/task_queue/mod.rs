//! Transport-independent bounded worker queues for self-describing P4 tasks.
//! See `apps/p4/docs/task-runtime.md#generic-workers`.

use p4_protocol::{Participant, QueueClass, RoutedMessage, TaskEnvelope, encode_routed_message};
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

mod worker;

use worker::Lane;

pub trait TaskHandler: Send + Sync + 'static {
    /// Must only validate/mutate local state and enqueue follow-up tasks.
    /// Blocking I/O and waiting for a response are forbidden here.
    fn handle(&self, task: TaskEnvelope, context: &TaskContext) -> TaskResult;
}

pub type TaskResult = Result<(), TaskQueueError>;

#[derive(Clone)]
pub struct TaskQueue {
    inner: Arc<QueueInner>,
}

pub struct TaskContext {
    queue: TaskQueue,
}

#[derive(Clone, Debug)]
pub struct TaskQueueConfig {
    pub control: LaneConfig,
    pub prefill: LaneConfig,
    pub decode: LaneConfig,
    pub response: LaneConfig,
}

#[derive(Clone, Debug)]
pub struct LaneConfig {
    pub items: usize,
    pub bytes: usize,
    pub workers: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskQueueStats {
    pub enqueued: u64,
    pub completed: u64,
    pub rejected: u64,
    pub failed: u64,
}

struct QueueInner {
    id_prefix: String,
    sequence: AtomicU64,
    control: Lane,
    prefill: Lane,
    decode: Lane,
    response: Lane,
    stats: Stats,
}

#[derive(Default)]
struct Stats {
    enqueued: AtomicU64,
    completed: AtomicU64,
    rejected: AtomicU64,
    failed: AtomicU64,
}

impl TaskQueue {
    pub fn start(
        id_prefix: impl Into<String>,
        config: TaskQueueConfig,
        handler: Arc<dyn TaskHandler>,
    ) -> Result<Self, TaskQueueError> {
        config.validate()?;
        let control = Lane::new(&config.control);
        let prefill = Lane::new(&config.prefill);
        let decode = Lane::new(&config.decode);
        let response = Lane::new(&config.response);
        let inner = Arc::new(QueueInner {
            id_prefix: id_prefix.into(),
            sequence: AtomicU64::new(1),
            control,
            prefill,
            decode,
            response,
            stats: Stats::default(),
        });
        let queue = Self { inner };
        queue
            .inner
            .control
            .spawn(config.control.workers, handler.clone(), queue.clone());
        queue
            .inner
            .prefill
            .spawn(config.prefill.workers, handler.clone(), queue.clone());
        queue
            .inner
            .decode
            .spawn(config.decode.workers, handler.clone(), queue.clone());
        queue
            .inner
            .response
            .spawn(config.response.workers, handler, queue.clone());
        Ok(queue)
    }

    pub fn submit(&self, task: TaskEnvelope) -> TaskResult {
        let lane = self.lane(task.queue);
        let bytes = encode_routed_message(&RoutedMessage {
            route_id: task.route_id.clone(),
            deadline_unix_ms: task.deadline_unix_ms,
            message: task.message.clone(),
        })
        .map_err(|error| TaskQueueError::Invalid(error.to_string()))?
        .len();
        let bytes = u32::try_from(bytes)
            .map_err(|_| TaskQueueError::Invalid("task exceeds byte budget range".into()))?;
        lane.push(task, bytes)
            .map_err(|(class, reason)| self.reject(class, reason))?;
        self.inner.stats.enqueued.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn task(
        &self,
        source: Participant,
        target: Participant,
        message: p4_protocol::Message,
    ) -> Result<TaskEnvelope, TaskQueueError> {
        let sequence = self.inner.sequence.fetch_add(1, Ordering::Relaxed);
        let task_id = format!("{}-{sequence}", self.inner.id_prefix);
        self.envelope(task_id.clone(), 0, task_id, None, source, target, message)
    }

    pub fn routed_task(
        &self,
        route_id: impl Into<String>,
        deadline_unix_ms: u64,
        source: Participant,
        target: Participant,
        message: p4_protocol::Message,
    ) -> Result<TaskEnvelope, TaskQueueError> {
        let sequence = self.inner.sequence.fetch_add(1, Ordering::Relaxed);
        self.envelope(
            route_id.into(),
            deadline_unix_ms,
            format!("{}-{sequence}", self.inner.id_prefix),
            None,
            source,
            target,
            message,
        )
    }

    pub fn response(
        &self,
        cause: &TaskEnvelope,
        message: p4_protocol::Message,
    ) -> Result<TaskEnvelope, TaskQueueError> {
        self.next_envelope(
            cause.route_id.clone(),
            cause.deadline_unix_ms,
            Some(cause.task_id.clone()),
            cause.target.clone(),
            cause.source.clone(),
            message,
        )
    }

    pub fn response_after(
        &self,
        route: &TaskEnvelope,
        causation_id: String,
        message: p4_protocol::Message,
    ) -> Result<TaskEnvelope, TaskQueueError> {
        self.next_envelope(
            route.route_id.clone(),
            route.deadline_unix_ms,
            Some(causation_id),
            route.target.clone(),
            route.source.clone(),
            message,
        )
    }

    pub fn follow_up(
        &self,
        cause: &TaskEnvelope,
        source: Participant,
        target: Participant,
        message: p4_protocol::Message,
    ) -> Result<TaskEnvelope, TaskQueueError> {
        self.next_envelope(
            cause.route_id.clone(),
            cause.deadline_unix_ms,
            Some(cause.task_id.clone()),
            source,
            target,
            message,
        )
    }

    pub fn stats(&self) -> TaskQueueStats {
        TaskQueueStats {
            enqueued: self.inner.stats.enqueued.load(Ordering::Relaxed),
            completed: self.inner.stats.completed.load(Ordering::Relaxed),
            rejected: self.inner.stats.rejected.load(Ordering::Relaxed),
            failed: self.inner.stats.failed.load(Ordering::Relaxed),
        }
    }

    fn next_envelope(
        &self,
        route_id: String,
        deadline_unix_ms: u64,
        causation_id: Option<String>,
        source: Participant,
        target: Participant,
        message: p4_protocol::Message,
    ) -> Result<TaskEnvelope, TaskQueueError> {
        let sequence = self.inner.sequence.fetch_add(1, Ordering::Relaxed);
        self.envelope(
            route_id,
            deadline_unix_ms,
            format!("{}-{sequence}", self.inner.id_prefix),
            causation_id,
            source,
            target,
            message,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn envelope(
        &self,
        route_id: String,
        deadline_unix_ms: u64,
        task_id: String,
        causation_id: Option<String>,
        source: Participant,
        target: Participant,
        message: p4_protocol::Message,
    ) -> Result<TaskEnvelope, TaskQueueError> {
        TaskEnvelope::new_routed(
            task_id,
            route_id,
            deadline_unix_ms,
            causation_id,
            source,
            target,
            message,
        )
        .map_err(|error| TaskQueueError::Invalid(error.to_string()))
    }

    fn lane(&self, class: QueueClass) -> &Lane {
        match class {
            QueueClass::Control => &self.inner.control,
            QueueClass::Prefill => &self.inner.prefill,
            QueueClass::Decode => &self.inner.decode,
            QueueClass::Response => &self.inner.response,
        }
    }

    fn reject(&self, class: QueueClass, reason: &str) -> TaskQueueError {
        self.inner.stats.rejected.fetch_add(1, Ordering::Relaxed);
        TaskQueueError::Full(class, reason.into())
    }

    fn finish(&self, result: &TaskResult) {
        if result.is_ok() {
            self.inner.stats.completed.fetch_add(1, Ordering::Relaxed);
        } else {
            self.inner.stats.failed.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl TaskContext {
    pub fn submit(&self, task: TaskEnvelope) -> TaskResult {
        self.queue.submit(task)
    }

    pub fn response(&self, cause: &TaskEnvelope, message: p4_protocol::Message) -> TaskResult {
        self.submit(self.queue.response(cause, message)?)
    }

    pub fn queue(&self) -> &TaskQueue {
        &self.queue
    }

    pub fn follow_up(
        &self,
        cause: &TaskEnvelope,
        source: Participant,
        target: Participant,
        message: p4_protocol::Message,
    ) -> TaskResult {
        self.submit(self.queue.follow_up(cause, source, target, message)?)
    }
}

impl TaskQueueConfig {
    pub fn for_parallelism(parallelism: usize) -> Self {
        let parallelism = parallelism.max(4);
        Self {
            control: LaneConfig {
                items: 4096,
                bytes: 16 << 20,
                workers: 2,
            },
            prefill: LaneConfig {
                items: 1024,
                bytes: 64 << 20,
                workers: (parallelism * 2 / 3).max(2),
            },
            decode: LaneConfig {
                items: 4096,
                bytes: 16 << 20,
                workers: (parallelism / 4).max(1),
            },
            response: LaneConfig {
                items: 4096,
                bytes: 16 << 20,
                workers: 2,
            },
        }
    }

    fn validate(&self) -> TaskResult {
        for lane in [&self.control, &self.prefill, &self.decode, &self.response] {
            if lane.items == 0 || lane.bytes == 0 || lane.workers == 0 {
                return Err(TaskQueueError::Invalid(
                    "queue items, bytes, and workers must be non-zero".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskQueueError {
    Full(QueueClass, String),
    Invalid(String),
}

impl Display for TaskQueueError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full(class, reason) => {
                write!(formatter, "{class:?} queue rejected task: {reason}")
            }
            Self::Invalid(reason) => formatter.write_str(reason),
        }
    }
}

impl std::error::Error for TaskQueueError {}

#[cfg(test)]
mod tests;

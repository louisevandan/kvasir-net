use super::delivery::DeliveryTracker;
use crate::domain::agent::{AgentProcessor, AsyncExecution};
use crate::foundation::transport::{ResponseSink, Result as P4Result};
use crate::infrastructure::peer_mux::PeerMuxPool;
use crate::{TaskQueue, TaskResult};
use p4_protocol::{Message, TaskEnvelope};
use std::sync::Arc;

pub(super) fn compatibility(
    processor: Arc<AgentProcessor>,
    queue: TaskQueue,
    task: TaskEnvelope,
    deliveries: Arc<DeliveryTracker>,
    runtime: tokio::runtime::Handle,
) {
    let request_id = task.correlation_id.clone();
    let mut responses = QueueResponseSink::new(queue, task, deliveries, runtime);
    if let Err(error) = processor.handle(responses.cause.message.clone(), &mut responses) {
        let _ = responses.emit(Message::Error {
            request_id,
            detail: error.to_string(),
        });
    }
}

pub(super) async fn execution(
    peers: Arc<PeerMuxPool>,
    queue: TaskQueue,
    cause: TaskEnvelope,
    prepared: AsyncExecution,
    deliveries: Arc<DeliveryTracker>,
) {
    let request_id = cause.correlation_id.clone();
    let endpoint = prepared
        .endpoint
        .as_deref()
        .expect("remote execution endpoint");
    let result = peers
        .execute(
            endpoint,
            cause.route_id.clone(),
            cause.deadline_unix_ms,
            prepared.execute.clone(),
        )
        .await;
    let mut causation_id = cause.task_id.clone();
    match result {
        Ok(mut responses) => {
            while let Some(message) = responses.recv().await {
                let terminal = message.is_terminal();
                if enqueue(&queue, &cause, &mut causation_id, message, &deliveries)
                    .await
                    .is_err()
                    || terminal
                {
                    break;
                }
            }
        }
        Err(error) => {
            let _ = enqueue(
                &queue,
                &cause,
                &mut causation_id,
                Message::Error {
                    request_id,
                    detail: error.to_string(),
                },
                &deliveries,
            )
            .await;
        }
    }
    drop(prepared._permit);
}

pub(super) fn local(
    queue: TaskQueue,
    cause: TaskEnvelope,
    prepared: AsyncExecution,
    deliveries: Arc<DeliveryTracker>,
    runtime: tokio::runtime::Handle,
) {
    let request_id = cause.correlation_id.clone();
    let mut responses = QueueResponseSink::new(queue, cause, deliveries, runtime);
    if let Err(error) = prepared
        .transport
        .dispatch(prepared.execute, &mut responses)
    {
        let _ = responses.emit(Message::Error {
            request_id,
            detail: error.to_string(),
        });
    }
    drop(prepared._permit);
}

struct QueueResponseSink {
    queue: TaskQueue,
    cause: TaskEnvelope,
    causation_id: String,
    deliveries: Arc<DeliveryTracker>,
    runtime: tokio::runtime::Handle,
}

impl QueueResponseSink {
    fn new(
        queue: TaskQueue,
        cause: TaskEnvelope,
        deliveries: Arc<DeliveryTracker>,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self {
            causation_id: cause.task_id.clone(),
            queue,
            cause,
            deliveries,
            runtime,
        }
    }
}

impl ResponseSink for QueueResponseSink {
    fn emit(&mut self, message: Message) -> P4Result<()> {
        let response =
            self.queue
                .response_after(&self.cause, self.causation_id.clone(), message)?;
        let task_id = response.task_id.clone();
        let delivered = self.deliveries.register(&task_id)?;
        if let Err(error) = self.queue.submit(response) {
            self.deliveries.cancel(&task_id);
            return Err(error.into());
        }
        self.causation_id = self.runtime.block_on(delivered.wait())?;
        Ok(())
    }
}

async fn enqueue(
    queue: &TaskQueue,
    cause: &TaskEnvelope,
    causation_id: &mut String,
    message: Message,
    deliveries: &DeliveryTracker,
) -> TaskResult {
    let response = queue.response_after(cause, causation_id.clone(), message)?;
    let task_id = response.task_id.clone();
    let delivered = deliveries.register(&task_id)?;
    if let Err(error) = queue.submit(response) {
        deliveries.cancel(&task_id);
        return Err(error);
    }
    *causation_id = delivered.wait().await?;
    Ok(())
}

//! One delivery acknowledgement per causal response chain.

use crate::TaskQueueError;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;

type DeliveryResult = Result<String, TaskQueueError>;

#[derive(Default)]
pub(super) struct DeliveryTracker {
    waiting: Mutex<HashMap<String, oneshot::Sender<DeliveryResult>>>,
}

pub(super) struct DeliveryWait {
    receiver: oneshot::Receiver<DeliveryResult>,
}

impl DeliveryTracker {
    pub(super) fn register(&self, task_id: &str) -> Result<DeliveryWait, TaskQueueError> {
        let (sender, receiver) = oneshot::channel();
        let mut waiting = self
            .waiting
            .lock()
            .map_err(|_| TaskQueueError::Invalid("delivery tracker lock poisoned".into()))?;
        if waiting.contains_key(task_id) {
            return Err(TaskQueueError::Invalid(format!(
                "response task {task_id} already has a delivery waiter"
            )));
        }
        waiting.insert(task_id.into(), sender);
        Ok(DeliveryWait { receiver })
    }

    pub(super) fn transfer(&self, current: &str, successor: &str) -> Result<(), TaskQueueError> {
        let mut waiting = self
            .waiting
            .lock()
            .map_err(|_| TaskQueueError::Invalid("delivery tracker lock poisoned".into()))?;
        let Some(sender) = waiting.remove(current) else {
            return Ok(());
        };
        if waiting.contains_key(successor) {
            waiting.insert(current.into(), sender);
            return Err(TaskQueueError::Invalid(format!(
                "successor task {successor} already has a delivery waiter"
            )));
        }
        waiting.insert(successor.into(), sender);
        Ok(())
    }

    pub(super) fn complete(&self, task_id: &str) {
        self.resolve(task_id, Ok(task_id.into()));
    }

    pub(super) fn fail(&self, task_id: &str, error: TaskQueueError) {
        self.resolve(task_id, Err(error));
    }

    pub(super) fn cancel(&self, task_id: &str) {
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.remove(task_id);
        }
    }

    fn resolve(&self, task_id: &str, result: DeliveryResult) {
        let sender = self
            .waiting
            .lock()
            .ok()
            .and_then(|mut waiting| waiting.remove(task_id));
        if let Some(sender) = sender {
            let _ = sender.send(result);
        }
    }
}

impl DeliveryWait {
    pub(super) async fn wait(self) -> DeliveryResult {
        self.receiver.await.map_err(|_| {
            TaskQueueError::Invalid("response delivery acknowledgement was dropped".into())
        })?
    }
}

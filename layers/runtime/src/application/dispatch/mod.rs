//! Agent-specific handlers layered over the generic P4 task queues.
//! See `apps/p4/docs/task-runtime.md#current-upper-layer-boundary`.

use crate::domain::agent::{AgentProcessor, AsyncExecution};
use crate::infrastructure::peer_mux::PeerMuxPool;
use crate::{TaskContext, TaskHandler, TaskQueueError, TaskResult};
use p4_protocol::{Message, Participant, ParticipantRole, RoutedMessage, TaskEnvelope, TaskKind};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::mpsc;

mod delivery;
mod relay;

use delivery::DeliveryTracker;

type Recipient = mpsc::Sender<RoutedMessage>;

#[derive(Clone)]
struct RecipientRoute {
    participant: Participant,
    sender: Recipient,
}

struct ActiveRelay {
    endpoint: String,
    request_id: String,
    abort: tokio::task::AbortHandle,
}

pub(crate) struct AgentTaskHandler {
    agent_id: String,
    processor: Arc<AgentProcessor>,
    peers: Arc<PeerMuxPool>,
    recipients: RwLock<HashMap<String, RecipientRoute>>,
    ingress_routes: RwLock<HashMap<String, Participant>>,
    prepared: Mutex<HashMap<String, AsyncExecution>>,
    active: Arc<Mutex<HashMap<String, ActiveRelay>>>,
    deliveries: Arc<DeliveryTracker>,
}

impl AgentTaskHandler {
    pub(crate) fn new(processor: Arc<AgentProcessor>, peers: Arc<PeerMuxPool>) -> Self {
        Self {
            agent_id: processor.id().into(),
            processor,
            peers,
            recipients: RwLock::new(HashMap::new()),
            ingress_routes: RwLock::new(HashMap::new()),
            prepared: Mutex::new(HashMap::new()),
            active: Arc::new(Mutex::new(HashMap::new())),
            deliveries: Arc::new(DeliveryTracker::default()),
        }
    }

    pub(crate) fn register(
        &self,
        route_id: String,
        participant: Participant,
        recipient: Recipient,
    ) -> TaskResult {
        let mut recipients = self
            .recipients
            .write()
            .map_err(|_| invalid("recipient registry lock poisoned"))?;
        if let Some(existing) = recipients.get(&route_id) {
            if existing.participant != participant {
                return Err(invalid("route_id is already owned by another participant"));
            }
            return Ok(());
        }
        recipients.insert(
            route_id,
            RecipientRoute {
                participant,
                sender: recipient,
            },
        );
        Ok(())
    }

    pub(crate) fn unregister(&self, route_id: &str) {
        if let Ok(mut recipients) = self.recipients.write() {
            recipients.remove(route_id);
        }
        self.cleanup_route(route_id);
    }

    fn cleanup_route(&self, route_id: &str) {
        if let Ok(mut routes) = self.ingress_routes.write() {
            routes.remove(route_id);
        }
        if let Ok(mut prepared) = self.prepared.lock() {
            prepared.remove(route_id);
        }
        if let Ok(mut active) = self.active.lock()
            && let Some(active) = active.remove(route_id)
        {
            active.abort.abort();
        }
    }

    fn request(&self, task: TaskEnvelope, context: &TaskContext) -> TaskResult {
        match task.message {
            Message::IngressSubmit { .. } => self.ingress(task, context),
            Message::Execute(_) => self.execute(task, context),
            Message::Cancel { .. } => self.cancel(task, context),
            _ => self.compatibility(task, context),
        }
    }

    fn ingress(&self, task: TaskEnvelope, context: &TaskContext) -> TaskResult {
        let prepared = match self.processor.prepare_async_ingress(task.message.clone()) {
            Ok(value) => value,
            Err(error) => return context.response(&task, error),
        };
        self.ingress_routes
            .write()
            .map_err(|_| invalid("ingress route lock poisoned"))?
            .insert(task.route_id.clone(), task.source.clone());
        self.prepared
            .lock()
            .map_err(|_| invalid("prepared execution lock poisoned"))?
            .insert(task.route_id.clone(), prepared.execution);
        context.response(&task, prepared.accepted)
    }

    fn execute(&self, task: TaskEnvelope, context: &TaskContext) -> TaskResult {
        let prepared = self
            .prepared
            .lock()
            .map_err(|_| invalid("prepared execution lock poisoned"))?
            .remove(&task.route_id)
            .map(Ok)
            .unwrap_or_else(|| self.processor.prepare_async_execution(task.message.clone()));
        let prepared = match prepared {
            Ok(value) => value,
            Err(error) => return context.response(&task, error),
        };
        let queue = context.queue().clone();
        if prepared.endpoint.is_some() {
            let peers = Arc::clone(&self.peers);
            let processor = Arc::clone(&self.processor);
            let deliveries = Arc::clone(&self.deliveries);
            let route_id = task.route_id.clone();
            let endpoint = prepared
                .endpoint
                .clone()
                .expect("remote execution endpoint");
            let request_id = task.correlation_id.clone();
            let active = Arc::clone(&self.active);
            let relay_route = route_id.clone();
            let (start, started) = tokio::sync::oneshot::channel();
            let handle = tokio::spawn(async move {
                if started.await.is_err() {
                    return;
                }
                relay::execution(processor, peers, queue, task, prepared, deliveries).await;
                if let Ok(mut active) = active.lock() {
                    active.remove(&relay_route);
                }
            });
            let mut active_routes = self
                .active
                .lock()
                .map_err(|_| invalid("active relay lock poisoned"))?;
            if active_routes.contains_key(&route_id) {
                handle.abort();
                return Err(invalid("route_id already has an active execution"));
            }
            active_routes.insert(
                route_id,
                ActiveRelay {
                    endpoint,
                    request_id,
                    abort: handle.abort_handle(),
                },
            );
            drop(active_routes);
            let _ = start.send(());
        } else {
            let processor = Arc::clone(&self.processor);
            let deliveries = Arc::clone(&self.deliveries);
            let runtime = tokio::runtime::Handle::current();
            tokio::spawn(async move {
                relay::local(processor, queue, task, prepared, deliveries, runtime).await
            });
        }
        Ok(())
    }

    fn cancel(&self, task: TaskEnvelope, context: &TaskContext) -> TaskResult {
        let reason = match &task.message {
            Message::Cancel { reason, .. } => reason.clone(),
            _ => unreachable!(),
        };
        let active = self
            .active
            .lock()
            .map_err(|_| invalid("active relay lock poisoned"))?
            .remove(&task.route_id);
        if let Some(active) = active {
            active.abort.abort();
            self.peers.cancel(
                &active.endpoint,
                &task.route_id,
                &active.request_id,
                &reason,
            );
        }
        self.prepared
            .lock()
            .map_err(|_| invalid("prepared execution lock poisoned"))?
            .remove(&task.route_id);
        context.response(
            &task,
            Message::Error {
                request_id: task.correlation_id.clone(),
                detail: format!("cancelled: {reason}"),
            },
        )
    }

    fn compatibility(&self, task: TaskEnvelope, context: &TaskContext) -> TaskResult {
        let processor = Arc::clone(&self.processor);
        let queue = context.queue().clone();
        let deliveries = Arc::clone(&self.deliveries);
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            relay::compatibility(processor, queue, task, deliveries, runtime)
        });
        Ok(())
    }

    fn response(&self, task: TaskEnvelope, context: &TaskContext) -> TaskResult {
        let recipient = self
            .recipients
            .read()
            .map_err(|_| invalid("recipient registry lock poisoned"))?
            .get(&task.route_id)
            .filter(|route| route.participant == task.target)
            .map(|route| route.sender.clone());
        if let Some(recipient) = recipient {
            recipient
                .try_send(RoutedMessage {
                    route_id: task.route_id.clone(),
                    deadline_unix_ms: task.deadline_unix_ms,
                    message: task.message.clone(),
                })
                .map_err(|_| {
                    TaskQueueError::Invalid("recipient response queue is full or closed".into())
                })?;
            if matches!(task.message, Message::IngressAccepted { .. }) {
                self.start_ingress_execution(&task, context)?;
            }
            if task.message.is_terminal() {
                self.unregister(&task.route_id);
            }
            self.deliveries.complete(&task.task_id);
            return Ok(());
        }
        if task.target.role == ParticipantRole::Controller {
            let external = self
                .ingress_routes
                .read()
                .map_err(|_| invalid("ingress route lock poisoned"))?
                .get(&task.route_id)
                .cloned();
            if let Some(external) = external {
                let terminal = task.message.is_terminal();
                let follow_up = context.queue().follow_up(
                    &task,
                    task.target.clone(),
                    external,
                    task.message.clone(),
                )?;
                self.deliveries
                    .transfer(&task.task_id, &follow_up.task_id)?;
                let follow_up_id = follow_up.task_id.clone();
                if let Err(error) = context.submit(follow_up) {
                    self.deliveries.fail(&follow_up_id, error.clone());
                    return Err(error);
                }
                if terminal {
                    self.ingress_routes
                        .write()
                        .map_err(|_| invalid("ingress route lock poisoned"))?
                        .remove(&task.route_id);
                }
                return Ok(());
            }
        }
        Err(TaskQueueError::Invalid(format!(
            "no local recipient for task {} target {:?}",
            task.task_id, task.target
        )))
    }

    fn start_ingress_execution(
        &self,
        accepted: &TaskEnvelope,
        context: &TaskContext,
    ) -> TaskResult {
        let execute = self
            .prepared
            .lock()
            .map_err(|_| invalid("prepared execution lock poisoned"))?
            .get(&accepted.route_id)
            .map(|value| value.execute.clone())
            .ok_or_else(|| invalid("accepted ingress has no prepared execution"))?;
        let node_id = match &execute {
            Message::Execute(request) => request.node_id.clone(),
            _ => unreachable!(),
        };
        context.follow_up(
            accepted,
            accepted.source.clone(),
            self.local(ParticipantRole::Node, node_id),
            execute,
        )
    }

    fn local(&self, role: ParticipantRole, instance_id: String) -> Participant {
        Participant {
            agent_id: self.agent_id.clone(),
            role,
            instance_id,
        }
    }
}

impl TaskHandler for AgentTaskHandler {
    fn handle(&self, task: TaskEnvelope, context: &TaskContext) -> TaskResult {
        let task_id = task.task_id.clone();
        let kind = task.kind;
        let result = if kind == TaskKind::Request {
            self.request(task, context)
        } else {
            self.response(task, context)
        };
        if let Err(error) = &result
            && kind == TaskKind::Response
        {
            self.deliveries.fail(&task_id, error.clone());
        }
        result
    }
}

fn invalid(detail: &str) -> TaskQueueError {
    TaskQueueError::Invalid(detail.into())
}

#[cfg(test)]
mod tests;

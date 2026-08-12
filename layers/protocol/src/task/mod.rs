//! Self-describing Agent task envelope used by every local and remote route.
//! See `apps/p4/docs/task-runtime.md#envelope`.

use crate::{Message, QueueClass};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParticipantRole {
    External,
    Controller,
    Node,
    Agent,
    Adapter,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Participant {
    pub agent_id: String,
    pub role: ParticipantRole,
    pub instance_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskDirection {
    ExternalController,
    ControllerNode,
    NodeNode,
    NodeController,
    /// Bootstrap and adapter plumbing; not a fifth public P4 communication path.
    AgentInternal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    Request,
    Response,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskEnvelope {
    pub task_id: String,
    pub route_id: String,
    pub deadline_unix_ms: u64,
    pub correlation_id: String,
    pub causation_id: Option<String>,
    pub source: Participant,
    pub target: Participant,
    pub direction: TaskDirection,
    pub kind: TaskKind,
    pub queue: QueueClass,
    pub message: Message,
}

impl TaskEnvelope {
    pub fn new(
        task_id: impl Into<String>,
        causation_id: Option<String>,
        source: Participant,
        target: Participant,
        message: Message,
    ) -> Result<Self, TaskError> {
        let task_id = task_id.into();
        Self::new_routed(
            task_id.clone(),
            task_id,
            0,
            causation_id,
            source,
            target,
            message,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_routed(
        task_id: impl Into<String>,
        route_id: impl Into<String>,
        deadline_unix_ms: u64,
        causation_id: Option<String>,
        source: Participant,
        target: Participant,
        message: Message,
    ) -> Result<Self, TaskError> {
        let task_id = task_id.into();
        let route_id = route_id.into();
        if task_id.is_empty()
            || route_id.is_empty()
            || source.instance_id.is_empty()
            || target.instance_id.is_empty()
        {
            return Err(TaskError(
                "task, route, and participant IDs must be non-empty".into(),
            ));
        }
        let direction = direction(&source, &target)?;
        if !message.allows_direction(direction) {
            return Err(TaskError(format!(
                "message {:?} is invalid for direction {direction:?}",
                message.kind()
            )));
        }
        let kind = if message.class() == crate::MessageClass::Request {
            TaskKind::Request
        } else {
            TaskKind::Response
        };
        Ok(Self {
            task_id,
            route_id,
            deadline_unix_ms,
            correlation_id: message.correlation_id().into(),
            causation_id,
            source,
            target,
            direction,
            kind,
            queue: message.queue_class(),
            message,
        })
    }

    pub fn is_local_bypass(&self) -> bool {
        !self.source.agent_id.is_empty() && self.source.agent_id == self.target.agent_id
    }
}

fn direction(source: &Participant, target: &Participant) -> Result<TaskDirection, TaskError> {
    use ParticipantRole as R;
    let value = match (source.role, target.role) {
        (R::External, R::Controller) | (R::Controller, R::External) => {
            TaskDirection::ExternalController
        }
        (R::Controller, R::Node) => TaskDirection::ControllerNode,
        (R::Node, R::Node) => TaskDirection::NodeNode,
        (R::Node, R::Controller) => TaskDirection::NodeController,
        (R::Agent, R::Adapter) | (R::Adapter, R::Agent) => TaskDirection::AgentInternal,
        roles => {
            return Err(TaskError(format!(
                "unsupported P4 participant route {roles:?}"
            )));
        }
    };
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskError(pub String);

impl std::fmt::Display for TaskError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TaskError {}

#[cfg(test)]
mod tests;

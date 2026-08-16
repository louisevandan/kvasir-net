//! Exhaustive semantic catalog for every P4 wire message.
//! See `apps/p4/docs/task-runtime.md#complete-message-catalog`.

use crate::{Message, Phase};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageKind {
    IngressSubmit,
    IngressAccepted,
    InventoryQuery,
    HardwareReport,
    AdapterRegister,
    AdapterRegistered,
    NodeCreate,
    NodeCreated,
    ModelLoad,
    ModelBound,
    ModelUnload,
    ModelUnbound,
    Execute,
    Token,
    Done,
    Cancel,
    HealthCheck,
    Health,
    LoadProgress,
    DraftReport,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageClass {
    Request,
    Acknowledgement,
    Progress,
    Event,
    Terminal,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueClass {
    Control,
    Prefill,
    Decode,
    Response,
}

impl Message {
    pub fn kind(&self) -> MessageKind {
        match self {
            Self::IngressSubmit { .. } => MessageKind::IngressSubmit,
            Self::IngressAccepted { .. } => MessageKind::IngressAccepted,
            Self::InventoryQuery { .. } => MessageKind::InventoryQuery,
            Self::HardwareReport { .. } => MessageKind::HardwareReport,
            Self::AdapterRegister { .. } => MessageKind::AdapterRegister,
            Self::AdapterRegistered { .. } => MessageKind::AdapterRegistered,
            Self::NodeCreate { .. } => MessageKind::NodeCreate,
            Self::NodeCreated { .. } => MessageKind::NodeCreated,
            Self::ModelLoad { .. } => MessageKind::ModelLoad,
            Self::ModelBound { .. } => MessageKind::ModelBound,
            Self::ModelUnload { .. } => MessageKind::ModelUnload,
            Self::ModelUnbound { .. } => MessageKind::ModelUnbound,
            Self::Execute(_) => MessageKind::Execute,
            Self::Token(_) => MessageKind::Token,
            Self::Done(_) => MessageKind::Done,
            Self::Cancel { .. } => MessageKind::Cancel,
            Self::HealthCheck { .. } => MessageKind::HealthCheck,
            Self::Health { .. } => MessageKind::Health,
            Self::LoadProgress { .. } => MessageKind::LoadProgress,
            Self::DraftReport { .. } => MessageKind::DraftReport,
            Self::Error { .. } => MessageKind::Error,
        }
    }

    pub fn correlation_id(&self) -> &str {
        match self {
            Self::IngressSubmit { request_id, .. }
            | Self::IngressAccepted { request_id, .. }
            | Self::InventoryQuery { request_id, .. }
            | Self::Cancel { request_id, .. }
            | Self::HealthCheck { request_id, .. }
            | Self::Health { request_id, .. }
            | Self::Error { request_id, .. } => request_id,
            Self::HardwareReport { report_id, .. } => report_id,
            Self::AdapterRegister { adapter_id, .. }
            | Self::AdapterRegistered { adapter_id, .. } => adapter_id,
            Self::NodeCreate { operation_id, .. }
            | Self::NodeCreated { operation_id, .. }
            | Self::ModelLoad { operation_id, .. }
            | Self::ModelBound { operation_id, .. }
            | Self::ModelUnload { operation_id, .. }
            | Self::ModelUnbound { operation_id, .. }
            | Self::LoadProgress { operation_id, .. }
            | Self::DraftReport { operation_id, .. } => operation_id,
            Self::Execute(value) => &value.request_id,
            Self::Token(value) => &value.request_id,
            Self::Done(value) => &value.request_id,
        }
    }

    pub fn class(&self) -> MessageClass {
        match self {
            Self::IngressSubmit { .. }
            | Self::InventoryQuery { .. }
            | Self::AdapterRegister { .. }
            | Self::NodeCreate { .. }
            | Self::ModelLoad { .. }
            | Self::ModelUnload { .. }
            | Self::Execute(_)
            | Self::Cancel { .. }
            | Self::HealthCheck { .. } => MessageClass::Request,
            Self::IngressAccepted { .. } => MessageClass::Acknowledgement,
            Self::LoadProgress { .. } | Self::DraftReport { .. } => MessageClass::Progress,
            Self::Token(_) => MessageClass::Event,
            Self::HardwareReport { .. }
            | Self::AdapterRegistered { .. }
            | Self::NodeCreated { .. }
            | Self::ModelBound { .. }
            | Self::ModelUnbound { .. }
            | Self::Done(_)
            | Self::Health { .. } => MessageClass::Terminal,
            Self::Error { .. } => MessageClass::Error,
        }
    }

    pub fn queue_class(&self) -> QueueClass {
        match self {
            Self::Execute(value) if value.phase == Phase::Prefill => QueueClass::Prefill,
            Self::Execute(_) => QueueClass::Decode,
            value if value.class() != MessageClass::Request => QueueClass::Response,
            _ => QueueClass::Control,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.class(), MessageClass::Terminal | MessageClass::Error)
    }
}

#[cfg(test)]
mod tests;

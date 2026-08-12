//! P4 relay runtime layers. See `apps/p4/docs/architecture.md`.

mod application;
mod domain;
mod foundation;
mod infrastructure;

pub use application::agent_host::serve_agent;
pub use application::routing::{
    AgentOptions, ControllerProcessor, NodeProcessor, parse_agent_options, parse_bindings,
};
pub use domain::agent::AgentProcessor;
pub use foundation::task_queue::{
    LaneConfig, TaskContext, TaskHandler, TaskQueue, TaskQueueConfig, TaskQueueError,
    TaskQueueStats, TaskResult,
};
pub use foundation::transport::{
    P4Handler, P4Transport, ResponseCollector, ResponseSink, Result, SharedHandler,
    SharedTransport, dispatch_tcp, in_memory, serve, tcp,
};

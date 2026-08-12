//! Controller and Node routing plus startup parsing.

mod processor;
mod startup;

pub use processor::{ControllerProcessor, NodeProcessor};
pub use startup::{AgentOptions, parse_agent_options, parse_bindings};

#[cfg(test)]
mod tests;

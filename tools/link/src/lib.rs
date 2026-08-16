//! A link that carries frames badly, on purpose.
//!
//! Two pieces, split because they change for different reasons: what a bad
//! link *is* (numbers, pure, testable on its own) and how those numbers are
//! applied to a socket.
//!
//! A test uses this as a library and stands a relay between two agents in the
//! same process; the fleet uses the `p4-link` binary between two machines.
//! Both are the same code, so a scenario proved in a test is the scenario the
//! fleet runs.

pub mod impairment;
pub mod relay;

pub use impairment::Impairment;

//! External retained adapter. Model execution and tensor semantics stay in Python.
mod construction;
mod ipc;
mod lifecycle;
mod process;
mod retained;
pub use construction::HfNodeAdapter;
pub use ipc::{COMMAND, RESULT};

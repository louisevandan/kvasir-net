//! Ownership and readiness state for one concrete llama stage server.
//!
//! `ServerControl` remains injectable so the lifecycle state machine can be
//! tested without a model or GPU. `ProcessServerControl` is the production
//! implementation: it owns the child, its still-open plan pipe, and the local
//! TCP session as one object.

use crate::{Frame, FrameIoError, Operation, PROTOCOL_REVISION, ProtocolLimits};
#[cfg(test)]
use crate::{KvPayload, KvResult, SequencePayload};
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

mod core;
mod server_process;
#[cfg(test)]
mod hello_wire_tests;
#[cfg(test)]
mod tests;

pub use core::{
    ProcessError, ProcessServerControl, ProcessState, ReadyInfo, ServerControl, ServerLaunch,
};
pub use server_process::ServerProcess;

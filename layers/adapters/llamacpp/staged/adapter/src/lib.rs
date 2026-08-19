//! Protocol primitives for the private adapter-to-stage-server link.
//!
//! P4 carries the outer hop. This crate owns only the binary framing needed
//! between one concrete adapter and the one server process it started.

pub mod lifecycle;
pub mod process;
mod protocol;
mod telemetry;

pub use protocol::PROTOCOL_REVISION;
pub use protocol::{
    Descriptor, Frame, FrameError, FrameHeader, FrameIoError, HopPayload, HopPhase, KvPayload,
    KvReceipt, KvReceiptState, KvResult, Operation, OutcomeMetadata, ProtocolLimits,
    SequencePayload, WireType,
};

use p4_adapter::{Adapter, Distribution, Event, EventSink, Outcome, Work};
use std::collections::HashMap;
use std::env;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use lifecycle::{LlamaLifecycle, LoadState};
include!("config.inc.rs");
include!("adapter/state.inc.rs");
include!("adapter/cache_transactions.inc.rs");
include!("adapter/cache_direct.inc.rs");
include!("adapter/hop.inc.rs");
include!("adapter_trait.inc.rs");
include!("tests.inc.rs");

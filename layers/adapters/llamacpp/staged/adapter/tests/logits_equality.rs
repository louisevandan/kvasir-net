use p4_llamacpp_staged_adapter::process::{ProcessServerControl, ServerControl, ServerLaunch};
use p4_llamacpp_staged_adapter::{
    Frame, HopPayload, HopPhase, Operation, OutcomeMetadata, PROTOCOL_REVISION, ProtocolLimits,
    SequencePayload, WireType,
};
use std::env;
use std::ffi::OsString;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SERVER_ENV: &str = "P4_STAGED_LLAMA_SERVER_BINARY";
const MODEL_ENV: &str = "P4_STAGED_LLAMA_MODEL";
const LAYER_END_ENV: &str = "P4_STAGED_LLAMA_LAYER_END";
const SPLIT_LAYER_ENV: &str = "P4_STAGED_LLAMA_SPLIT_LAYER";
const PROMPT: &str = "Reply with one short word: hello";
const ABS_TOLERANCE: f32 = 1.0e-4;
const REL_TOLERANCE: f32 = 1.0e-4;

include!("logits_equality/core.inc.rs");
include!("logits_equality/compare.inc.rs");
include!("logits_equality/metrics.inc.rs");
include!("logits_equality/launch.inc.rs");

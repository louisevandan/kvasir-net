use p4_llamacpp_staged_adapter::process::{ProcessServerControl, ServerControl, ServerLaunch};
use p4_llamacpp_staged_adapter::{
    Frame, HopPayload, HopPhase, KvPayload, KvResult, Operation, PROTOCOL_REVISION, ProtocolLimits,
    SequencePayload,
};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SERVER_ENV: &str = "P4_STAGED_LLAMA_SERVER_BINARY";
const MODEL_ENV: &str = "P4_STAGED_LLAMA_MODEL";
const BOUNDARIES_ENV: &str = "P4_STAGED_LLAMA_KV_BOUNDARIES";
const PROMPT: &str = "Reply with one short word: hello";

include!("multi_stage_kv_e2e/core.inc.rs");
include!("multi_stage_kv_e2e/helpers.inc.rs");

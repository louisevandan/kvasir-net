//! What a load's plan means to this adapter.
//!
//! The plan is opaque everywhere above the boundary and read exactly here,
//! which is the point of it being a string: adding a key is a change to one
//! backend and to nothing else.
//!
//! ## A distributed load
//!
//! One model can be larger than one card. llama.cpp splits it across devices
//! itself, over its own RPC backend and without any patch to it: a worker
//! process holds a share on its device, and the process serving completions
//! holds the rest on its own.
//!
//! P4 gives each share a node, so a placement is stated rather than implied:
//!
//! ```json
//! { "role": "worker", "device": "CUDA1", "vram_gb": 23,
//!   "endpoint": "127.0.0.1:50052" }
//!
//! { "role": "front",  "device": "CUDA0", "vram_gb": 11,
//!   "endpoint": "127.0.0.1:18090", "workers": ["127.0.0.1:50052"] }
//! ```
//!
//! Both must bind before the deployment is executable, which is what makes a
//! distributed load a transaction rather than two independent ones. A worker's
//! load is the claim; the front's is what proves it, because llama.cpp will not
//! start against an RPC device it cannot reach and will not answer across one
//! that died. Probing a worker directly does not work and is not tried: an RPC
//! worker already serving a front refuses further connections, and that refusal
//! is indistinguishable from an empty port.

use crate::endpoint::Endpoint;
use serde_json::Value;
use std::time::Duration;

/// Which share of a deployment this node holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Serves completions. Holds its own share and reaches the others.
    Front,
    /// Holds a share on its device and serves nothing. Its load is a claim on
    /// that device, and its binding is what says the claim was met.
    Worker,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub role: Role,
    pub endpoint: Endpoint,
    /// What the backend calls the model. Most servers holding one model accept
    /// anything here, so it defaults rather than being required.
    pub model: String,
    /// Whether the plan actually said so, as against the default standing in.
    ///
    /// Kept apart from the name because a backend that checks the name cannot
    /// tell a default from a choice, and sending "default" to one that does
    /// check is a request refused for a model that does not exist.
    pub names_the_model: bool,
    /// How long a hop waits for its token before giving up on the backend.
    pub patience: Duration,
    /// The device this node's share sits on, in the backend's own naming.
    /// Reported rather than interpreted.
    pub device: Option<String>,
    /// What this node claims of that device. Declared, never derived — the
    /// same rule the ceiling follows.
    pub vram_gb: Option<u64>,
    /// The shares held elsewhere. Recorded and reported, not probed — see the
    /// module note on why a busy RPC worker cannot be told from an absent one.
    pub workers: Vec<Endpoint>,
    /// What to start, and what to give it.
    ///
    /// Absent means a server is already there and this node attaches to it,
    /// which is how every plan written before this one still works.
    pub start: Option<Start>,
}

/// What a node needs in order to bring its own backend up.
///
/// Placement intent, not a command line. The plan says which device, how much
/// of it, how many sequences and how long a context; the adapter knows what
/// those become for the server it is starting, because that is the one thing a
/// concrete adapter is for. A plan carrying `-ts` would be a plan that had
/// learned llama.cpp, and then vLLM could not read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Start {
    /// The server to run.
    pub binary: String,
    /// The weights it should hold.
    pub weights: String,
    /// Total context across every sequence, and how many sequences at once.
    pub context: u32,
    pub slots: u32,
    /// How wide a batch the backend may build, and its micro-batch. Declared
    /// because a batch too small to hold one prompt makes a server admit
    /// prefills one at a time however fast work arrives.
    pub batch: u32,
    pub ubatch: u32,
}

impl Start {
    /// Reads the `start` object, if the plan has one.
    ///
    /// Every field is required once the object is present. A default context or
    /// a guessed slot count would be this adapter deciding a placement, which
    /// is the caller's to decide and the whole reason the plan exists.
    fn parse(value: Option<&Value>) -> Result<Option<Self>, String> {
        let Some(value) = value else {
            return Ok(None);
        };
        let text = |key: &str| -> Result<String, String> {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("start has no {key}"))
        };
        let count = |key: &str| -> Result<u32, String> {
            value
                .get(key)
                .and_then(Value::as_u64)
                .and_then(|found| u32::try_from(found).ok())
                .filter(|found| *found > 0)
                .ok_or_else(|| format!("start has no usable {key}"))
        };
        Ok(Some(Self {
            binary: text("binary")?,
            weights: text("weights")?,
            context: count("context")?,
            slots: count("slots")?,
            batch: count("batch")?,
            ubatch: count("ubatch")?,
        }))
    }
}

impl Plan {
    pub fn parse(plan: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(plan).map_err(|error| format!("plan is not json: {error}"))?;
        let endpoint = value
            .get("endpoint")
            .and_then(Value::as_str)
            .ok_or("plan has no endpoint")?;
        let endpoint = Endpoint::parse(endpoint)
            .ok_or_else(|| format!("endpoint {endpoint} is not host:port"))?;
        let role = match value.get("role").and_then(Value::as_str) {
            None | Some("front") => Role::Front,
            Some("worker") => Role::Worker,
            Some(other) => return Err(format!("unknown role {other}")),
        };
        let mut workers = Vec::new();
        for declared in value
            .get("workers")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let text = declared
                .as_str()
                .ok_or("a worker must be a host:port string")?;
            workers.push(
                Endpoint::parse(text).ok_or_else(|| format!("worker {text} is not host:port"))?,
            );
        }
        if role == Role::Worker && !workers.is_empty() {
            // A worker holding a share cannot also be reaching for others; the
            // shape would be a chain, and this backend spreads internally.
            return Err("a worker plan cannot declare workers of its own".into());
        }
        let patience = value
            .get("patience_ms")
            .and_then(Value::as_u64)
            .map(Duration::from_millis)
            .unwrap_or(endpoint.idle);
        // The socket is told the same thing, because it is the same wait. The
        // two were separate and the plan reached only one of them: an operator
        // asking for fifteen minutes of patience got fifteen minutes on the
        // channel and two on the socket underneath it, so a backend that went
        // quiet for two minutes — a crowded server working other slots —
        // killed the stream from below while the adapter was still waiting
        // patiently above. Sequences died around their ninetieth token with a
        // timeout nobody had asked for.
        let mut endpoint = endpoint;
        endpoint.idle = patience;
        let named = value.get("model").and_then(Value::as_str);
        Ok(Self {
            role,
            endpoint,
            model: named.unwrap_or("default").to_owned(),
            names_the_model: named.is_some(),
            patience,
            device: value
                .get("device")
                .and_then(Value::as_str)
                .map(str::to_owned),
            vram_gb: value.get("vram_gb").and_then(Value::as_u64),
            workers,
            start: Start::parse(value.get("start"))?,
        })
    }

    /// How this share describes itself when the load reports what it reserved.
    pub fn share(&self) -> String {
        match (&self.device, self.vram_gb) {
            (Some(device), Some(gb)) => format!("{device}.declared_{gb}GiB"),
            (Some(device), None) => format!("{device}.declared"),
            (None, Some(gb)) => format!("declared_{gb}GiB"),
            (None, None) => "declared".into(),
        }
    }
}

#[cfg(test)]
mod tests;

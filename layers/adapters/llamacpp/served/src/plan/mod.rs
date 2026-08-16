//! What a load's plan means to this adapter.
//!
//! The plan is opaque everywhere above the boundary and read exactly here,
//! which is the point of it being a string: adding a key is a change to one
//! backend and to nothing else.

use crate::endpoint::Endpoint;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub endpoint: Endpoint,
    /// What the backend calls the model. Servers holding one model accept
    /// anything here, so it defaults rather than being required.
    pub model: String,
    /// How long a hop waits for its token before giving up on the backend.
    pub patience: Duration,
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
        let patience = value
            .get("patience_ms")
            .and_then(Value::as_u64)
            .map(Duration::from_millis)
            .unwrap_or(endpoint.idle);
        Ok(Self {
            endpoint,
            model: value
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("default")
                .to_owned(),
            patience,
        })
    }
}

#[cfg(test)]
mod tests;

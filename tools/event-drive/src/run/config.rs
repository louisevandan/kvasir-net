use p4_llamacpp_staged_adapter::v2::NodeAddress;
use p4_protocol::Address;
use p4_protocol::event::Endpoint;
use serde::Deserialize;
use std::str::FromStr;

#[derive(Clone, Debug, Deserialize)]
pub struct RunConfig {
    pub ingress_agent: String,
    pub channel: String,
    pub connection_generation: u64,
    pub load_generation: u64,
    pub session_id: String,
    pub request_id: String,
    pub nodes: Vec<NodeConfig>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub prompts: Vec<String>,
    pub max_tokens: u32,
    #[serde(default = "default_waves")]
    pub waves: Vec<ArrivalWave>,
    #[serde(default)]
    pub options: String,
    #[serde(default)]
    pub pre_inference_hold_ms: u64,
    #[serde(default)]
    pub acceptance: AcceptanceConfig,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AcceptanceConfig {
    #[serde(default = "default_minimum_generated_tokens")]
    pub minimum_generated_tokens: usize,
    #[serde(default)]
    pub expected_prefill_rows: Option<usize>,
    #[serde(default)]
    pub allowed_stop_reasons: Vec<String>,
    #[serde(default)]
    pub responses: Vec<ResponseExpectation>,
}

impl Default for AcceptanceConfig {
    fn default() -> Self {
        Self {
            minimum_generated_tokens: default_minimum_generated_tokens(),
            expected_prefill_rows: None,
            allowed_stop_reasons: Vec::new(),
            responses: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ResponseExpectation {
    #[serde(default)]
    pub minimum_generated_tokens: Option<usize>,
    #[serde(default)]
    pub minimum_response_chars: Option<usize>,
    #[serde(default)]
    pub exact_response: Option<String>,
    #[serde(default)]
    pub required_substrings: Vec<String>,
    #[serde(default)]
    pub forbidden_substrings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ArrivalWave {
    pub after_ms: u64,
    pub count: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NodeConfig {
    pub agent: String,
    pub node: String,
    pub generation: u64,
    pub binary: String,
    pub endpoint: String,
    pub plan: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub environment: Vec<(String, String)>,
    pub n_batch: usize,
    pub n_ubatch: usize,
    pub context_size: usize,
    pub total_context_size: usize,
    pub sequence_capacity: u32,
}

fn default_waves() -> Vec<ArrivalWave> {
    vec![ArrivalWave {
        after_ms: 0,
        count: 1,
    }]
}

fn default_timeout() -> u64 {
    600_000
}

fn default_minimum_generated_tokens() -> usize {
    1
}

pub(super) fn validate(config: &RunConfig) -> Result<(), &'static str> {
    let request_count: usize = config.waves.iter().map(|wave| wave.count).sum();
    if config.nodes.len() < 2
        || (config.prompt.is_empty()
            && (config.prompts.len() != request_count
                || config.prompts.iter().any(String::is_empty)))
        || (!config.prompts.is_empty() && config.prompts.len() != request_count)
        || config.max_tokens == 0
        || config.acceptance.minimum_generated_tokens == 0
        || config.acceptance.minimum_generated_tokens > config.max_tokens as usize
        || config.acceptance.expected_prefill_rows == Some(0)
        || config
            .acceptance
            .allowed_stop_reasons
            .iter()
            .any(String::is_empty)
        || (!config.acceptance.responses.is_empty()
            && config.acceptance.responses.len() != request_count)
        || config.acceptance.responses.iter().any(|expectation| {
            expectation.minimum_generated_tokens == Some(0)
                || expectation
                    .minimum_generated_tokens
                    .is_some_and(|value| value > config.max_tokens as usize)
                || expectation.minimum_response_chars == Some(0)
                || expectation
                    .exact_response
                    .as_ref()
                    .is_some_and(String::is_empty)
                || expectation.required_substrings.iter().any(String::is_empty)
                || expectation
                    .forbidden_substrings
                    .iter()
                    .any(String::is_empty)
        })
        || config.channel.is_empty()
        || config.connection_generation == 0
        || config.load_generation == 0
        || config.pre_inference_hold_ms > 120_000
        || config.waves.is_empty()
        || config.waves[0].after_ms != 0
        || config.waves.iter().any(|wave| wave.count == 0)
        || config.nodes.iter().any(|node| {
            node.generation == 0
                || node.n_batch == 0
                || node.n_ubatch == 0
                || node.n_ubatch > node.n_batch
                || node.context_size == 0
                || node.total_context_size == 0
                || node.sequence_capacity == 0
                || node
                    .context_size
                    .checked_mul(node.sequence_capacity as usize)
                    .is_none_or(|required| required > node.total_context_size)
        })
        || config
            .waves
            .windows(2)
            .any(|pair| pair[0].after_ms >= pair[1].after_ms)
    {
        return Err("run requires nodes, prompt, token budget, OUTER identity and ordered waves");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> RunConfig {
        RunConfig {
            ingress_agent: "tcp://127.0.0.1:52000".into(),
            channel: "test".into(),
            connection_generation: 1,
            load_generation: 1,
            session_id: "session".into(),
            request_id: "request".into(),
            nodes: vec![
                node("node-a", "tcp://127.0.0.1:52001"),
                node("node-b", "tcp://127.0.0.1:52002"),
            ],
            prompt: "prompt".into(),
            prompts: Vec::new(),
            max_tokens: 500,
            waves: default_waves(),
            options: String::new(),
            pre_inference_hold_ms: 0,
            acceptance: AcceptanceConfig::default(),
            timeout_ms: default_timeout(),
        }
    }

    fn node(node: &str, endpoint: &str) -> NodeConfig {
        NodeConfig {
            agent: "tcp://127.0.0.1:52000".into(),
            node: node.into(),
            generation: 1,
            binary: "server".into(),
            endpoint: endpoint.into(),
            plan: "plan".into(),
            args: Vec::new(),
            environment: Vec::new(),
            n_batch: 512,
            n_ubatch: 512,
            context_size: 1200,
            total_context_size: 1200,
            sequence_capacity: 1,
        }
    }

    #[test]
    fn rejects_acceptance_that_cannot_be_satisfied() {
        let mut config = valid_config();
        config.acceptance.minimum_generated_tokens = 501;
        assert!(validate(&config).is_err());
    }

    #[test]
    fn requires_one_response_expectation_per_request() {
        let mut config = valid_config();
        config.acceptance.responses = vec![ResponseExpectation::default(); 2];
        assert!(validate(&config).is_err());
    }
}

pub(super) fn address(node: &NodeConfig) -> NodeAddress {
    NodeAddress {
        agent: node.agent.clone(),
        node: node.node.clone(),
        generation: node.generation,
    }
}

pub(super) fn node_endpoint(node: &NodeConfig) -> Result<Endpoint, p4_protocol::ProtocolError> {
    Ok(Endpoint::node(
        Address::from_str(&node.agent)?,
        node.node.clone(),
        node.generation,
    ))
}

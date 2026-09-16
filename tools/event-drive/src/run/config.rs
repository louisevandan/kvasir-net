use p4_llamacpp_staged_adapter::v2::{NodeAddress, ResourceProfile};
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
    pub pipeline_compatibility: p4_llamacpp_staged_adapter::v2::PipelineCompatibility,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub prompts: Vec<String>,
    /// Conversation identity this OUTER mints per request. Takes the same
    /// `{{request_id}}` and `{{request_index}}` substitutions the prompt does,
    /// so a run can give every request its own conversation or share one
    /// across turns. Empty means this OUTER does not persist.
    #[serde(default)]
    pub session_key_template: String,
    pub max_tokens: u32,
    #[serde(default = "default_waves")]
    pub waves: Vec<ArrivalWave>,
    /// Optional OUTER-side bound on submitted requests that have not received
    /// their terminal RELEASE receipt. Quality evaluation uses one so queue
    /// time from an unrelated corpus member cannot replace that request's
    /// model-quality result. Open-loop service modes leave it unset.
    #[serde(default)]
    pub max_in_flight: Option<usize>,
    /// Per-request end-to-end limits, in corpus order. Empty preserves the
    /// legacy no-deadline envelope. A populated vector must cover every
    /// configured request exactly.
    #[serde(default)]
    pub request_timeout_ms: Vec<u64>,
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
    pub resource_profile: ResourceProfile,
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

#[cfg(test)]
pub(super) fn test_resource_profile() -> ResourceProfile {
    ResourceProfile {
        version: 1,
        max_requests: 1,
        max_request_retained_bytes: 1 << 20,
        max_input_tokens: 4096,
        max_request_bytes: 1 << 19,
        max_output_tokens_per_request: 500,
        max_output_tokens: 500,
        max_physical_result_bytes: 33_554_432,
        max_completion_payload_bytes: 64 << 20,
        max_completion_retained_bytes: 64 << 20,
        max_edge_retained_bytes: 64 << 20,
        max_receipt_retained_bytes: 1 << 20,
    }
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
        || config.max_in_flight == Some(0)
        || config.max_in_flight.is_some_and(|limit| {
            config
                .nodes
                .iter()
                .map(|node| node.sequence_capacity as usize)
                .min()
                .is_none_or(|capacity| limit > capacity)
        })
        || (!config.request_timeout_ms.is_empty()
            && (config.request_timeout_ms.len() != request_count
                || config.request_timeout_ms.contains(&0)))
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
            session_key_template: String::new(),
            max_tokens: 500,
            waves: default_waves(),
            max_in_flight: None,
            request_timeout_ms: Vec::new(),
            options: String::new(),
            pre_inference_hold_ms: 0,
            acceptance: AcceptanceConfig::default(),
            timeout_ms: default_timeout(),
            pipeline_compatibility: Default::default(),
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
            resource_profile: test_resource_profile(),
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

    #[test]
    fn closed_loop_limits_and_request_deadlines_cover_the_exact_run() {
        let mut config = valid_config();
        config.waves[0].count = 2;
        config.prompts = vec!["first".into(), "second".into()];
        config.max_in_flight = Some(1);
        config.request_timeout_ms = vec![600_000, 1_800_000];
        assert!(validate(&config).is_ok());

        config.request_timeout_ms.pop();
        assert!(validate(&config).is_err());
        config.request_timeout_ms.push(0);
        assert!(validate(&config).is_err());
        config.request_timeout_ms[1] = 1_800_000;
        config.max_in_flight = Some(2);
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

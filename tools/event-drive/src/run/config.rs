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
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
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

pub(super) fn validate(config: &RunConfig) -> Result<(), &'static str> {
    let request_count: usize = config.waves.iter().map(|wave| wave.count).sum();
    if config.nodes.len() < 2
        || (config.prompt.is_empty()
            && (config.prompts.len() != request_count
                || config.prompts.iter().any(String::is_empty)))
        || (!config.prompts.is_empty() && config.prompts.len() != request_count)
        || config.max_tokens == 0
        || config.channel.is_empty()
        || config.connection_generation == 0
        || config.load_generation == 0
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

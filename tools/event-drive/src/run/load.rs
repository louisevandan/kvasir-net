use super::{RunConfig, Sender, node_endpoint, replies, wire};
use p4_llamacpp_staged_adapter::v2::{LOAD_CONTENT_TYPE, LOADED_CONTENT_TYPE, LoadCommand};
use p4_protocol::event::EventClass;
use std::collections::HashSet;

pub(super) async fn drive<R, W>(
    config: &RunConfig,
    wire: &mut wire::EventWire<R, W>,
    sender: &mut Sender,
) -> Result<(), Box<dyn std::error::Error>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    for wave in agent_load_waves(config.nodes.iter().map(|node| node.agent.as_str())) {
        let mut expected = Vec::with_capacity(wave.len());
        for index in wave {
            let node = &config.nodes[index];
            let command = LoadCommand {
                load_generation: config.load_generation,
                binary: node.binary.clone(),
                endpoint: node.endpoint.clone(),
                plan: node.plan.clone(),
                args: node.args.clone(),
                environment: node.environment.clone(),
                n_batch: node.n_batch,
                n_ubatch: node.n_ubatch,
                context_size: node.context_size,
                total_context_size: node.total_context_size,
                sequence_capacity: node.sequence_capacity,
                ready_timeout_ms: config.timeout_ms,
                io_timeout_ms: config.timeout_ms,
            };
            let event = sender.event(
                node_endpoint(node)?,
                EventClass::Control,
                LOAD_CONTENT_TYPE,
                serde_json::to_vec(&command)?,
                "load",
            );
            expected.push(replies::ExpectedReply::from_request(&event));
            wire.send(event).await?;
        }
        replies::receive_exact(
            wire,
            LOADED_CONTENT_TYPE,
            expected,
            "load",
            config.timeout_ms,
        )
        .await?;
    }
    Ok(())
}

fn agent_load_waves<'a>(agents: impl IntoIterator<Item = &'a str>) -> Vec<Vec<usize>> {
    let agents: Vec<_> = agents.into_iter().collect();
    let mut loaded = vec![false; agents.len()];
    let mut waves = Vec::new();
    while loaded.iter().any(|done| !done) {
        let mut seen = HashSet::new();
        let wave = agents
            .iter()
            .enumerate()
            .filter_map(|(index, agent)| {
                (!loaded[index] && seen.insert(*agent)).then(|| {
                    loaded[index] = true;
                    index
                })
            })
            .collect();
        waves.push(wave);
    }
    waves
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_are_serial_per_agent_and_parallel_between_agents() {
        let agents = ["remote", "remote", "local", "local"];
        assert_eq!(agent_load_waves(agents), vec![vec![0, 2], vec![1, 3]]);
    }
}

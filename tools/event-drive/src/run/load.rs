use super::{RunConfig, Sender, node_endpoint, replies, wire};
use p4_llamacpp_staged_adapter::v2::{
    BuildIdentity, LOAD_CONTENT_TYPE, LOADED_CONTENT_TYPE, LoadCommand, UNIDENTIFIED,
    agree_for_profile,
};
use p4_protocol::event::EventClass;
use std::collections::HashSet;

#[derive(Debug, serde::Serialize)]
pub(crate) struct StageBuild {
    pub agent: String,
    pub node: String,
    pub generation: u64,
    pub identity: BuildIdentity,
}

pub(super) struct LoadedBuild {
    pub representative: BuildIdentity,
    pub stages: Vec<StageBuild>,
}

pub(super) async fn drive<R, W>(
    config: &RunConfig,
    wire: &mut wire::EventWire<R, W>,
    sender: &mut Sender,
) -> Result<LoadedBuild, Box<dyn std::error::Error>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut builds = Vec::with_capacity(config.nodes.len());
    let mut stages = Vec::with_capacity(config.nodes.len());
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
        let loaded = replies::receive_exact(
            wire,
            LOADED_CONTENT_TYPE,
            expected,
            "load",
            config.timeout_ms,
        )
        .await?;
        for event in &loaded {
            let identity = build_identity(&event.payload)?;
            let node = config
                .nodes
                .iter()
                .find(|node| node_endpoint(node).ok().as_ref() == Some(&event.envelope.source))
                .ok_or("loaded reply has no configured stage")?;
            builds.push(identity.clone());
            stages.push(StageBuild {
                agent: node.agent.clone(),
                node: node.node.clone(),
                generation: node.generation,
                identity,
            });
        }
    }
    // A bench may drive a stage server too old to name itself; a production
    // load path should not, which is why the choice is the caller's.
    let require_identified = std::env::var_os("P4_DRIVE_ALLOW_UNIDENTIFIED_BUILD").is_none();
    agree_for_profile(&builds, config.pipeline_compatibility, require_identified)?;
    stages.sort_by_key(|stage| {
        config.nodes.iter().position(|node| {
            node.agent == stage.agent
                && node.node == stage.node
                && node.generation == stage.generation
        })
    });
    let representative = stages
        .first()
        .map(|stage| stage.identity.clone())
        .unwrap_or(BuildIdentity {
            upstream_commit: UNIDENTIFIED.into(),
            patch_set: UNIDENTIFIED.into(),
            backend_inventory: UNIDENTIFIED.into(),
            stage_wire_abi: UNIDENTIFIED.into(),
        });
    Ok(LoadedBuild {
        representative,
        stages,
    })
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

/// What one stage reported, read out of its loaded telemetry.
///
/// The rule itself lives in the adapter: whether a pipeline is one build is
/// a property of the pipeline, not of the harness that drives it.
fn build_identity(payload: &[u8]) -> Result<BuildIdentity, Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_slice(payload)?;
    let field = |name: &str| {
        value
            .get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or(UNIDENTIFIED)
            .to_owned()
    };
    Ok(BuildIdentity {
        upstream_commit: field("upstream_commit"),
        patch_set: field("patch_set"),
        backend_inventory: field("backend_inventory"),
        stage_wire_abi: field("stage_wire_abi"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_are_serial_per_agent_and_parallel_between_agents() {
        let agents = ["remote", "remote", "local", "local"];
        assert_eq!(agent_load_waves(agents), vec![vec![0, 2], vec![1, 3]]);
    }

    // Whether a pipeline is one build is the adapter's rule and is tested
    // there. What belongs here is reading a stage's answer out of the
    // telemetry it sent.
    #[test]
    fn a_payload_without_the_fields_reads_as_unidentified() {
        let payload = br#"{"state":"loaded","load_generation":1}"#;
        let identity = build_identity(payload).expect("parse");
        assert_eq!(identity.upstream_commit, UNIDENTIFIED);
        assert_eq!(identity.patch_set, UNIDENTIFIED);
        assert_eq!(identity.backend_inventory, UNIDENTIFIED);
        assert!(!identity.identified());
    }

    #[test]
    fn a_payload_with_the_fields_is_read_verbatim() {
        let payload = br#"{"upstream_commit":"557614e02","patch_set":"00e66c6b","backend_inventory":"CPU[CPU]|CUDA[CUDA0]"}"#;
        let identity = build_identity(payload).expect("parse");
        assert_eq!(identity.upstream_commit, "557614e02");
        assert_eq!(identity.patch_set, "00e66c6b");
        assert_eq!(identity.backend_inventory, "CPU[CPU]|CUDA[CUDA0]");
        assert!(identity.identified());
    }
}

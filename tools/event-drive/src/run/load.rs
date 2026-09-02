use super::{RunConfig, Sender, node_endpoint, replies, wire};
use p4_llamacpp_staged_adapter::v2::{LOAD_CONTENT_TYPE, LOADED_CONTENT_TYPE, LoadCommand};
use p4_protocol::event::EventClass;
use std::collections::HashSet;

pub(super) async fn drive<R, W>(
    config: &RunConfig,
    wire: &mut wire::EventWire<R, W>,
    sender: &mut Sender,
) -> Result<BuildIdentity, Box<dyn std::error::Error>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut builds = Vec::with_capacity(config.nodes.len());
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
            builds.push(build_identity(&event.payload)?);
        }
    }
    agree(&builds)?;
    Ok(builds.into_iter().next().unwrap_or(BuildIdentity {
        upstream_commit: "unknown".into(),
        patch_set: "unknown".into(),
        backend_layout: "unknown".into(),
    }))
}

/// What one stage reported it was built from.
#[derive(Debug, Eq, PartialEq, serde::Serialize)]
pub(super) struct BuildIdentity {
    pub upstream_commit: String,
    pub patch_set: String,
    /// Which ggml backends the stage loaded. Two stages can share a source
    /// identity and still be a CPU build and a CUDA build - they would agree
    /// on every other field while computing on different hardware.
    pub backend_layout: String,
}

fn build_identity(payload: &[u8]) -> Result<BuildIdentity, Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_slice(payload)?;
    let field = |name: &str| {
        value
            .get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_owned()
    };
    Ok(BuildIdentity {
        upstream_commit: field("upstream_commit"),
        patch_set: field("patch_set"),
        backend_layout: field("backend_layout"),
    })
}

/// Every stage of one pipeline has to be the same build.
///
/// A pipeline assembled from stages built at different times passes every
/// structural check and then produces wrong numbers or a segfault, because
/// the cut-set layout each side assumes is decided by the code, not by the
/// plan. The upstream commit alone does not settle it: two builds can share
/// it and differ in every behaviour the patch queue touches.
///
/// A stage that does not name its build fails the check. Uniform `unknown`
/// used to pass, on the reasoning that an older stage server predates the
/// field - but four different legacy binaries all answer `unknown`, so that
/// reasoning let exactly the mixed pipeline this exists to catch through the
/// gate. A run that cannot name its own build is not attributable, and an
/// unattributable measurement is the thing this whole mechanism is for.
///
/// `P4_DRIVE_ALLOW_UNIDENTIFIED_BUILD` reopens it for driving a stage server
/// too old to answer - deliberately, and visibly in the environment, rather
/// than as a silent fallback.
fn agree(builds: &[BuildIdentity]) -> Result<(), Box<dyn std::error::Error>> {
    let Some(first) = builds.first() else {
        return Ok(());
    };
    let unidentified = std::env::var_os("P4_DRIVE_ALLOW_UNIDENTIFIED_BUILD").is_some();
    if !unidentified
        && let Some(nameless) = builds
            .iter()
            .find(|build| build.upstream_commit == "unknown" || build.patch_set == "unknown")
    {
        return Err(format!(
            "stage does not name its build: upstream {} patch_set {}; set P4_DRIVE_ALLOW_UNIDENTIFIED_BUILD to drive it anyway",
            nameless.upstream_commit, nameless.patch_set
        )
        .into());
    }
    if let Some(other) = builds.iter().find(|build| *build != first) {
        return Err(format!(
            "pipeline stages are different builds: \
             upstream {} patch_set {} backend {} \
             against upstream {} patch_set {} backend {}",
            first.upstream_commit,
            first.patch_set,
            first.backend_layout,
            other.upstream_commit,
            other.patch_set,
            other.backend_layout
        )
        .into());
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

    fn build(upstream: &str, patch_set: &str) -> BuildIdentity {
        backend_build(upstream, patch_set, "CUDA[CUDA0,CUDA1];CPU[CPU]")
    }

    fn backend_build(upstream: &str, patch_set: &str, backend: &str) -> BuildIdentity {
        BuildIdentity {
            upstream_commit: upstream.into(),
            patch_set: patch_set.into(),
            backend_layout: backend.into(),
        }
    }

    #[test]
    fn one_build_across_every_stage_is_accepted() {
        let builds = vec![
            build("557614e02", "00e66c6b"),
            build("557614e02", "00e66c6b"),
        ];
        assert!(agree(&builds).is_ok());
    }

    #[test]
    fn the_same_upstream_with_a_different_queue_is_refused() {
        // The case the upstream commit alone cannot see, and the reason the
        // patch set travels at all.
        let builds = vec![
            build("557614e02", "00e66c6b"),
            build("557614e02", "615fe3c6"),
        ];
        let error = agree(&builds).expect_err("mixed queues");
        assert!(error.to_string().contains("different builds"), "{error}");
    }

    #[test]
    fn a_different_upstream_is_refused() {
        let builds = vec![
            build("557614e02", "00e66c6b"),
            build("d7a207411", "00e66c6b"),
        ];
        assert!(agree(&builds).is_err());
    }

    #[test]
    fn a_pipeline_that_cannot_name_its_build_is_refused() {
        // Four different legacy binaries all answer `unknown`, so tolerating
        // uniform `unknown` admitted exactly the mixed pipeline this check
        // exists to catch.
        let builds = vec![build("unknown", "unknown"), build("unknown", "unknown")];
        let error = agree(&builds).expect_err("unidentified build");
        assert!(error.to_string().contains("does not name its build"), "{error}");
    }

    #[test]
    fn one_named_stage_beside_an_unnamed_one_is_refused() {
        let builds = vec![build("557614e02", "00e66c6b"), build("unknown", "unknown")];
        assert!(agree(&builds).is_err());
    }

    #[test]
    fn a_stage_that_predates_the_field_beside_one_that_does_not_is_refused() {
        // Already a mixed pipeline: one of them was built from a tree the
        // other's build system did not know how to stamp.
        let builds = vec![build("557614e02", "00e66c6b"), build("unknown", "unknown")];
        assert!(agree(&builds).is_err());
    }

    #[test]
    fn one_source_identity_on_two_different_backends_is_refused() {
        // The case source identity cannot see: the same commit and the same
        // patch queue, built once for CPU and once for CUDA. Every other
        // field agrees while the two stages compute on different hardware.
        let builds = vec![
            backend_build("557614e02", "00e66c6b", "CUDA[CUDA0];CPU[CPU]"),
            backend_build("557614e02", "00e66c6b", "CPU[CPU]"),
        ];
        let error = agree(&builds).expect_err("mixed backends");
        assert!(error.to_string().contains("backend"), "{error}");
    }

    #[test]
    fn the_same_backend_with_different_devices_is_refused() {
        // Two CUDA builds, one of which enumerated a second GPU: the stage
        // that sees one device cannot run the placement the other planned.
        let builds = vec![
            backend_build("557614e02", "00e66c6b", "CUDA[CUDA0,CUDA1]"),
            backend_build("557614e02", "00e66c6b", "CUDA[CUDA0]"),
        ];
        assert!(agree(&builds).is_err());
    }

    #[test]
    fn an_empty_pipeline_has_nothing_to_disagree_about() {
        assert!(agree(&[]).is_ok());
    }

    #[test]
    fn a_payload_without_the_fields_reads_as_unknown() {
        let payload = br#"{"state":"loaded","load_generation":1}"#;
        let identity = build_identity(payload).expect("parse");
        assert_eq!(identity, backend_build("unknown", "unknown", "unknown"));
    }

    #[test]
    fn a_payload_with_the_fields_is_read_verbatim() {
        let payload = br#"{"upstream_commit":"557614e02","patch_set":"00e66c6b","backend_layout":"CPU[CPU]"}"#;
        let identity = build_identity(payload).expect("parse");
        assert_eq!(identity, backend_build("557614e02", "00e66c6b", "CPU[CPU]"));
    }
}

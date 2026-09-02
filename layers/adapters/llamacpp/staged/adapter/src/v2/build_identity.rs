//! Which llama.cpp build a stage is, and whether a pipeline is one build.
//!
//! This lives in the adapter rather than in the test drive because the rule is
//! a property of the pipeline, not of how it happens to be exercised. A
//! four-node pipeline assembled from stages built at different times, or for
//! different backends, passes every structural check and then produces wrong
//! numbers or a segfault: the cut-set layout and the buffer placement each
//! side assumes are decided by the code, not by the plan.
//!
//! It was written in `tools/event-drive` first, which meant the harness
//! refused a mixed pipeline and nothing else did.

use serde::{Deserialize, Serialize};

/// What one stage says it is.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct BuildIdentity {
    /// The llama.cpp commit this stage was built from.
    pub upstream_commit: String,
    /// The compat patch queue applied on top of it. The commit alone does not
    /// identify a build: two can share it and differ in every behaviour the
    /// queue touches.
    pub patch_set: String,
    /// Which ggml backends and devices the process registered. An inventory,
    /// not a placement - it separates a CPU build from a CUDA one and counts
    /// registered devices, and says nothing about where tensors ended up.
    pub backend_inventory: String,
}

/// The value a stage reports when it predates a field, or could not answer.
pub const UNIDENTIFIED: &str = "unknown";

impl BuildIdentity {
    /// Whether every part of this identity is actually named.
    pub fn identified(&self) -> bool {
        self.upstream_commit != UNIDENTIFIED
            && self.patch_set != UNIDENTIFIED
            && self.backend_inventory != UNIDENTIFIED
    }

    /// A one-line form for an error or a record.
    pub fn describe(&self) -> String {
        format!(
            "upstream {} patch_set {} backend {}",
            self.upstream_commit, self.patch_set, self.backend_inventory
        )
    }
}

/// Why a pipeline was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildDisagreement {
    /// A stage did not name some part of its build.
    Unidentified(BuildIdentity),
    /// Two stages are different builds.
    Mixed {
        first: BuildIdentity,
        other: BuildIdentity,
    },
}

impl std::fmt::Display for BuildDisagreement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.describe())
    }
}

impl std::error::Error for BuildDisagreement {}

impl BuildDisagreement {
    pub fn describe(&self) -> String {
        match self {
            Self::Unidentified(build) => {
                format!("stage does not name its build: {}", build.describe())
            }
            Self::Mixed { first, other } => format!(
                "pipeline stages are different builds: {} against {}",
                first.describe(),
                other.describe()
            ),
        }
    }
}

/// Refuses a pipeline that is not one identified build.
///
/// `require_identified` is what an OUTER decides: a bench driving a stage
/// server too old to answer may choose to proceed, a production load path
/// should not. Agreement among the unidentified is not identification - four
/// stages that all answer `unknown` agree with each other perfectly.
pub fn agree(
    builds: &[BuildIdentity],
    require_identified: bool,
) -> Result<(), BuildDisagreement> {
    let Some(first) = builds.first() else {
        return Ok(());
    };
    if require_identified
        && let Some(nameless) = builds.iter().find(|build| !build.identified())
    {
        return Err(BuildDisagreement::Unidentified(nameless.clone()));
    }
    if let Some(other) = builds.iter().find(|build| *build != first) {
        return Err(BuildDisagreement::Mixed {
            first: first.clone(),
            other: other.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BuildDisagreement, BuildIdentity, agree};

    fn build(upstream: &str, patch_set: &str, backend: &str) -> BuildIdentity {
        BuildIdentity {
            upstream_commit: upstream.into(),
            patch_set: patch_set.into(),
            backend_inventory: backend.into(),
        }
    }

    fn cuda(upstream: &str, patch_set: &str) -> BuildIdentity {
        build(upstream, patch_set, "CPU[CPU]|CUDA[CUDA0]")
    }

    #[test]
    fn one_build_across_every_stage_is_accepted() {
        let builds = vec![cuda("557614e02", "00e66c6b"), cuda("557614e02", "00e66c6b")];
        assert_eq!(agree(&builds, true), Ok(()));
    }

    #[test]
    fn the_same_upstream_with_a_different_queue_is_refused() {
        let builds = vec![cuda("557614e02", "00e66c6b"), cuda("557614e02", "615fe3c6")];
        assert!(matches!(
            agree(&builds, true),
            Err(BuildDisagreement::Mixed { .. })
        ));
    }

    #[test]
    fn one_source_identity_on_two_backends_is_refused() {
        // Every other field agrees while the two stages compute on different
        // hardware, which source identity alone cannot see.
        let builds = vec![
            build("557614e02", "00e66c6b", "CPU[CPU]|CUDA[CUDA0]"),
            build("557614e02", "00e66c6b", "CPU[CPU]"),
        ];
        let error = agree(&builds, true).expect_err("mixed backends");
        assert!(error.describe().contains("backend"), "{}", error.describe());
    }

    #[test]
    fn the_same_backend_with_different_devices_is_refused() {
        let builds = vec![
            build("557614e02", "00e66c6b", "CUDA[CUDA0,CUDA1]"),
            build("557614e02", "00e66c6b", "CUDA[CUDA0]"),
        ];
        assert!(agree(&builds, true).is_err());
    }

    #[test]
    fn stages_that_all_answer_unknown_are_refused_when_identity_is_required() {
        let builds = vec![
            build("unknown", "unknown", "unknown"),
            build("unknown", "unknown", "unknown"),
        ];
        assert!(matches!(
            agree(&builds, true),
            Err(BuildDisagreement::Unidentified(_))
        ));
    }

    #[test]
    fn an_unnamed_backend_alone_is_enough_to_refuse() {
        let builds = vec![
            build("557614e02", "00e66c6b", "unknown"),
            build("557614e02", "00e66c6b", "unknown"),
        ];
        assert!(agree(&builds, true).is_err());
    }

    #[test]
    fn an_outer_may_choose_to_drive_an_unidentified_pipeline() {
        let builds = vec![
            build("unknown", "unknown", "unknown"),
            build("unknown", "unknown", "unknown"),
        ];
        assert_eq!(agree(&builds, false), Ok(()));
    }

    #[test]
    fn an_empty_pipeline_has_nothing_to_disagree_about() {
        assert_eq!(agree(&[], true), Ok(()));
    }
}

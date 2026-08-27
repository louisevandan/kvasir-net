// Pure compatibility-manifest logic for an upstream bump.
//
// Kept free of git and the filesystem so the manifest shape can be tested
// without a network fetch or a worktree. Git orchestration lives in
// ../bump-pipeline-upstream.mjs.

export const OFFICIAL_REPOSITORY = "https://github.com/ggml-org/llama.cpp.git";

export function parseArguments(argv) {
  const positional = argv.filter((value) => !value.startsWith("--"));
  const ref = positional[0];
  if (!ref) {
    throw new Error("usage: bump-pipeline-upstream.mjs <ref> [--apply] [--from <sha9>]");
  }
  const fromIndex = argv.indexOf("--from");
  const from = fromIndex < 0 ? null : argv[fromIndex + 1];
  if (fromIndex >= 0 && (!from || from.startsWith("--"))) {
    throw new Error("--from requires a compatibility directory name");
  }
  return { ref, apply: argv.includes("--apply"), from };
}

/// The bump is a provenance record: it keeps the previous pin so a rebase can
/// always be traced back to the base it was derived from.
export function nextManifest(previous, next) {
  const { target, identity, patches, patchSetSha256, patchedTree, observedAt } = next;
  if (!/^[0-9a-f]{40}$/u.test(target)) {
    throw new Error(`upstream commit must be a full sha: ${target}`);
  }
  if (!patches.length) throw new Error("a compatibility manifest needs at least one patch");
  for (const patch of patches) {
    if (!/^[0-9a-f]{64}$/u.test(patch.sha256)) {
      throw new Error(`patch ${patch.file} has an invalid sha256`);
    }
  }
  const replacingSameTarget = previous.upstream_commit === target;
  return {
    ...previous,
    upstream_repository: OFFICIAL_REPOSITORY,
    upstream_commit: target,
    upstream_commit_date: identity.date,
    upstream_subject: identity.subject,
    observed_at: observedAt,
    nearest_release_tag: identity.tag,
    previous_linker_pin: replacingSameTarget
      ? previous.previous_linker_pin
      : previous.upstream_commit,
    previous_official_base: replacingSameTarget
      ? previous.previous_official_base
      : previous.upstream_commit,
    patch_set_sha256: patchSetSha256,
    patched_tree: patchedTree,
    patches,
  };
}

/// A bump is adoptable only when the entire series still applies; a partial
/// application would leave the ABI half-installed.
export function summarize(results) {
  const conflicts = results.filter((entry) => !entry.ok);
  return {
    total: results.length,
    conflicts: conflicts.map((entry) => entry.file),
    adoptable: conflicts.length === 0,
  };
}

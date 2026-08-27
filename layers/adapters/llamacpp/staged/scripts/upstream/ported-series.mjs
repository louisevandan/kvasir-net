// Pure validation for recording a manually rebased compatibility series.
// Git and filesystem orchestration live in ../record-pipeline-port.mjs.

function option(argv, name) {
  const index = argv.indexOf(name);
  if (index < 0) return null;
  const value = argv[index + 1];
  if (!value || value.startsWith("--")) {
    throw new Error(`${name} requires a value`);
  }
  return value;
}

export function parseRecordArguments(argv) {
  const candidate = option(argv, "--candidate");
  const target = option(argv, "--target");
  const from = option(argv, "--from");
  if (!candidate || !target) {
    throw new Error(
      "usage: record-pipeline-port.mjs --candidate <git-worktree> --target <official-ref> [--from <sha9>] [--replace]",
    );
  }
  if (from && !/^[0-9a-f]{9}$/u.test(from)) {
    throw new Error("--from must be a nine-character lowercase commit prefix");
  }
  return { candidate, target, from, replace: argv.includes("--replace") };
}

export function validatePortedSeries(patches, commits) {
  if (commits.length !== patches.length) {
    throw new Error(
      `ported series has ${commits.length} commits; expected ${patches.length} compatibility patches`,
    );
  }
  return patches.map((patch, index) => {
    const expected = `compat: ${patch.file.replace(/\.patch$/u, "")}`;
    const commit = commits[index];
    if (commit.subject !== expected) {
      throw new Error(
        `ported commit ${index + 1} must be named "${expected}", got "${commit.subject}"`,
      );
    }
    if (index > 0 && commit.parent !== commits[index - 1].sha) {
      throw new Error(`ported series is not linear at ${commit.sha}`);
    }
    return { patch, commit: commit.sha };
  });
}

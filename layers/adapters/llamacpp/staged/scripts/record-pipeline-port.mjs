#!/usr/bin/env node
// Records a manually rebased, one-commit-per-patch llama.cpp compatibility
// branch as a versioned compat/<sha9> queue. The official upstream checkout is
// read-only here; adopting its pin remains a separate explicit step.
// See apps/llama/docs/upstream.md#conflict-port-recording.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { nextManifest } from "./upstream/manifest.mjs";
import { parseRecordArguments, validatePortedSeries } from "./upstream/ported-series.mjs";

const shapeRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const backendRoot = path.resolve(shapeRoot, "..");
const repoRoot = path.resolve(backendRoot, "../../../../..");
const upstreamDir = path.join(backendRoot, "upstream");
const compatibilityRoot = path.join(shapeRoot, "compat");

function git(args, cwd, { binary = false, allowFailure = false, input = null } = {}) {
  const result = spawnSync("git", args, {
    cwd,
    input,
    encoding: binary ? null : "utf8",
    maxBuffer: 128 * 1024 * 1024,
  });
  if (result.status !== 0 && !allowFailure) {
    const stderr = binary ? result.stderr.toString("utf8") : result.stderr;
    throw new Error(`git ${args.join(" ")} failed: ${stderr.trim()}`);
  }
  return { status: result.status, stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
}

const sha256 = (data) => crypto.createHash("sha256").update(data).digest("hex");

function sourceCompatibility(from) {
  const head = git(["rev-parse", "HEAD"], upstreamDir).stdout.trim();
  const directory = path.join(compatibilityRoot, from ?? head.slice(0, 9));
  const manifestPath = path.join(directory, "manifest.json");
  if (!fs.existsSync(manifestPath)) {
    throw new Error(`no source compatibility manifest at ${path.relative(repoRoot, manifestPath)}`);
  }
  return { directory, manifest: JSON.parse(fs.readFileSync(manifestPath, "utf8")) };
}

function describe(commit) {
  const lines = git(["show", "-s", "--format=%H%n%cI%n%s", commit], upstreamDir)
    .stdout.trim().split("\n");
  const tag = git(["describe", "--tags", "--abbrev=0", commit], upstreamDir, {
    allowFailure: true,
  });
  return {
    commit: lines[0],
    date: lines[1],
    subject: lines[2] ?? "",
    tag: tag.status === 0 ? tag.stdout.trim() : "",
  };
}

function candidateCommits(candidate, target) {
  const lines = git([
    "log", "--reverse", "--format=%H%x09%P%x09%s", `${target}..HEAD`,
  ], candidate).stdout.trim();
  if (!lines) return [];
  return lines.split("\n").map((line) => {
    const [sha, parents, ...subject] = line.split("\t");
    const parentList = parents.split(" ").filter(Boolean);
    if (parentList.length !== 1) throw new Error(`ported commit ${sha} must have one parent`);
    return { sha, parent: parentList[0], subject: subject.join("\t") };
  });
}

function patchBuffers(candidate, series) {
  return series.map((entry) => ({
    ...entry,
    contents: git(
      ["diff", "--binary", "--full-index", entry.parent, entry.sha],
      candidate,
      { binary: true },
    ).stdout,
  }));
}

function replayAndVerify(target, patches, expectedTree) {
  const worktree = path.join(repoRoot, ".cache", "llama-pipeline-record", target.slice(0, 12));
  fs.rmSync(worktree, { recursive: true, force: true });
  fs.mkdirSync(path.dirname(worktree), { recursive: true });
  git(["worktree", "add", "--detach", worktree, target], upstreamDir);
  try {
    for (const patch of patches) {
      git(["apply", "--check", "--whitespace=nowarn", "-"], worktree, {
        binary: true,
        input: patch.contents,
      });
      git(["apply", "--whitespace=nowarn", "-"], worktree, {
        binary: true,
        input: patch.contents,
      });
    }
    git(["diff", "--check"], worktree);
    git(["add", "-A"], worktree);
    const replayTree = git(["write-tree"], worktree).stdout.trim();
    if (replayTree !== expectedTree) {
      throw new Error(`replayed tree ${replayTree} does not match candidate tree ${expectedTree}`);
    }
  } finally {
    git(["worktree", "remove", "--force", worktree], upstreamDir, { allowFailure: true });
    fs.rmSync(worktree, { recursive: true, force: true });
  }
}

const options = parseRecordArguments(process.argv.slice(2));
const candidate = path.resolve(options.candidate);
const { directory: sourceDirectory, manifest } = sourceCompatibility(options.from);
if (!fs.existsSync(path.join(candidate, ".git"))) {
  throw new Error(`candidate is not a git worktree: ${candidate}`);
}
if (git(["status", "--porcelain"], candidate).stdout.trim()) {
  throw new Error("candidate worktree must be clean");
}

git(["fetch", "origin", options.target, "--tags"], upstreamDir, { allowFailure: true });
const target = git(["rev-parse", "--verify", `${options.target}^{commit}`], upstreamDir).stdout.trim();
if (git(["merge-base", "--is-ancestor", target, "HEAD"], candidate, { allowFailure: true }).status !== 0) {
  throw new Error(`official target ${target} is not an ancestor of candidate HEAD`);
}

const commits = candidateCommits(candidate, target);
const named = validatePortedSeries(manifest.patches, commits);
if (commits[0]?.parent !== target) throw new Error("first compatibility commit must directly follow target");
const series = named.map((entry, index) => ({ ...commits[index], patch: entry.patch }));
const patches = patchBuffers(candidate, series);
const candidateTree = git(["rev-parse", "HEAD^{tree}"], candidate).stdout.trim();
replayAndVerify(target, patches, candidateTree);

const targetDirectory = path.join(compatibilityRoot, target.slice(0, 9));
const sourceReadme = fs.readFileSync(path.join(sourceDirectory, "README.md"), "utf8");
if (fs.existsSync(targetDirectory)) {
  if (!options.replace) {
    throw new Error(`target compatibility directory already exists: ${path.relative(repoRoot, targetDirectory)}`);
  }
  const existingPath = path.join(targetDirectory, "manifest.json");
  const existing = fs.existsSync(existingPath)
    ? JSON.parse(fs.readFileSync(existingPath, "utf8"))
    : null;
  if (existing?.upstream_commit !== target) {
    throw new Error("--replace may only rewrite the compatibility directory for the same target");
  }
  fs.rmSync(targetDirectory, { recursive: true, force: true });
}
fs.mkdirSync(targetDirectory, { recursive: true });
const patchEntries = patches.map(({ patch, contents }) => {
  fs.writeFileSync(path.join(targetDirectory, patch.file), contents);
  return { ...patch, sha256: sha256(contents) };
});
const fullDiff = git(["diff", "--binary", "--full-index", target, "HEAD"], candidate, {
  binary: true,
}).stdout;
const identity = describe(target);
const next = nextManifest(manifest, {
  target,
  identity,
  patches: patchEntries,
  patchSetSha256: sha256(fullDiff),
  patchedTree: candidateTree,
  observedAt: new Date().toISOString(),
});
fs.writeFileSync(path.join(targetDirectory, "manifest.json"), `${JSON.stringify(next, null, 2)}\n`);
fs.writeFileSync(
  path.join(targetDirectory, "README.md"),
  sourceReadme.replace(manifest.upstream_commit.slice(0, 9), target.slice(0, 9)),
);

process.stdout.write(`${JSON.stringify({
  compatibility_dir: path.relative(repoRoot, targetDirectory),
  upstream_commit: target,
  patch_count: patchEntries.length,
  patch_set_sha256: next.patch_set_sha256,
  patched_tree: candidateTree,
})}\n`);

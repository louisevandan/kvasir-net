#!/usr/bin/env node
//
// Replays the compatibility patch series onto a newer official llama.cpp
// commit and records the result as a new compat/<sha9>/ directory.
//
//   node apps/p4/layers/adapters/llamacpp/staged/scripts/bump-pipeline-upstream.mjs <ref> [--apply] [--from <sha9>]
//
// Without --apply this only reports, per patch, whether it still applies.
// The submodule pin is never moved here: moving it changes what everyone
// builds, so it stays an explicit human action printed after a clean replay.
//
// Manifest shape and adoption rules live in ./upstream/manifest.mjs.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { nextManifest, parseArguments, summarize } from "./upstream/manifest.mjs";

// upstream/ and compat/ are siblings of this script inside the adapter that
// owns them, so they are located from here rather than from the repository.
// This shape of the llama.cpp adapter; the clone it patches belongs to the
// backend rather than to one shape of it, so it sits a level up.
const shapeRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const backendRoot = path.resolve(shapeRoot, "..");
const repoRoot = path.resolve(backendRoot, "../../../../..");
const upstreamDir = path.join(backendRoot, "upstream");
const compatibilityRoot = path.join(shapeRoot, "compat");

function git(args, cwd, { binary = false, allowFailure = false } = {}) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: binary ? null : "utf8",
    maxBuffer: 64 * 1024 * 1024,
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
    throw new Error(`no compatibility manifest at ${path.relative(repoRoot, manifestPath)}`);
  }
  return { directory, manifest: JSON.parse(fs.readFileSync(manifestPath, "utf8")) };
}

function resolveTarget(ref) {
  git(["fetch", "origin", ref, "--tags"], upstreamDir, { allowFailure: true });
  for (const candidate of [ref, `origin/${ref}`, "FETCH_HEAD"]) {
    const resolved = git(["rev-parse", "--verify", `${candidate}^{commit}`], upstreamDir, {
      allowFailure: true,
    });
    if (resolved.status === 0) return resolved.stdout.trim();
  }
  throw new Error(`unable to resolve upstream ref ${ref}`);
}

function describe(commit) {
  const show = git(["show", "-s", "--format=%H%n%cI%n%s", commit], upstreamDir)
    .stdout.trim()
    .split("\n");
  const tag = git(["describe", "--tags", "--abbrev=0", commit], upstreamDir, {
    allowFailure: true,
  });
  return {
    commit: show[0],
    date: show[1],
    subject: show[2] ?? "",
    tag: tag.status === 0 ? tag.stdout.trim() : "",
  };
}

/// Applies the series one patch at a time so a failure names the exact patch a
/// maintainer has to rebase, instead of failing the whole series at once.
function replay(worktree, sourceDirectory, manifest) {
  return manifest.patches.map((entry) => {
    const patchPath = path.join(sourceDirectory, entry.file);
    const check = git(["apply", "--check", "--whitespace=nowarn", patchPath], worktree, {
      allowFailure: true,
    });
    if (check.status !== 0) {
      return { file: entry.file, ok: false, detail: check.stderr.trim() };
    }
    git(["apply", "--whitespace=nowarn", patchPath], worktree);
    return { file: entry.file, ok: true, detail: "" };
  });
}

function verifyAbi(worktree) {
  const api = fs.readFileSync(path.join(worktree, "include", "llama.h"), "utf8");
  if (!api.includes("llama_linkcpp_runtime_configure")) {
    throw new Error("patched source does not expose the pipeline compatibility ABI");
  }
}

function record(target, identity, sourceDirectory, manifest, worktree) {
  const directory = path.join(compatibilityRoot, target.slice(0, 9));
  fs.mkdirSync(directory, { recursive: true });
  const patches = manifest.patches.map((entry) => {
    const contents = fs.readFileSync(path.join(sourceDirectory, entry.file));
    fs.writeFileSync(path.join(directory, entry.file), contents);
    return { file: entry.file, sha256: sha256(contents) };
  });
  const diff = git(["diff", "--binary", "--full-index"], worktree, { binary: true }).stdout;
  git(["add", "-A"], worktree);
  const next = nextManifest(manifest, {
    target,
    identity,
    patches,
    patchSetSha256: sha256(diff),
    patchedTree: git(["write-tree"], worktree).stdout.trim(),
    observedAt: new Date().toISOString(),
  });
  fs.writeFileSync(path.join(directory, "manifest.json"), `${JSON.stringify(next, null, 2)}\n`);
  return directory;
}

const options = parseArguments(process.argv.slice(2));
const { directory: sourceDirectory, manifest } = sourceCompatibility(options.from);
const target = resolveTarget(options.ref);
const identity = describe(target);

if (target === manifest.upstream_commit) {
  console.log(`upstream is already pinned at ${target}; nothing to replay`);
  process.exit(0);
}

const worktree = path.join(repoRoot, ".cache", "llama-pipeline-bump", target.slice(0, 12));
fs.rmSync(worktree, { recursive: true, force: true });
fs.mkdirSync(path.dirname(worktree), { recursive: true });
git(["worktree", "add", "--detach", worktree, target], upstreamDir);

let adoptable = false;
try {
  const results = replay(worktree, sourceDirectory, manifest);
  const summary = summarize(results);
  adoptable = summary.adoptable;

  console.log(`from ${manifest.upstream_commit.slice(0, 12)} -> ${target.slice(0, 12)}`);
  console.log(`  ${identity.tag || "(untagged)"}  ${identity.date}  ${identity.subject}`);
  for (const entry of results) {
    console.log(`  ${entry.ok ? "ok      " : "CONFLICT"} ${entry.file}`);
    if (!entry.ok) {
      for (const line of entry.detail.split("\n")) console.log(`            ${line}`);
    }
  }

  if (!summary.adoptable) {
    console.log(
      `\n${summary.conflicts.length} of ${summary.total} patches need a rebase before this upstream can be adopted.`,
    );
  } else if (!options.apply) {
    console.log("\nthe whole series still applies; rerun with --apply to record it");
  } else {
    verifyAbi(worktree);
    const written = record(target, identity, sourceDirectory, manifest, worktree);
    console.log(`\nrecorded ${path.relative(repoRoot, written)}`);
    console.log("next, adopt the pin and rebuild:");
    console.log(`  git -C apps/p4/layers/adapters/llamacpp/upstream checkout ${target}`);
    console.log("  node apps/p4/layers/adapters/llamacpp/staged/scripts/prepare-pipeline-upstream.mjs");
    console.log("  npm run check:pipeline-contract --workspace apps/llama");
  }
} finally {
  git(["worktree", "remove", "--force", worktree], upstreamDir, { allowFailure: true });
  fs.rmSync(worktree, { recursive: true, force: true });
}

process.exit(adoptable ? 0 : 1);

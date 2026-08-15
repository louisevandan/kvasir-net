#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

// upstream/ and compat/ are siblings of this script inside the adapter that
// owns them, so they are located from here rather than from the repository.
const adapterRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(adapterRoot, "../../../../..");
const upstreamDir = path.join(adapterRoot, "upstream");
const compatibilityRoot = path.join(adapterRoot, "compat");

function runGit(args, cwd, binary = false) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: binary ? null : "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.status !== 0) {
    const stderr = binary ? result.stderr.toString("utf8") : result.stderr;
    throw new Error(`git ${args.join(" ")} failed: ${stderr.trim()}`);
  }
  return result.stdout;
}

function sha256(data) {
  return crypto.createHash("sha256").update(data).digest("hex");
}

function parseOutputPath() {
  const outputIndex = process.argv.indexOf("--out");
  if (outputIndex < 0) return null;
  const value = process.argv[outputIndex + 1];
  if (!value) throw new Error("--out requires a path");
  return path.resolve(value);
}

function validateOfficialPin(manifest) {
  if (manifest.upstream_repository !== "https://github.com/ggml-org/llama.cpp.git") {
    throw new Error("compatibility manifest must name the official llama.cpp repository");
  }
  const gitmodules = fs.readFileSync(path.join(repoRoot, ".gitmodules"), "utf8");
  if (!gitmodules.includes("url = https://github.com/ggml-org/llama.cpp.git")) {
    throw new Error("the upstream submodule must use the official ggml-org/llama.cpp URL");
  }
  const head = runGit(["rev-parse", "HEAD"], upstreamDir).trim();
  if (head !== manifest.upstream_commit) {
    throw new Error(`upstream HEAD ${head} does not match ${manifest.upstream_commit}`);
  }
  if (runGit(["status", "--porcelain"], upstreamDir).trim()) {
    throw new Error("official upstream submodule is dirty");
  }
  const intrusion = spawnSync(
    "git",
    ["grep", "-n", "llama_linkcpp_", "HEAD", "--"],
    { cwd: upstreamDir, encoding: "utf8" },
  );
  if (intrusion.status === 0) {
    throw new Error("official upstream commit contains Linker-owned symbols");
  }
  if (intrusion.status !== 1) {
    throw new Error(`unable to inspect official upstream: ${intrusion.stderr.trim()}`);
  }
}

function validatePatches(manifest, compatibilityDir) {
  for (const entry of manifest.patches) {
    const patchPath = path.join(compatibilityDir, entry.file);
    if (!fs.existsSync(patchPath)) throw new Error(`missing compatibility patch: ${entry.file}`);
    const actual = sha256(fs.readFileSync(patchPath));
    if (actual !== entry.sha256) {
      throw new Error(`compatibility patch hash mismatch: ${entry.file}`);
    }
  }
}

function validatePreparedSource(targetDir, manifest) {
  const head = runGit(["rev-parse", "HEAD"], targetDir).trim();
  if (head !== manifest.upstream_commit) {
    throw new Error(`prepared source HEAD ${head} does not match ${manifest.upstream_commit}`);
  }
  const status = runGit(["status", "--porcelain"], targetDir);
  if (!status.trim() || status.split(/\r?\n/).some((line) => line.startsWith("??"))) {
    throw new Error("prepared source must contain only the tracked compatibility diff");
  }
  runGit(["diff", "--check"], targetDir);
  const diff = runGit(["diff", "--binary", "--full-index"], targetDir, true);
  const actual = sha256(diff);
  if (actual !== manifest.patch_set_sha256) {
    throw new Error(`prepared compatibility diff hash mismatch: ${actual}`);
  }
  const api = fs.readFileSync(path.join(targetDir, "include", "llama.h"), "utf8");
  if (!api.includes("llama_linkcpp_runtime_configure")) {
    throw new Error("prepared source does not expose the Pipeline compatibility ABI");
  }
}

const upstreamHead = runGit(["rev-parse", "HEAD"], upstreamDir).trim();
const compatibilityDir = path.join(compatibilityRoot, upstreamHead.slice(0, 9));
const manifestPath = path.join(compatibilityDir, "manifest.json");
if (!fs.existsSync(manifestPath)) {
  throw new Error(`no compatibility manifest for official upstream ${upstreamHead}`);
}
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
if (manifest.upstream_commit !== upstreamHead) {
  throw new Error(`compatibility manifest targets ${manifest.upstream_commit}, not ${upstreamHead}`);
}
validateOfficialPin(manifest);
validatePatches(manifest, compatibilityDir);

const defaultTarget = path.join(
  repoRoot,
  ".cache",
  "llama-pipeline-upstream",
  `${upstreamHead.slice(0, 10)}-${manifest.patch_set_sha256.slice(0, 12)}`,
);
const targetDir = parseOutputPath() ?? defaultTarget;

if (!fs.existsSync(targetDir)) {
  fs.mkdirSync(path.dirname(targetDir), { recursive: true });
  runGit(["worktree", "add", "--detach", targetDir, manifest.upstream_commit], upstreamDir);
  for (const entry of manifest.patches) {
    const patchPath = path.join(compatibilityDir, entry.file);
    runGit(["apply", "--check", "--whitespace=nowarn", patchPath], targetDir);
    runGit(["apply", "--whitespace=nowarn", patchPath], targetDir);
  }
}

validatePreparedSource(targetDir, manifest);
const output = {
  source_dir: targetDir,
  upstream_commit: manifest.upstream_commit,
  compatibility_id: `${upstreamHead.slice(0, 10)}.${manifest.patch_set_sha256}`,
  patch_set_sha256: manifest.patch_set_sha256,
};
process.stdout.write(process.argv.includes("--json") ? `${JSON.stringify(output)}\n` : `${targetDir}\n`);

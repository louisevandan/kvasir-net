#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

// upstream/ and compat/ are siblings of this script inside the adapter that
// owns them, so they are located from here rather than from the repository.
// This shape of the llama.cpp adapter; the clone it patches belongs to the
// backend rather than to one shape of it, so it sits a level up.
const shapeRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const backendRoot = path.resolve(shapeRoot, "..");
const repoRoot = path.resolve(backendRoot, "../../../../..");
const upstreamDir = path.join(backendRoot, "upstream");
const compatibilityRoot = path.join(shapeRoot, "compat");

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
  // Asked of the clone itself rather than of a declaration about it. It used
  // to read .gitmodules, which said what the submodule was supposed to be;
  // upstream/ is now llama.cpp's own repository, so its origin is the fact.
  const origin = runGit(["remote", "get-url", "origin"], upstreamDir).trim();
  if (origin !== "https://github.com/ggml-org/llama.cpp.git") {
    throw new Error(
      `upstream/ must be a clone of the official ggml-org/llama.cpp, not ${origin}`,
    );
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
const explicitTarget = parseOutputPath();
const targetDir = explicitTarget ?? defaultTarget;

function createPreparedSource(destination) {
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  runGit(["worktree", "add", "--detach", destination, manifest.upstream_commit], upstreamDir);
  for (const entry of manifest.patches) {
    const patchPath = path.join(compatibilityDir, entry.file);
    runGit(["apply", "--check", "--whitespace=nowarn", patchPath], destination);
    runGit(["apply", "--whitespace=nowarn", patchPath], destination);
  }
}

function verifiedPreparedSource(destination) {
  if (!fs.existsSync(destination)) createPreparedSource(destination);
  validatePreparedSource(destination, manifest);
  return destination;
}

let preparedTarget;
try {
  preparedTarget = verifiedPreparedSource(targetDir);
} catch (error) {
  // A cache worktree is generated output, not a source of compatibility
  // truth.  Do not delete or reuse a worktree whose diff no longer matches
  // the manifest: it may be useful forensic evidence and building it would
  // make an unrecorded llama.cpp mutation authoritative.  An explicit --out
  // remains strict so callers can ask for an exact path.  The default cache
  // gets one independently reconstructed, manifest-named recovery worktree.
  if (explicitTarget) throw error;
  const recoveredTarget = `${defaultTarget}-verified`;
  try {
    preparedTarget = verifiedPreparedSource(recoveredTarget);
  } catch (recoveryError) {
    throw new Error(
      `default prepared source failed verification (${error.message}); `
      + `independent recovery also failed (${recoveryError.message})`,
    );
  }
}
const output = {
  source_dir: preparedTarget,
  upstream_commit: manifest.upstream_commit,
  compatibility_id: `${upstreamHead.slice(0, 10)}.${manifest.patch_set_sha256}`,
  patch_set_sha256: manifest.patch_set_sha256,
};
process.stdout.write(process.argv.includes("--json") ? `${JSON.stringify(output)}\n` : `${targetDir}\n`);

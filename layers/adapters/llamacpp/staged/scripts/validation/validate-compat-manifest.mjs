#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SHA256 = /^[0-9a-f]{64}$/u;
const COMMIT = /^[0-9a-f]{40}$/u;
const PATCH_FILE = /^(\d{4})-[^/\\]+\.patch$/u;

export const OFFICIAL_REPOSITORY = "https://github.com/ggml-org/llama.cpp.git";

function fail(message) {
  throw new Error(message);
}

function requiredString(value, name) {
  if (typeof value !== "string" || value.trim() === "") {
    fail(`${name} is required`);
  }
  return value;
}

function requiredSha(value, name) {
  requiredString(value, name);
  if (!SHA256.test(value)) fail(`${name} must be a lowercase SHA-256`);
}

function validatePatches(manifest, manifestDir) {
  if (!Array.isArray(manifest.patches) || manifest.patches.length === 0) {
    fail("patches must be a non-empty ordered array");
  }

  const seen = new Set();
  for (const [index, patch] of manifest.patches.entries()) {
    if (!patch || typeof patch !== "object") fail(`patches[${index}] must be an object`);
    const file = requiredString(patch.file, `patches[${index}].file`);
    const match = PATCH_FILE.exec(file);
    if (!match) fail(`patches[${index}].file must use NNNN-name.patch: ${file}`);
    const number = Number(match[1]);
    if (number !== index + 1) {
      fail(`patches must be contiguous and ordered: expected ${String(index + 1).padStart(4, "0")}, got ${match[1]}`);
    }
    if (seen.has(file)) fail(`patches contains a duplicate: ${file}`);
    seen.add(file);

    requiredSha(patch.sha256, `patches[${index}].sha256`);
    const patchPath = path.join(manifestDir, file);
    if (!fs.existsSync(patchPath)) fail(`missing patch: ${file}`);
    const actual = crypto.createHash("sha256").update(fs.readFileSync(patchPath)).digest("hex");
    if (actual !== patch.sha256) fail(`patch SHA-256 mismatch: ${file}`);
  }
}

function validateSymbols(manifest) {
  const symbols = manifest.required_artifact_symbols;
  if (!Array.isArray(symbols) || symbols.length === 0) {
    fail("required_artifact_symbols must be a non-empty array");
  }
  const seen = new Set();
  for (const [index, symbol] of symbols.entries()) {
    if (typeof symbol !== "string" || symbol.trim() === "") {
      fail(`required_artifact_symbols[${index}] must be a non-empty string`);
    }
    if (seen.has(symbol)) fail(`required_artifact_symbols contains a duplicate: ${symbol}`);
    seen.add(symbol);
  }
}

export function validateManifest(manifest, manifestDir) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) {
    fail("manifest must be a JSON object");
  }
  if (manifest.upstream_repository !== OFFICIAL_REPOSITORY) {
    fail("upstream_repository must name the official llama.cpp repository");
  }
  requiredString(manifest.abi_revision, "abi_revision");
  if (!COMMIT.test(manifest.upstream_commit ?? "")) {
    fail("upstream_commit must be a full 40-character lowercase SHA");
  }
  validatePatches(manifest, manifestDir);
  validateSymbols(manifest);
  return {
    upstream_commit: manifest.upstream_commit,
    abi_revision: manifest.abi_revision,
    patch_count: manifest.patches.length,
    required_artifact_symbols: manifest.required_artifact_symbols,
  };
}

export function validateArtifactSymbols(manifest, artifactPath) {
  const artifact = fs.readFileSync(artifactPath);
  const missing = manifest.required_artifact_symbols.filter(
    (symbol) => !artifact.includes(Buffer.from(symbol, "utf8")),
  );
  if (missing.length) fail(`artifact is missing required symbols: ${missing.join(", ")}`);
  return { artifact: artifactPath, symbol_count: manifest.required_artifact_symbols.length };
}

function parseArgs(argv) {
  const manifestIndex = argv.indexOf("--manifest");
  if (manifestIndex < 0 || !argv[manifestIndex + 1]) fail("usage: --manifest <path> [--artifact <path>]");
  const artifactIndex = argv.indexOf("--artifact");
  if (artifactIndex >= 0 && !argv[artifactIndex + 1]) fail("--artifact requires a path");
  return { manifestPath: path.resolve(argv[manifestIndex + 1]), artifactPath: artifactIndex >= 0 ? path.resolve(argv[artifactIndex + 1]) : null };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const { manifestPath, artifactPath } = parseArgs(process.argv.slice(2));
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    const result = validateManifest(manifest, path.dirname(manifestPath));
    if (artifactPath) Object.assign(result, validateArtifactSymbols(manifest, artifactPath));
    process.stdout.write(`${JSON.stringify({ valid: true, ...result })}\n`);
  } catch (error) {
    process.stderr.write(`compat manifest invalid: ${error.message}\n`);
    process.exitCode = 1;
  }
}

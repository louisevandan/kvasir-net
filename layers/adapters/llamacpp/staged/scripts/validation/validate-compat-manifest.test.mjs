import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { validateArtifactSymbols, validateManifest } from "./validate-compat-manifest.mjs";

const root = fs.mkdtempSync(path.join(os.tmpdir(), "linkcpp-compat-validation-"));
const checkedInFixture = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "valid-manifest");

function fixture(overrides = {}) {
  const directory = path.join(root, `case-${Math.random().toString(16).slice(2)}`);
  fs.mkdirSync(directory, { recursive: true });
  const patch = Buffer.from("diff --git a/a b/a\n");
  const patchName = "0001-test.patch";
  fs.writeFileSync(path.join(directory, patchName), patch);
  return {
    directory,
    manifest: {
      schema_version: 1,
      upstream_repository: "https://github.com/ggml-org/llama.cpp.git",
      upstream_commit: "a".repeat(40),
      abi_revision: "pipeline-abi-v1",
      patches: [{ file: patchName, sha256: crypto.createHash("sha256").update(patch).digest("hex") }],
      required_artifact_symbols: ["llama_linkcpp_runtime_configure", "llama_linkcpp_runtime_clear"],
      ...overrides,
    },
  };
}

test("accepts an ordered compat manifest and verifies patch bytes", () => {
  const value = fixture();
  assert.deepEqual(validateManifest(value.manifest, value.directory), {
    upstream_commit: "a".repeat(40),
    abi_revision: "pipeline-abi-v1",
    patch_count: 1,
    required_artifact_symbols: ["llama_linkcpp_runtime_configure", "llama_linkcpp_runtime_clear"],
  });
});

test("accepts the checked-in fixture manifest", () => {
  const manifestPath = path.join(checkedInFixture, "manifest.json");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  assert.equal(validateManifest(manifest, checkedInFixture).patch_count, 1);
});

test("rejects missing ABI and artifact symbol metadata", () => {
  const value = fixture({ abi_revision: undefined, required_artifact_symbols: undefined });
  assert.throws(() => validateManifest(value.manifest, value.directory), /abi_revision is required/u);
});

test("rejects gaps, reordering, and duplicate patches", () => {
  const value = fixture();
  value.manifest.patches = [
    value.manifest.patches[0],
    { file: "0003-gap.patch", sha256: "b".repeat(64) },
  ];
  assert.throws(() => validateManifest(value.manifest, value.directory), /expected 0002, got 0003/u);
});

test("checks required symbols in an artifact without writing it", () => {
  const value = fixture();
  const artifact = path.join(value.directory, "artifact.bin");
  fs.writeFileSync(artifact, Buffer.from("prefix llama_linkcpp_runtime_configure suffix llama_linkcpp_runtime_clear"));
  assert.deepEqual(validateArtifactSymbols(value.manifest, artifact), { artifact, symbol_count: 2 });
});

test("rejects an artifact missing a required symbol", () => {
  const value = fixture();
  const artifact = path.join(value.directory, "artifact.bin");
  fs.writeFileSync(artifact, Buffer.from("llama_linkcpp_runtime_configure"));
  assert.throws(() => validateArtifactSymbols(value.manifest, artifact), /runtime_clear/u);
});

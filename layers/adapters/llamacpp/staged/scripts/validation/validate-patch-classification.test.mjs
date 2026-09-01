import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const gate = path.join(path.dirname(fileURLToPath(import.meta.url)),
  "validate-patch-classification.mjs");

function fixture(patches) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "p4-classify-"));
  const entries = patches.map(({ name, layer, files }) => {
    const body = files.map((f) =>
      `diff --git a/${f} b/${f}\n--- a/${f}\n+++ b/${f}\n@@ -1 +1 @@\n-x\n+y\n`).join("");
    fs.writeFileSync(path.join(dir, name), body);
    return layer === undefined ? { file: name } : { file: name, layer };
  });
  const manifest = path.join(dir, "manifest.json");
  fs.writeFileSync(manifest, JSON.stringify({ patches: entries }, null, 2));
  return manifest;
}

const run = (manifest) =>
  spawnSync(process.execPath, [gate, "--manifest", manifest], { encoding: "utf8" });

test("accepts a correctly classified queue", () => {
  const result = run(fixture([
    { name: "0001-x.patch", layer: "upstream_fix", files: ["ggml/src/ggml-backend.cpp"] },
    { name: "0002-y.patch", layer: "stage_hook", files: ["src/llama-context.cpp"] },
    { name: "0003-z.patch", layer: "model_feature", files: ["common/speculative.cpp"] },
  ]));
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout).by_layer,
    { upstream_fix: 1, stage_hook: 1, model_feature: 1 });
});

test("rejects an unclassified patch", () => {
  const result = run(fixture([{ name: "0001-x.patch", files: ["src/llama-context.cpp"] }]));
  assert.equal(result.status, 1);
  assert.match(result.stderr, /no layer declared/u);
});

test("rejects an unknown class", () => {
  const result = run(fixture([
    { name: "0001-x.patch", layer: "misc", files: ["src/llama-context.cpp"] },
  ]));
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unknown layer/u);
});

// The reason the split exists: a stage hook that reaches into ggml makes the
// whole queue undroppable, because the ggml layer is the part upstream moves
// independently of llama core.
test("rejects a stage hook that reaches below the llama layer", () => {
  const result = run(fixture([
    { name: "0001-x.patch", layer: "stage_hook", files: ["ggml/src/ggml-backend.cpp"] },
  ]));
  assert.equal(result.status, 1);
  assert.match(result.stderr, /out of scope/u);
});

test("rejects a patch that touches nothing", () => {
  const result = run(fixture([{ name: "0001-x.patch", layer: "stage_hook", files: [] }]));
  assert.equal(result.status, 1);
  assert.match(result.stderr, /touches no file/u);
});

test("accepts the checked-in queue for the current pin", () => {
  const manifest = path.resolve(path.dirname(fileURLToPath(import.meta.url)),
    "../../compat/557614e02/manifest.json");
  const result = run(manifest);
  assert.equal(result.status, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.total, 24);
  assert.deepEqual(report.by_layer, { upstream_fix: 2, stage_hook: 18, model_feature: 4 });
});

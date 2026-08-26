import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import {
  officialModelIdentifiers,
  validateCompatibilityPatch,
  validateRuntimeSource,
} from "./model-agnostic-boundary.mjs";

test("generic compatibility capabilities are allowed", () => {
  assert.doesNotThrow(() => validateCompatibilityPatch("0016-memory.patch", [
    "diff --git a/src/llama-memory.h b/src/llama-memory.h",
    "+++ b/src/llama-memory.h",
    "+    virtual bool supports_stage_residency() const { return false; }",
  ].join("\n")));
});

test("model implementation patches are rejected", () => {
  assert.throws(() => validateCompatibilityPatch("model.patch", [
    "diff --git a/src/models/future.cpp b/src/models/future.cpp",
    "+++ b/src/models/future.cpp",
    "+bool has_special_cache = true;",
  ].join("\n")), /update official llama\.cpp instead/u);
});

test("an identified official upstream port may update its model-side graph writer", () => {
  assert.doesNotThrow(() => validateCompatibilityPatch("backport.patch", [
    "diff --git a/src/models/future.cpp b/src/models/future.cpp",
    "+++ b/src/models/future.cpp",
    "+auto state = build_generic_state_carry();",
  ].join("\n"), {
    repository: "https://github.com/ggml-org/llama.cpp.git",
    pull_request: "https://github.com/ggml-org/llama.cpp/pull/25004",
    commit: "7ff2fd83e9794efffd0190652aadd3924820e6e5",
  }));
});

test("official upstream provenance does not permit architecture branches", () => {
  assert.throws(() => validateCompatibilityPatch("backport.patch", [
    "diff --git a/src/models/future.cpp b/src/models/future.cpp",
    "+++ b/src/models/future.cpp",
    "+if (arch == LLM_ARCH_FUTURE) return special_memory();",
  ].join("\n"), {
    repository: "https://github.com/ggml-org/llama.cpp.git",
    pull_request: "https://github.com/ggml-org/llama.cpp/pull/25004",
    commit: "7ff2fd83e9794efffd0190652aadd3924820e6e5",
  }), /adds model or architecture knowledge/u);
});

test("architecture branches in compatibility patches are rejected", () => {
  assert.throws(() => validateCompatibilityPatch("branch.patch", [
    "diff --git a/src/llama-model.cpp b/src/llama-model.cpp",
    "+++ b/src/llama-model.cpp",
    "+if (arch == LLM_ARCH_FUTURE) return special_memory();",
  ].join("\n")), /adds model or architecture knowledge/u);
});

test("runtime code may use public model APIs", () => {
  assert.doesNotThrow(() => validateRuntimeSource(
    "runtime.cpp",
    "const auto n = llama_model_n_layer(model);",
  ));
});

test("runtime code cannot include private model headers or branch on architecture", () => {
  assert.throws(
    () => validateRuntimeSource("runtime.cpp", "#include \"llama-model.h\""),
    /private llama\.cpp header/u,
  );
  assert.throws(
    () => validateRuntimeSource("runtime.cpp", "if (arch == LLM_ARCH_FUTURE) {}"),
    /model or architecture knowledge/u,
  );
});

test("runtime code cannot name model implementations discovered from official upstream", () => {
  assert.throws(
    () => validateRuntimeSource(
      "runtime.cpp",
      "// futuremodel needs a different tensor axis",
      ["futuremodel"],
    ),
    /generic tensor contract/u,
  );
});

test("official model identifiers come from upstream file names, not an adapter list", (context) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "p4-model-boundary-"));
  context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  fs.writeFileSync(path.join(directory, "futuremodel.cpp"), "");
  fs.writeFileSync(path.join(directory, "llama.cpp"), "");
  fs.writeFileSync(path.join(directory, "tiny.cpp"), "");
  fs.writeFileSync(path.join(directory, "README.md"), "");
  assert.deepEqual(officialModelIdentifiers(directory), ["futuremodel"]);
});

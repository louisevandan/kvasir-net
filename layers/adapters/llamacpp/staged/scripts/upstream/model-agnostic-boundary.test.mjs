import assert from "node:assert/strict";
import test from "node:test";
import {
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

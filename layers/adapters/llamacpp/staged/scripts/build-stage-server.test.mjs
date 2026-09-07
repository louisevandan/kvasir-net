import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import vm from "node:vm";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const builder = fs.readFileSync(path.join(scriptDir, "build-stage-server.mjs"), "utf8");
const cmake = fs.readFileSync(path.join(scriptDir, "..", "server", "CMakeLists.txt"), "utf8");

// Execute the production builder, replacing only filesystem/process boundaries.
// Merely mentioning a target in a comment or dead branch cannot pass this test.
function captureBuild(arguments_, source = builder) {
  const calls = [];
  const prepared = path.resolve(scriptDir, "fixture-prepared");
  const fakeFs = {
    existsSync(name) {
      return name === path.join(prepared, "CMakeLists.txt")
        || name === path.resolve("fixture-cmake")
        || name === path.join(path.dirname(path.resolve("fixture-cmake")), "ctest.exe");
    },
    mkdirSync() {},
    copyFileSync() { assert.fail("CPU target selection must not copy CUDA libraries"); },
  };
  const fakeProcess = {
    argv: ["node", "build-stage-server.mjs", "--cmake", path.resolve("fixture-cmake"),
      "--build-dir", path.resolve("fixture-build"), "--parallel", "2", ...arguments_],
    env: {}, execPath: "fixture-node", platform: "linux",
    stdout: { write() {} }, stderr: { write() {} },
    exit(code) { throw new Error(`unexpected builder exit ${code}`); },
  };
  const program = source.replace(/^import .*;\r?\n/gmu, "")
    .replaceAll("import.meta.url", JSON.stringify(pathToFileURL(path.join(scriptDir, "build-stage-server.mjs")).href));
  vm.runInNewContext(program, {
    fs: fakeFs, os: { availableParallelism: () => 2 }, path, process: fakeProcess,
    fileURLToPath,
    spawnSync(command, args) {
      calls.push({ command, args: Array.from(args) });
      return { status: 0, stdout: JSON.stringify({ source_dir: prepared }), stderr: "" };
    },
  }, { filename: "build-stage-server.mjs", timeout: 1000 });
  const build = calls.find((call) => call.args[0] === "--build");
  assert.ok(build, "the production builder must invoke the build command");
  assert.ok(calls.findIndex((call) => call.args[0] === "--test-dir") > calls.indexOf(build),
    "CTest must execute after the build, including when reusing imported libraries");
  return { targets: build.args.slice(build.args.indexOf("--target") + 1), calls };
}

function registeredTests(withLlama) {
  const source = withLlama ? cmake : cmake.split("if(P4_STAGED_BUILD_LLAMA AND P4_STAGED_LLAMA_SOURCE_DIR)")[0];
  return [...source.matchAll(/add_test\(NAME\s+(p4_staged_[a-z0-9_]+_test)\b/gu)].map((match) => match[1]);
}

for (const [mode, args, withLlama] of [
  ["source build", [], true],
  ["imported-library relink", ["--llama-build-dir", "fixture-imported", "--llama-runtime-dir", "fixture-runtime"], true],
  ["without llama", ["--no-llama"], false],
]) {
  test(`official ${mode} builds every registered CTest executable before running CTest`, () => {
    const { targets, calls } = captureBuild(args);
    const expected = ["p4_staged_server", ...registeredTests(withLlama)];
    assert.deepEqual([...targets].sort(), expected.sort());
    assert.equal(new Set(targets).size, targets.length);
    if (mode === "imported-library relink") {
      assert.ok(calls.some((call) => call.args.some((arg) => arg.startsWith("-DP4_STAGED_LLAMA_BUILD_DIR="))));
    }
  });
}

for (const target of ["p4_staged_physical_authority_test", "p4_staged_plan_lifetime_test",
  "p4_staged_physical_logits_consumer_test"]) {
  test(`target-wiring assertion detects ${target} kept only in dead code`, () => {
    const mutant = builder.replace(`  "${target}",`, `  // "${target}" is no longer built`);
    const { targets } = captureBuild([], mutant);
    assert.ok(!targets.includes(target));
    assert.throws(() => assert.deepEqual([...targets].sort(), ["p4_staged_server", ...registeredTests(true)].sort()));
  });
}

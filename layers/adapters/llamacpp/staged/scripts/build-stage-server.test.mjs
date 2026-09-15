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
function captureBuild(arguments_, source = builder, cudaLayout = null, cudaMajor = "13", cudaRuntimeLayout = "x64") {
  const calls = [];
  const copies = [];
  const prepared = path.resolve(scriptDir, "fixture-prepared");
  const cudaRoot = path.resolve("fixture-cuda");
  const buildRoot = path.resolve("fixture-build");
  const runtimeDir = cudaLayout === "multi" ? path.join(buildRoot, "Release") : buildRoot;
  const fakeFs = {
    existsSync(name) {
      if (cudaLayout && (name === path.join(cudaRoot, "bin", "nvcc.exe")
        || name === path.join(cudaRoot, "extras", "visual_studio_integration", "MSBuildExtensions")
        || name === path.join(runtimeDir, "p4_staged_server.exe")
        || [`cublas64_${cudaMajor}.dll`, `cublasLt64_${cudaMajor}.dll`, `cudart64_${cudaMajor}.dll`].some(
          (dll) => name === path.join(cudaRoot, "bin", ...(cudaRuntimeLayout === "x64" ? ["x64"] : []), dll)))) return true;
      return name === path.join(prepared, "CMakeLists.txt")
        || name === path.resolve("fixture-cmake")
        || name === path.join(path.dirname(path.resolve("fixture-cmake")), "ctest.exe");
    },
    mkdirSync() {},
    copyFileSync(from, to) {
      assert.ok(cudaLayout, "CPU target selection must not copy CUDA libraries");
      copies.push({ from, to });
    },
  };
  const fakeProcess = {
    argv: ["node", "build-stage-server.mjs", "--cmake", path.resolve("fixture-cmake"),
      "--build-dir", path.resolve("fixture-build"), "--parallel", "2", ...arguments_],
    env: {}, execPath: "fixture-node", platform: cudaLayout ? "win32" : "linux",
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
  return { targets: build.args.slice(build.args.indexOf("--target") + 1), calls, copies };
}

function registeredTests(withLlama) {
  const source = withLlama ? cmake : cmake.split("if(P4_STAGED_BUILD_LLAMA AND P4_STAGED_LLAMA_SOURCE_DIR)")[0];
  return [...source.matchAll(/add_test\(NAME\s+(p4_staged_[a-z0-9_]+_test)\b/gu)].map((match) => match[1]);
}

for (const [name, args, expected] of [
  ["default Release", [], "Release"],
  ["explicit Debug", ["--config", "Debug"], "Debug"],
]) {
  test(`Ninja ${name} is selected during configure as well as build and CTest`, () => {
    const { calls } = captureBuild(["--generator", "Ninja", ...args]);
    const configure = calls.find((call) => call.args[0] === "-S");
    assert.ok(configure.args.includes(`-DCMAKE_BUILD_TYPE=${expected}`));
    const build = calls.find((call) => call.args[0] === "--build");
    assert.equal(build.args[build.args.indexOf("--config") + 1], expected);
    const ctest = calls.find((call) => call.args[0] === "--test-dir");
    assert.equal(ctest.args[ctest.args.indexOf("-C") + 1], expected);
  });
}

for (const [layout, cudaMajor, cudaRuntimeLayout] of [
  ["single", "12", "bin"], ["single", "12", "x64"],
  ["single", "13", "bin"], ["single", "13", "x64"],
  ["multi", "12", "bin"], ["multi", "12", "x64"],
  ["multi", "13", "bin"], ["multi", "13", "x64"],
]) {
  test(`CUDA ${cudaMajor} ${cudaRuntimeLayout} DLLs are copied beside the actual ${layout}-configuration executable`, () => {
    const { copies } = captureBuild([
      "--cuda", "--cuda-root", path.resolve("fixture-cuda"),
      "--generator", layout === "single" ? "Ninja" : "Visual Studio 17 2022",
    ], builder, layout, cudaMajor, cudaRuntimeLayout);
    const destination = path.resolve("fixture-build", ...(layout === "multi" ? ["Release"] : []));
    assert.deepEqual(copies.map((copy) => copy.to).sort(),
      [`cublas64_${cudaMajor}.dll`, `cublasLt64_${cudaMajor}.dll`, `cudart64_${cudaMajor}.dll`]
        .map((dll) => path.join(destination, dll)).sort());
  });
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

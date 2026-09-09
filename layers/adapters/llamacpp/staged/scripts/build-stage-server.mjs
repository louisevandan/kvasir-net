#!/usr/bin/env node

// Configure and build the staged C++ server against a prepared compatibility
// worktree. This script owns no upstream source and never changes the official
// checkout; preparation is delegated to prepare-pipeline-upstream.mjs.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const stagedRoot = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(stagedRoot, "../../../../../../");
const serverDir = path.join(stagedRoot, "server");
const defaultBuildDir = path.join(repoRoot, ".cache", "staged-server-build");
const noLlama = process.argv.includes("--no-llama");
const backend = process.argv.includes("--cuda")
  ? "cuda"
  : argument("--backend", "cpu").toLowerCase();
const supportedBackends = new Set(["cpu", "cuda", "vulkan", "hip", "metal", "opencl"]);
if (!supportedBackends.has(backend)) {
  throw new Error(`--backend must be one of ${[...supportedBackends].join(", ")}`);
}
const parallel = positiveIntegerArgument(
  "--parallel", Math.max(1, Math.min(4, os.availableParallelism()))
);

function argument(name, fallback) {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

function positiveIntegerArgument(name, fallback) {
  const value = Number(argument(name, String(fallback)));
  if (!Number.isInteger(value) || value < 1) {
    throw new Error(`${name} must be a positive integer`);
  }
  return value;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    stdio: "inherit",
    windowsHide: true,
    ...options,
  });
  if (result.error) {
    throw new Error(`${command} could not be executed: ${result.error.message}`);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function executable(pathOrName) {
  if (path.isAbsolute(pathOrName) && fs.existsSync(pathOrName)) return pathOrName;
  const probe = spawnSync(pathOrName, ["--version"], {
    cwd: repoRoot,
    stdio: "ignore",
    windowsHide: true,
  });
  return probe.error ? null : pathOrName;
}

function visualStudioInstall() {
  const candidates = [
    "C:\\Program Files (x86)\\Microsoft Visual Studio\\Installer\\vswhere.exe",
    "C:\\Program Files\\Microsoft Visual Studio\\Installer\\vswhere.exe",
  ];
  const vswhere = candidates.find((candidate) => fs.existsSync(candidate));
  if (!vswhere) return null;
  const result = spawnSync(vswhere, [
    "-latest", "-products", "*",
    "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
    "-format", "json",
  ], {
    encoding: "utf8",
    windowsHide: true,
  });
  if (result.status !== 0) return null;
  try {
    return JSON.parse(result.stdout)?.[0]?.installationPath ?? null;
  } catch {
    return null;
  }
}

function importVisualStudioEnvironment(install) {
  if (process.platform !== "win32" || !install) return;
  const devCmd = path.join(install, "Common7", "Tools", "VsDevCmd.bat");
  if (!fs.existsSync(devCmd)) throw new Error(`Visual Studio developer environment is missing: ${devCmd}`);
  // Invoke through an explicit relative path from its own directory. Node's
  // Windows argv quoting escapes embedded quotes before cmd.exe sees them,
  // which breaks a quoted absolute path containing spaces. A bare basename
  // does not resolve either: an environment that sets
  // NoDefaultCurrentDirectoryInExePath drops the current directory from
  // cmd.exe command resolution, and this machine sets it. A leading `.\`
  // names that directory outright and needs neither.
  const command = ".\\VsDevCmd.bat -arch=x64 -host_arch=x64 >nul && set";
  const result = spawnSync(process.env.ComSpec ?? "cmd.exe", ["/d", "/c", command], {
    cwd: path.dirname(devCmd),
    encoding: "utf8",
    windowsHide: true,
  });
  if (result.status !== 0) throw new Error(`Visual Studio developer environment initialization failed: ${result.stderr ?? ""}`);
  for (const line of result.stdout.split(/\r?\n/u)) {
    const separator = line.indexOf("=");
    if (separator > 0) process.env[line.slice(0, separator)] = line.slice(separator + 1);
  }
}

function resolveCudaRoot() {
  const requested = argument("--cuda-root", process.env.CUDA_PATH ?? "");
  const nvccFromPath = executable(process.platform === "win32" ? "nvcc.exe" : "nvcc");
  const nvcc = requested
    ? path.join(path.resolve(requested), "bin", process.platform === "win32" ? "nvcc.exe" : "nvcc")
    : nvccFromPath;
  if (!nvcc || !fs.existsSync(nvcc)) {
    throw new Error("CUDA validation requires nvcc; pass --cuda-root <CUDA toolkit root> or set CUDA_PATH.");
  }
  const root = path.dirname(path.dirname(nvcc));
  if (process.platform === "win32") {
    const integration = path.join(root, "extras", "visual_studio_integration", "MSBuildExtensions");
    if (!fs.existsSync(integration)) {
      throw new Error(`CUDA Visual Studio integration files are missing: ${integration}`);
    }
  }
  return { root, nvcc };
}

function copyCudaRuntimeDependencies(cuda, buildDir, config) {
  if (process.platform !== "win32" || !cuda) return [];
  const sourceDir = path.join(cuda.root, "bin", "x64");
  const destinationDir = [path.join(buildDir, config), buildDir, path.join(buildDir, "bin")]
    .find((candidate) => fs.existsSync(path.join(candidate, "p4_staged_server.exe")));
  if (!destinationDir) throw new Error("built stage executable was not found for CUDA runtime deployment");
  const names = ["cublas64_13.dll", "cublasLt64_13.dll", "cudart64_13.dll"];
  const copied = [];
  for (const name of names) {
    const source = path.join(sourceDir, name);
    if (!fs.existsSync(source)) {
      throw new Error(`CUDA runtime dependency is missing: ${source}`);
    }
    const destination = path.join(destinationDir, name);
    fs.copyFileSync(source, destination);
    copied.push(destination);
  }
  return copied;
}

function resolveCMake() {
  const requested = argument("--cmake", process.env.CMAKE ?? "cmake");
  const direct = executable(requested);
  if (direct) return direct;
  const install = visualStudioInstall();
  const candidates = [];
  if (install) {
    candidates.push(path.join(
      install, "Common7", "IDE", "CommonExtensions", "Microsoft", "CMake", "CMake", "bin", "cmake.exe",
    ));
  }
  candidates.push(
    "C:\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe",
  );
  const found = candidates.find((candidate) => fs.existsSync(candidate));
  if (found) return found;
  throw new Error([
    "CMake was not found on PATH or in the installed Visual Studio Build Tools.",
    `requested: ${requested}`,
    "install CMake or pass --cmake <path-to-cmake.exe>.",
  ].join("\n"));
}

function resolveCTest(cmake) {
  const sibling = path.join(path.dirname(cmake), "ctest.exe");
  if (fs.existsSync(sibling)) return sibling;
  const found = executable("ctest");
  if (found) return found;
  throw new Error("CTest was not found; install the CMake component or pass a complete CMake installation.");
}

function defaultGenerator(cmake, buildDir) {
  if (fs.existsSync(path.join(buildDir, "CMakeCache.txt"))) return null;
  const install = visualStudioInstall();
  if (install && path.isAbsolute(cmake)) return "Visual Studio 17 2022";
  return null;
}

const buildDir = path.resolve(argument("--build-dir", defaultBuildDir));
const cuda = backend === "cuda" ? resolveCudaRoot() : null;
const cudaArchitectures = argument("--cuda-architectures", "75;89");
const llamaBuildDir = argument("--llama-build-dir", "");
const llamaRuntimeDir = argument("--llama-runtime-dir", "");
const visualStudio = visualStudioInstall();
let prepared = null;
if (!noLlama) {
  const prepare = spawnSync(process.execPath, [
    path.join(stagedRoot, "scripts", "prepare-pipeline-upstream.mjs"), "--json",
  ], { cwd: repoRoot, encoding: "utf8" });
  if (prepare.status !== 0) {
    process.stderr.write(prepare.stderr || "failed to prepare patched llama.cpp\n");
    process.exit(prepare.status ?? 1);
  }
  prepared = JSON.parse(prepare.stdout.trim());
  if (!fs.existsSync(path.join(prepared.source_dir, "CMakeLists.txt"))) {
    throw new Error(`prepared source is not a CMake tree: ${prepared.source_dir}`);
  }
}
fs.mkdirSync(buildDir, { recursive: true });

const cmake = resolveCMake();
importVisualStudioEnvironment(visualStudio);
const config = argument("--config", "Release");
const configureArgs = [
  "-S", serverDir,
  "-B", buildDir,
  `-DP4_STAGED_BUILD_LLAMA=${noLlama ? "OFF" : "ON"}`,
  `-DCMAKE_BUILD_TYPE=${config}`,
];
const generator = argument("--generator", defaultGenerator(cmake, buildDir));
if (generator) {
  configureArgs.push("-G", generator);
  if (generator.startsWith("Visual Studio")) configureArgs.push("-A", argument("--platform", "x64"));
}
if (cuda) {
  if (noLlama) throw new Error("--cuda requires the llama stage; remove --no-llama");
  configureArgs.push(
    "-DP4_STAGED_CUDA=ON",
    "-DGGML_CUDA=ON",
    `-DCMAKE_CUDA_ARCHITECTURES=${cudaArchitectures}`,
  );
  if (generator?.startsWith("Visual Studio")) {
    const hostToolset = argument("--toolset", "v143");
    configureArgs.push("-T", `${hostToolset},cuda=${cuda.root}`);
  } else {
    configureArgs.push(`-DCMAKE_CUDA_COMPILER=${cuda.nvcc}`);
  }
} else {
  configureArgs.push("-DP4_STAGED_CUDA=OFF");
}
for (const [name, option] of [
  ["cuda", "GGML_CUDA"],
  ["vulkan", "GGML_VULKAN"],
  ["hip", "GGML_HIP"],
  ["metal", "GGML_METAL"],
  ["opencl", "GGML_OPENCL"],
]) {
  configureArgs.push(`-D${option}=${backend === name ? "ON" : "OFF"}`);
}
if (prepared) configureArgs.push(`-DP4_STAGED_LLAMA_SOURCE_DIR=${prepared.source_dir}`);
if (llamaBuildDir) configureArgs.push(`-DP4_STAGED_LLAMA_BUILD_DIR=${path.resolve(llamaBuildDir)}`);
if (llamaRuntimeDir) configureArgs.push(`-DP4_STAGED_LLAMA_RUNTIME_DIR=${path.resolve(llamaRuntimeDir)}`);
run(cmake, configureArgs);
const targets = [
  "p4_staged_server",
  "p4_staged_server_test",
  "p4_staged_protocol_test",
  "p4_staged_runtime_test",
  "p4_staged_state_store_test",
  "p4_staged_physical_authority_test",
];
if (!noLlama) targets.push(
  "p4_staged_llama_runtime_compile_test",
  "p4_staged_physical_wire_test",
  "p4_staged_physical_logits_consumer_test",
  "p4_staged_request_options_test",
  "p4_staged_plan_lifetime_test",
  "p4_staged_request_stops_test",
  "p4_staged_utf8_boundary_test",
  "p4_staged_capability_test",
  "p4_staged_plan_invariants_test",
  "p4_staged_mtp_ownership_test",
);
run(cmake, [
  "--build", buildDir, "--config", config,
  "--parallel", String(parallel), "--target", ...targets,
]);
const runtimeDependencies = copyCudaRuntimeDependencies(cuda, buildDir, config);
const ctest = resolveCTest(cmake);
run(ctest, ["--test-dir", buildDir, "-C", config, "--output-on-failure"]);
process.stdout.write(`${JSON.stringify({
  build_dir: buildDir,
  source_dir: prepared?.source_dir ?? null,
  llama: !noLlama,
  cmake,
  generator: generator ?? "existing-cache",
  backend,
  cuda: Boolean(cuda),
  cuda_root: cuda?.root ?? null,
  cuda_architectures: cuda ? cudaArchitectures : null,
  parallel,
  runtime_dependencies: runtimeDependencies,
})}\n`);

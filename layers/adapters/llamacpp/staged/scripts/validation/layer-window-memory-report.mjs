#!/usr/bin/env node

// Read-only GGUF and staged-runtime preflight. This never calls llama_model_load.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GIB = 1024 ** 3;
const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = findRepoRoot(scriptDir);

function findRepoRoot(start) {
  let current = path.resolve(start);
  while (current !== path.dirname(current)) {
    if (fs.existsSync(path.join(current, "package.json")) &&
        fs.existsSync(path.join(current, "apps"))) return current;
    current = path.dirname(current);
  }
  throw new Error("could not locate repository root");
}

function values(argv, name) {
  const result = [];
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === name && argv[index + 1]) result.push(argv[++index]);
  }
  return result;
}

function value(argv, name, fallback) {
  return values(argv, name)[0] ?? fallback;
}

function parseBudgets(argv) {
  const raw = values(argv, "--budget");
  const entries = raw.length ? raw : ["3090=23", "4080=11"];
  return entries.map((entry) => {
    const match = /^(.*)=(\d+(?:\.\d+)?)$/u.exec(entry);
    if (!match || Number(match[2]) <= 0) throw new Error(`invalid --budget: ${entry}`);
    return { name: match[1], budgetGiB: Number(match[2]) };
  });
}

function resolveModelPaths(argv) {
  const explicit = [...values(argv, "--model"), ...values(argv, "--shard")];
  if (explicit.length) return explicit.map((item) => path.resolve(item));
  const directory = path.resolve(value(argv, "--models-dir", "S:\\models"));
  if (!fs.existsSync(directory)) throw new Error(`models directory not found: ${directory}`);
  const files = fs.readdirSync(directory)
    .filter((name) => name.toLowerCase().endsWith(".gguf"))
    .filter((name) => !name.toLowerCase().includes("mmproj"))
    .sort()
    .map((name) => path.join(directory, name));
  if (!files.length) throw new Error(`no GGUF files found in ${directory}`);
  if (files.length > 1 && !value(argv, "--model-name", "")) {
    throw new Error(`multiple GGUF files found; pass --model <primary> and --shard <shard> or --model-name`);
  }
  const selected = value(argv, "--model-name", "");
  return selected ? files.filter((file) => path.basename(file).startsWith(selected)) : files;
}

function parseCache(cachePath) {
  if (!fs.existsSync(cachePath)) return null;
  const cache = {};
  for (const line of fs.readFileSync(cachePath, "utf8").split(/\r?\n/u)) {
    const match = /^([^#=]+):[^=]*=(.*)$/u.exec(line);
    if (match) cache[match[1]] = match[2];
  }
  return cache;
}

function runtimeCapability(argv) {
  const buildDir = path.resolve(value(argv, "--build-dir", ".cache/staged-server-llama"));
  const cache = parseCache(path.join(buildDir, "CMakeCache.txt"));
  const cmakePath = path.join(repoRoot, "apps/p4/layers/adapters/llamacpp/staged/server/CMakeLists.txt");
  const runtimePath = path.join(repoRoot, "apps/p4/layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime.cpp");
  const cmake = fs.readFileSync(cmakePath, "utf8");
  const runtime = fs.readFileSync(runtimePath, "utf8");
  const executable = [
    path.join(buildDir, "Release", "p4_staged_server.exe"),
    path.join(buildDir, "p4_staged_server.exe"),
  ].find((file) => fs.existsSync(file)) ?? null;
  const prepared = cache?.P4_STAGED_LLAMA_SOURCE_DIR;
  return {
    build_dir: buildDir,
    cache_present: cache !== null,
    cmake_generator: cache?.CMAKE_GENERATOR ?? null,
    p4_staged_build_llama: cache?.P4_STAGED_BUILD_LLAMA ?? null,
    ggml_cuda: cache?.GGML_CUDA ?? null,
    prepared_source_dir: prepared ?? null,
    prepared_source_present: Boolean(prepared && fs.existsSync(prepared)),
    executable,
    executable_present: executable !== null,
    source_contract: {
      embeds_llama_when_enabled: cmake.includes("add_subdirectory") && cmake.includes("P4_STAGED_BUILD_LLAMA"),
      stage_runtime_target: cmake.includes("p4_staged_llama_runtime"),
      plan_only_option: fs.readFileSync(path.join(repoRoot, "apps/p4/layers/adapters/llamacpp/staged/server/src/main.cpp"), "utf8").includes("--validate-plan"),
      model_load_entrypoint: runtime.includes("llama_model_load_from_file"),
      layer_window_hooks: runtime.includes("linkcpp_layer_begin") && runtime.includes("linkcpp_layer_end"),
      unload_path: runtime.includes("llama_model_free") && runtime.includes("llama_backend_free"),
    },
    evidence_scope: "CMake cache/source and artifact presence; not proof of CUDA execution or VRAM residency",
  };
}

function metadataObject(index) {
  return Object.fromEntries(index.metadata.entries());
}

function chooseSplit(layerBytes, budgets, boundaryBytes, boundaryOwner) {
  if (budgets.length !== 2) return null;
  let best = null;
  for (let split = 1; split < layerBytes.length; split += 1) {
    const leftLayers = layerBytes.slice(0, split).reduce((sum, value) => sum + value, 0);
    const rightLayers = layerBytes.slice(split).reduce((sum, value) => sum + value, 0);
    const required = [leftLayers, rightLayers];
    if (boundaryOwner === "first") required[0] += boundaryBytes;
    if (boundaryOwner === "last") required[1] += boundaryBytes;
    const fits = required.every((bytes, index) => bytes <= budgets[index].budgetGiB * GIB);
    const score = Math.max(...required.map((bytes, index) => bytes / (budgets[index].budgetGiB * GIB)));
    if (best === null || (fits && !best.fits) || (fits === best.fits && score < best.score)) {
      best = { split, required, fits, score };
    }
  }
  return best;
}

function reportWindows(layerBytes, budgets, boundaryBytes, boundaryOwner) {
  const split = chooseSplit(layerBytes, budgets, boundaryBytes, boundaryOwner);
  if (!split) return { status: "not-computed", reason: "exactly two --budget entries are required" };
  return {
    status: split.fits ? "fits_tensor_bytes" : "does_not_fit_tensor_bytes",
    boundary_owner: boundaryOwner,
    split_after_layer: split.split,
    windows: budgets.map((budget, index) => {
      const begin = index === 0 ? 0 : split.split;
      const end = index === 0 ? split.split : layerBytes.length;
      const layerBytesInWindow = split.required[index] - (boundaryOwner === "first" && index === 0 ? boundaryBytes : 0) - (boundaryOwner === "last" && index === 1 ? boundaryBytes : 0);
      return {
        device: budget.name,
        layer_begin: begin,
        layer_end_exclusive: end,
        layer_bytes: layerBytesInWindow,
        layer_gib: round(layerBytesInWindow / GIB),
        boundary_bytes_included: (boundaryOwner === "first" && index === 0) || (boundaryOwner === "last" && index === 1),
        required_bytes: split.required[index],
        required_gib: round(split.required[index] / GIB),
        budget_gib: budget.budgetGiB,
        headroom_gib: round(budget.budgetGiB - split.required[index] / GIB),
        fits: split.required[index] <= budget.budgetGiB * GIB,
      };
    }),
  };
}

function round(value) {
  return Number(value.toFixed(4));
}

export async function buildReport(argv = process.argv.slice(2)) {
  const modelPaths = resolveModelPaths(argv);
  const { readGgufIndex, plannerModelFromIndexes } = await import("llama_domain/server");
  const indexes = await Promise.all(modelPaths.map((file) => readGgufIndex(file)));
  const model = plannerModelFromIndexes(indexes);
  const budgets = parseBudgets(argv);
  const boundaryOwner = value(argv, "--boundary-owner", "none");
  if (!["none", "first", "last"].includes(boundaryOwner)) throw new Error("--boundary-owner must be none, first, or last");
  const layerBytes = model.weightLayer;
  return {
    status: "ok",
    generated_at: new Date().toISOString(),
    model: {
      primary: modelPaths[0],
      shards: indexes.map((index) => ({
        path: index.filePath,
        bytes: index.fileSize,
        gib: round(index.fileSize / GIB),
        gguf_version: index.version,
        metadata_count: index.metadataCount,
        tensor_count: index.tensors.length,
        data_offset: index.dataOffset,
      })),
      metadata: metadataObject(indexes[0]),
      architecture: model.arch,
      layers: model.nLayer,
      embedding_length: model.nEmbd,
      experts: model.nExpert,
      tensor_bytes: {
        total: model.totalWeight,
        total_gib: round(model.totalWeight / GIB),
        boundary: model.boundaryBytes,
        boundary_gib: round(model.boundaryBytes / GIB),
        transformer_layers: layerBytes.reduce((sum, value) => sum + value, 0),
        transformer_layers_gib: round(layerBytes.reduce((sum, value) => sum + value, 0) / GIB),
      },
      layer_bytes: layerBytes.map((bytes, layer) => ({ layer, bytes, gib: round(bytes / GIB) })),
    },
    constraints: {
      budgets,
      boundary_owner: boundaryOwner,
      windows: reportWindows(layerBytes, budgets, model.boundaryBytes, boundaryOwner),
      measurement_scope: "GGUF tensor bytes only; excludes KV cache, compute buffers, allocator fragmentation, CUDA context, and runtime overhead",
    },
    runtime: runtimeCapability(argv),
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const report = await buildReport();
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  } catch (error) {
    process.stderr.write(`layer-window report failed: ${error.message}\n`);
    process.exitCode = 1;
  }
}

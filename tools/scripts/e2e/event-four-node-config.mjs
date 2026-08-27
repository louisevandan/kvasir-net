import fs from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

const TCP_ADDRESS = /^tcp:\/\/[^\s/:]+:\d{1,5}$/;
const ENDPOINT = /^[^\s:]+:\d{1,5}$/;

export async function buildEventFourNodeConfig(spec, specPath = process.cwd()) {
  requireObject(spec, "spec");
  const requestCount = positiveInteger(spec.request_count, "request_count");
  const parallel = positiveInteger(spec.parallel, "parallel");
  const contextSize = positiveInteger(spec.context_size, "context_size");
  const nBatch = positiveInteger(spec.n_batch, "n_batch");
  const nUbatch = positiveInteger(spec.n_ubatch, "n_ubatch");
  const speculativeType = spec.speculative_type ?? "none";
  if (!["none", "draft-mtp"].includes(speculativeType)) {
    throw new Error("speculative_type must be none or draft-mtp");
  }
  if (nUbatch > nBatch) throw new Error("n_ubatch cannot exceed n_batch");
  if (!TCP_ADDRESS.test(spec.ingress_agent ?? "")) {
    throw new Error("ingress_agent must be tcp://HOST:PORT");
  }
  if (!Array.isArray(spec.stages) || spec.stages.length !== 4) {
    throw new Error("stages must contain exactly four entries");
  }
  const waves = validateWaves(spec.waves, requestCount);
  const base = path.dirname(path.resolve(specPath));
  const prompts = await readArray(base, spec.prompts_file, "prompts_file");
  const responses = await readArray(base, spec.responses_file, "responses_file");
  if (prompts.length < requestCount || responses.length < requestCount) {
    throw new Error("prompt and response fixtures must cover every request");
  }
  if (prompts.slice(0, requestCount).some((value) => typeof value !== "string" || !value)) {
    throw new Error("every selected prompt must be a non-empty string");
  }
  responses.slice(0, requestCount).forEach((value, index) => {
    requireObject(value, `responses[${index}]`);
  });
  const totalContextSize = contextSize * parallel;
  if (!Number.isSafeInteger(totalContextSize)) throw new Error("total context size overflow");
  const placementDocument = JSON.parse(
    await fs.readFile(resolveFrom(base, spec.placement_file), "utf8"),
  );
  const placement = validatePlacement(
    placementDocument.plan ?? placementDocument,
    totalContextSize,
    parallel,
  );
  const options = JSON.parse(await fs.readFile(resolveFrom(base, spec.options_file), "utf8"));
  requireObject(options, "options_file");

  const nodes = placement.map((stagePlacement, index) => {
    const stage = spec.stages[stagePlacement.nodeIndex];
    requireObject(stage, `stages[${stagePlacement.nodeIndex}]`);
    if (!TCP_ADDRESS.test(stage.agent ?? "")) {
      throw new Error(`stages[${stagePlacement.nodeIndex}].agent must be tcp://HOST:PORT`);
    }
    if (!ENDPOINT.test(stage.endpoint ?? "")) {
      throw new Error(`stages[${stagePlacement.nodeIndex}].endpoint must be HOST:PORT`);
    }
    for (const field of ["node", "binary", "model", "cuda_visible_devices"]) {
      if (typeof stage[field] !== "string" || !stage[field]) {
        throw new Error(
          `stages[${stagePlacement.nodeIndex}].${field} must be a non-empty string`,
        );
      }
    }
    return {
      agent: stage.agent,
      node: stage.node,
      generation: positiveInteger(spec.node_generation ?? 1, "node_generation"),
      binary: stage.binary,
      endpoint: stage.endpoint,
      plan: buildPlan({
        model: stage.model,
        placement: stagePlacement,
        allPlacement: placement,
        parallel,
        nBatch,
        nUbatch,
        totalContextSize,
        flashAttention: spec.flash_attention === true,
        speculativeType,
      }),
      args: [],
      environment: [
        ["CUDA_VISIBLE_DEVICES", stage.cuda_visible_devices],
        ["P4_STAGED_TRACE_HELLO", "1"],
      ],
      n_batch: nBatch,
      n_ubatch: nUbatch,
      context_size: contextSize,
      total_context_size: totalContextSize,
      sequence_capacity: parallel,
    };
  });

  return {
    ingress_agent: spec.ingress_agent,
    channel: requireText(spec.channel, "channel"),
    connection_generation: positiveInteger(spec.connection_generation ?? 1, "connection_generation"),
    load_generation: positiveInteger(spec.load_generation ?? 1, "load_generation"),
    session_id: requireText(spec.session_id, "session_id"),
    request_id: requireText(spec.request_id, "request_id"),
    nodes,
    prompts: prompts.slice(0, requestCount),
    max_tokens: positiveInteger(spec.max_tokens, "max_tokens"),
    waves,
    options: JSON.stringify(options),
    pre_inference_hold_ms: positiveInteger(
      spec.pre_inference_hold_ms ?? 30_000,
      "pre_inference_hold_ms",
    ),
    acceptance: {
      minimum_generated_tokens: positiveInteger(
        spec.minimum_generated_tokens ?? 180,
        "minimum_generated_tokens",
      ),
      expected_prefill_rows: positiveInteger(
        spec.expected_prefill_rows ?? 500,
        "expected_prefill_rows",
      ),
      allowed_stop_reasons: spec.allowed_stop_reasons ?? ["eos", "length"],
      responses: responses.slice(0, requestCount),
    },
    timeout_ms: positiveInteger(spec.timeout_ms ?? 3_600_000, "timeout_ms"),
  };
}

function buildPlan(input) {
  const { begin, end, tensorOverride } = input.placement;
  const totalLayers = input.allPlacement.at(-1).end;
  const unowned = [];
  for (let layer = 0; layer < totalLayers; layer += 1) {
    if (layer < begin || layer >= end) unowned.push(layer);
  }
  const overrides = [];
  if (tensorOverride) overrides.push(tensorOverride);
  if (unowned.length) overrides.push(`blk\\.(${unowned.join("|")})\\..*=CPU`);
  const tokens = [
    "--model", quote(input.model),
    "--memory-topology", "discrete",
    "--layer-begin", `${begin}`,
    "--layer-end", `${end}`,
    "--kv-layer-begin", `${begin}`,
    "--kv-layer-end", `${end}`,
    "--n-seq-max", `${input.parallel}`,
    "--spec-type", input.speculativeType,
    // This OUTER plan deliberately selects llama.cpp's unified cache. It lets
    // split_simple() pack unequal Prefill and Decode contributions into the
    // same physical UBATCH; physical-v2 then forwards that exact membership
    // to every downstream stage. This is a batching policy, not a model-load
    // workaround.
    "--kv-unified",
    "--batch-size", `${input.nBatch}`,
    "--ubatch-size", `${input.nUbatch}`,
    "--ctx-size", `${input.totalContextSize}`,
    "--n-gpu-layers", `${totalLayers - begin}`,
    "--device", "CUDA0",
    "--flash-attn", input.flashAttention ? "on" : "off",
    "--no-mmap",
    "--cache-type-k", "q8_0",
    // Current llama.cpp requires flash attention for every quantized V cache.
    // Keep the non-flash form portable across backends by using F16 rather
    // than emitting a plan that can only fail after loading the model.
    "--cache-type-v", input.flashAttention ? "q8_0" : "f16",
  ];
  if (overrides.length) tokens.push("--override-tensor", quote(overrides.join(",")));
  return tokens.join(" ");
}

function validatePlacement(plan, contextSize, parallel) {
  requireObject(plan, "placement plan");
  if (plan.n_ctx !== contextSize || plan.n_parallel !== parallel) {
    throw new Error(
      `placement capacity mismatch: expected n_ctx=${contextSize} n_parallel=${parallel}`,
    );
  }
  if (plan.feasible !== true || !Array.isArray(plan.placement) || plan.placement.length !== 4) {
    throw new Error("placement plan must be feasible and contain four stages");
  }
  const entries = [...plan.placement].sort((a, b) => a.stage_index - b.stage_index);
  let expectedBegin = 0;
  const nodeIndexes = new Set();
  const result = entries.map((entry, index) => {
    requireObject(entry, `placement[${index}]`);
    if (entry.stage_index !== index || !Array.isArray(entry.layers) || entry.layers.length !== 2
        || !Number.isInteger(entry.node) || entry.node < 0 || entry.node >= entries.length) {
      throw new Error("placement stages must be indexed 0..3 with one layer range");
    }
    if (nodeIndexes.has(entry.node)) throw new Error("placement node indexes must be unique");
    nodeIndexes.add(entry.node);
    const [begin, end] = entry.layers;
    if (!Number.isInteger(begin) || !Number.isInteger(end) || begin !== expectedBegin || end <= begin) {
      throw new Error("placement layer ranges must be positive and contiguous from zero");
    }
    if (entry.n_layers !== end - begin) throw new Error("placement n_layers mismatches its range");
    expectedBegin = end;
    return { begin, end, tensorOverride: entry.ot ?? "", nodeIndex: entry.node };
  });
  if (Array.isArray(plan.stage_node_indexes)
      && (plan.stage_node_indexes.length !== result.length
        || plan.stage_node_indexes.some((node, index) => node !== result[index].nodeIndex))) {
    throw new Error("placement stage_node_indexes mismatch placement node mapping");
  }
  return result;
}

function validateWaves(waves, requestCount) {
  if (!Array.isArray(waves) || waves.length === 0 || waves[0].after_ms !== 0) {
    throw new Error("waves must start at zero");
  }
  let prior = -1;
  let total = 0;
  for (const [index, wave] of waves.entries()) {
    requireObject(wave, `waves[${index}]`);
    if (!Number.isInteger(wave.after_ms) || wave.after_ms <= prior) {
      throw new Error("wave times must be strictly increasing non-negative integers");
    }
    total += positiveInteger(wave.count, `waves[${index}].count`);
    prior = wave.after_ms;
  }
  if (total !== requestCount) throw new Error("wave request total does not match request_count");
  return waves;
}

async function readArray(base, file, name) {
  const value = JSON.parse(await fs.readFile(resolveFrom(base, requireText(file, name)), "utf8"));
  if (!Array.isArray(value)) throw new Error(`${name} must contain a JSON array`);
  return value;
}

function resolveFrom(base, value) {
  return path.isAbsolute(value) ? value : path.resolve(base, value);
}

function quote(value) {
  if (value.includes('"')) throw new Error("plan values cannot contain a double quote");
  return `"${value}"`;
}

function requireObject(value, name) {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${name} must be an object`);
  }
}

function requireText(value, name) {
  if (typeof value !== "string" || !value) throw new Error(`${name} must be a non-empty string`);
  return value;
}

function positiveInteger(value, name) {
  if (!Number.isSafeInteger(value) || value <= 0) throw new Error(`${name} must be a positive integer`);
  return value;
}

async function main() {
  const [specFile, outputFile] = process.argv.slice(2);
  if (!specFile || !outputFile) throw new Error("usage: node event-four-node-config.mjs SPEC OUTPUT");
  const specPath = path.resolve(specFile);
  const spec = JSON.parse(await fs.readFile(specPath, "utf8"));
  const config = await buildEventFourNodeConfig(spec, specPath);
  await fs.mkdir(path.dirname(path.resolve(outputFile)), { recursive: true });
  await fs.writeFile(path.resolve(outputFile), `${JSON.stringify(config, null, 2)}\n`, "utf8");
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((error) => {
    process.stderr.write(`P4_EVENT_CONFIG_FAILED ${error.message}\n`);
    process.exitCode = 1;
  });
}

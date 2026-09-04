// Builds a p4-event-drive run config from a scenario.
//
// This is an OUTER implementation: it decides placement, capacity and the
// llama.cpp plan text. P4 carries that plan as an opaque string.

import fs from "node:fs";
import path from "node:path";
import { ACCEPTANCE_PROMPT } from "./scenarios.mjs";

function quote(value) {
  if (value.includes('"')) throw new Error("plan values cannot contain a double quote");
  return `"${value}"`;
}

/// One stage's llama.cpp plan. Layers this stage does not own are pushed to
/// CPU so their GGUF bytes are never loaded here; `--kv-unified` is what lets
/// split_simple() pack unequal Prefill and Decode rows into one physical
/// UBATCH, which is a batching policy rather than a load workaround.
function buildPlan({ model, begin, end, totalLayers, parallel, nBatch, nUbatch, totalContext,
                    flashAttn, cacheTypeK, cacheTypeV }) {
  const unowned = [];
  for (let layer = 0; layer < totalLayers; layer += 1) {
    if (layer < begin || layer >= end) unowned.push(layer);
  }
  const tokens = [
    "--model", quote(model),
    "--memory-topology", "discrete",
    "--layer-begin", `${begin}`,
    "--layer-end", `${end}`,
    "--kv-layer-begin", `${begin}`,
    "--kv-layer-end", `${end}`,
    "--n-seq-max", `${parallel}`,
    "--spec-type", "none",
    "--kv-unified",
    "--batch-size", `${nBatch}`,
    "--ubatch-size", `${nUbatch}`,
    "--ctx-size", `${totalContext}`,
    "--n-gpu-layers", `${totalLayers - begin}`,
    "--device", "CUDA0",
    // Flash attention and the cache types are the model's business, not the
    // harness's: a 2B model with an unquantised V cache is cheap and a 40
    // layer one is not. Defaults are what every scenario used before.
    "--flash-attn", flashAttn ?? "off",
    "--no-mmap",
    "--cache-type-k", cacheTypeK ?? "q8_0",
    "--cache-type-v", cacheTypeV ?? "f16",
  ];
  if (unowned.length) {
    tokens.push("--override-tensor", quote(`blk\\.(${unowned.join("|")})\\..*=CPU`));
  }
  return tokens.join(" ");
}

/// This run's incarnation number, worn by its connection, its nodes and its
/// load alike - they are three views of one thing.
///
/// Reusing it is not a detail. An agent keeps a high-water mark per node so
/// a superseded incarnation can never come back, and it keeps a duplicate
/// window a quarter of a million events deep keyed by the connection's own
/// identity. A second run that calls itself generation 1 is therefore either
/// refused as stale or, worse, silently swallowed as a repeat of the first
/// run's opening command - which reads as a hang, because nothing was
/// delivered and nothing was refused.
///
/// Seconds since the epoch: monotonic on one machine, distinct for any two
/// runs a person could start in sequence.
function runGeneration() {
  return Math.floor(Date.now() / 1000);
}

/// Whether a placement gives every stage a device of its own.
///
/// Two stages on one card are not two lanes - they contend for one, and each
/// pays a full set of per-batch fixed cost. Measured 2026-09-04: one stage a
/// card beat two by 42% on gemma-4-E2B and 33% on a 35B, interleaved, every
/// arm passing. A scenario may still oversubscribe deliberately - the
/// `prefill_mix` family does, because it is the arm that measurement
/// compares against - but it says so, and the default is refused.
function oversubscribed(devices) {
  return new Set(devices).size < devices.length;
}

export function buildConfig(spec, options = {}) {
  if (oversubscribed(spec.devices) && !spec.allowOversubscribedDevices) {
    throw new Error(
      `placement puts ${spec.devices.length} stages on ${new Set(spec.devices).size}` +
        ' devices; set allowOversubscribedDevices to measure that deliberately',
    );
  }
  const generation = runGeneration();
  const totalLayers = spec.cuts.at(-1)[1];
  const totalContext = spec.context * spec.parallel;
  const ingress = new URL(spec.ingress);

  const nodes = spec.cuts.map(([begin, end], index) => ({
    agent: spec.ingress,
    node: `node-${index}`,
    generation,
    binary: spec.binary,
    endpoint: `${ingress.hostname}:${spec.endpointBase + index}`,
    plan: buildPlan({
      model: spec.model,
      begin,
      end,
      totalLayers,
      parallel: spec.parallel,
      nBatch: spec.nBatch,
      nUbatch: spec.nUbatch,
      totalContext,
      flashAttn: spec.flashAttn,
      cacheTypeK: spec.cacheTypeK,
      cacheTypeV: spec.cacheTypeV,
    }),
    args: [],
    environment: [
      ["CUDA_VISIBLE_DEVICES", spec.devices[index]],
      // nvidia-smi enumerates by PCI bus id while CUDA defaults to
      // fastest-first, so the mapping must be pinned or a "3090 stage" can
      // land on the 4080.
      ["CUDA_DEVICE_ORDER", "PCI_BUS_ID"],
      ["P4_STAGED_TRACE_HELLO", "1"],
    ],
    n_batch: spec.nBatch,
    n_ubatch: spec.nUbatch,
    context_size: spec.context,
    total_context_size: totalContext,
    sequence_capacity: spec.parallel,
  }));

  return {
    ingress_agent: spec.ingress,
    channel: `p4-4node-${spec.name}`,
    connection_generation: generation,
    load_generation: generation,
    session_id: `session-${spec.name}`,
    request_id: "req",
    nodes,
    // A scenario may vary its prompts by request index; the default is the
    // acceptance prompt for every request, which every earlier run used.
    prompts: spec.promptFor
      ? Array.from({ length: spec.requestCount }, (_, index) => spec.promptFor(index))
      : Array(spec.requestCount).fill(ACCEPTANCE_PROMPT),
    // Each request is its own conversation here: the acceptance prompt is a
    // single turn, so sharing one key across them would claim a continuity
    // that does not exist. An OUTER serving a real chat would reuse the key
    // across the turns of that chat instead.
    session_key_template: `sk1:p4-4node/${spec.name}-{{request_id}}`,
    max_tokens: spec.maxTokens,
    waves: spec.waves,
    // Sampling is the same everywhere; stop strings are the model's, because
    // where a turn ends is a property of the template a model was trained on
    // and this is the layer that owns the template.
    options: JSON.stringify({
      temperature: 0.2,
      top_p: 0.9,
      top_k: 20,
      seed: 7,
      ...(spec.stops ? { stop: spec.stops } : {}),
      ...(spec.reasoning ?? {}),
    }),
    pre_inference_hold_ms: spec.preInferenceHoldMs,
    // Structural acceptance only. Whether the answer means anything is
    // judged by judge.mjs, which the drive cannot express.
    acceptance: {
      minimum_generated_tokens: 1,
      // A scenario that supplies stop strings will terminate on them, and
      // that is a clean finish rather than a fault: the first run with them
      // completed 64/64 and was still failed here for seven "stop"s.
      allowed_stop_reasons: spec.stops ? ["eos", "length", "stop"] : ["eos", "length"],
      responses: Array.from({ length: spec.requestCount }, () => ({})),
    },
    timeout_ms: spec.timeoutMs,
    ...options,
  };
}

export function writeConfig(spec, directory, options = {}) {
  fs.mkdirSync(directory, { recursive: true });
  const config = buildConfig(spec, options);
  const file = path.join(directory, "config.json");
  fs.writeFileSync(file, `${JSON.stringify(config, null, 2)}\n`, "utf8");
  return { file, config };
}

// Turns the inventory into stage-load probe jobs.
//
//   node test/benchmarks/model-catalog/build-jobs.mjs --mode plan --contexts 4096,32768,102400 --out <file>
//   node test/benchmarks/model-catalog/build-jobs.mjs --mode load --contexts 32768 --only <id,id> --out <file>
//
// A job is one model at one context under one placement strategy, and it runs
// every stage at once so the host memory two stages really need is measured
// rather than inferred. The parameters written here are what an OUTER hands P4;
// P4 carries the plan string without interpreting it.

import fs from 'node:fs';
import path from 'node:path';

const argument = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
};

const inventoryFile = argument('--inventory', path.join('target', 'model-catalog', 'inventory.json'));
const mode = argument('--mode', 'plan');
const contexts = argument('--contexts', '4096,32768,102400').split(',').map(Number);
const strategies = argument('--strategies', 'auto').split(',');
const stageCount = Number(argument('--stages', '2'));
const seqMax = Number(argument('--seq-max', '1'));
const devices = argument('--devices', '0,1').split(',');
const only = argument('--only', null);
const skip = argument('--skip', null);
const maxBytes = Number(argument('--max-gib', '0')) * 2 ** 30;
const out = argument('--out', path.join('target', 'model-catalog', `${mode}-jobs.json`));
const timeout = Number(argument('--timeout-s', mode === 'plan' ? '900' : '5400'));

const inventory = JSON.parse(fs.readFileSync(inventoryFile, 'utf8'));
const wanted = only ? new Set(only.split(',')) : null;
const unwanted = skip ? new Set(skip.split(',')) : null;

/// Architectures whose KV is shared from a layer to the end, so a stage
/// boundary must not fall inside that span. gemma-4 shares layers 13..n.
const SHARED_KV = { gemma4: 13 };

/// Every routed-expert weight stays on the CPU and is computed there, which is
/// what llama.cpp's --cpu-moe does. Routers, attention, norms and the KV cache
/// stay on the device.
const EXPERTS_TO_CPU = 'blk\\..*\\.ffn_(up|down|gate)_exps.*=CPU';
/// The dense equivalent: the feed-forward weights, which are most of a dense
/// model's bytes, move to the CPU while attention stays on the device.
const DENSE_FFN_TO_CPU = 'blk\\..*\\.ffn_(up|down|gate)\\.weight=CPU';

/// A model that shares its KV across a span of layers reads them as one
/// storage unit, so no stage boundary may fall inside that span. gemma-4 shares
/// from `shared_kv_from` to the last layer; the last stage therefore swallows
/// the whole region and the earlier stages divide what is left.
function cuts(layers, count, sharedFrom = null) {
  const even = (from, to, parts) => {
    const edges = [from];
    for (let i = 1; i < parts; i += 1) edges.push(from + Math.round((i * (to - from)) / parts));
    edges.push(to);
    return edges.slice(0, -1).map((begin, index) => [begin, edges[index + 1]]);
  };
  if (sharedFrom == null || sharedFrom <= 0 || sharedFrom >= layers) return even(0, layers, count);
  if (count === 1) return [[0, layers]];
  if (count - 1 > sharedFrom) return null; // not enough free layers to cut
  return [...even(0, sharedFrom, count - 1), [sharedFrom, layers]];
}

function planTokens(model, cut, layers, context, strategy) {
  const [begin, end] = cut;
  const unowned = [];
  for (let layer = 0; layer < layers; layer += 1) if (layer < begin || layer >= end) unowned.push(layer);
  const overrides = [];
  if (unowned.length) overrides.push(`blk\\.(${unowned.join('|')})\\..*=CPU`);
  if (strategy === 'expert_cpu') overrides.push(EXPERTS_TO_CPU);
  if (strategy === 'dense_ffn_cpu') overrides.push(DENSE_FFN_TO_CPU);
  const tokens = [
    '--model', `"${model.first_shard}"`,
    '--memory-topology', 'discrete',
    '--layer-begin', `${begin}`,
    '--layer-end', `${end}`,
    '--kv-layer-begin', `${begin}`,
    '--kv-layer-end', `${end}`,
    '--n-seq-max', `${seqMax}`,
    '--spec-type', 'none',
    '--kv-unified',
    '--batch-size', '512',
    '--ubatch-size', '512',
    '--ctx-size', `${context}`,
    '--n-gpu-layers', `${layers - begin}`,
    '--device', 'CUDA0',
    '--flash-attn', 'on',
    '--no-mmap',
    '--cache-type-k', 'q8_0',
    '--cache-type-v', 'q8_0',
  ];
  if (overrides.length) tokens.push('--override-tensor', `"${overrides.join(',')}"`);
  return tokens;
}

const jobs = [];
for (const model of inventory.models) {
  if (wanted && !wanted.has(model.id)) continue;
  if (unwanted && unwanted.has(model.id)) continue;
  if (maxBytes && (model.file_bytes ?? 0) > maxBytes) continue;
  if (model.header_error || !model.trunk_layers) continue;
  const layers = model.trunk_layers;
  if (layers < stageCount) continue;
  const sharedFrom = SHARED_KV[model.architecture] ?? null;
  const layerCuts = cuts(layers, stageCount, sharedFrom);
  if (!layerCuts) continue;
  const chosen = strategies[0] === 'auto'
    ? [model.expert_count ? 'expert_cpu' : 'vram_only']
    : strategies;
  for (const strategy of chosen) {
    for (const context of contexts) {
      if (model.context_length && context > model.context_length) continue;
      jobs.push({
        id: `${model.id}__${strategy}__ctx${context}__s${stageCount}__seq${seqMax}`,
        model_id: model.id,
        mode,
        strategy,
        context,
        seq_max: seqMax,
        stage_count: stageCount,
        timeout_s: timeout,
        stages: layerCuts.map((cut, index) => ({
          device: devices[index % devices.length],
          cut,
          plan: planTokens(model, cut, layers, context, strategy),
        })),
      });
    }
  }
}

fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, `${JSON.stringify(jobs, null, 2)}\n`);
process.stdout.write(`${JSON.stringify({ out, jobs: jobs.length, models: new Set(jobs.map((j) => j.model_id)).size })}\n`);

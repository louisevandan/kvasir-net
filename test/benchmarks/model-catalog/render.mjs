// Turns probe results into one record per model plus an index.
//
//   node test/benchmarks/model-catalog/render.mjs --results <a.jsonl,b.jsonl> --out <dir>
//
// Every number here is copied from a probe record, which is copied from the
// stage server's own MEMORY_PLAN / MEMORY_ACTUAL lines. Nothing is derived from
// a formula: a hybrid attention plus recurrent plus MoE stack does not have one.

import fs from 'node:fs';
import path from 'node:path';

const argument = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
};

const inventoryFile = argument('--inventory', path.join('target', 'model-catalog', 'inventory.json'));
const resultFiles = argument('--results', '').split(',').filter(Boolean);
const jobFiles = argument('--jobs', '').split(',').filter(Boolean);
const outDir = argument('--out', path.join('test', 'benchmarks', 'model-catalog', 'models'));

const inventory = JSON.parse(fs.readFileSync(inventoryFile, 'utf8'));
const byId = new Map(inventory.models.map((m) => [m.id, m]));

const jobsById = new Map();
for (const file of jobFiles) {
  for (const job of JSON.parse(fs.readFileSync(file, 'utf8'))) jobsById.set(job.id, job);
}

const records = [];
for (const file of resultFiles) {
  for (const line of fs.readFileSync(file, 'utf8').split(/\r?\n/)) {
    if (line.trim()) records.push(JSON.parse(line));
  }
}

const gib = (bytes) => (bytes == null ? null : Number((bytes / 2 ** 30).toFixed(3)));

/// One stage's memory as the stage server reported it, split by where it lives
/// and what it is for. `required` is the server's own sum.
function entries(plan) {
  if (!plan?.entries) return null;
  return plan.entries.map((e) => ({
    scope: e.scope,
    name: e.name,
    model_bytes: e.model,
    context_bytes: e.context,
    compute_bytes: e.compute,
    required_bytes: e.model + e.context + e.compute,
    free_bytes: e.free,
    total_bytes: e.total,
  }));
}

const perModel = new Map();
for (const record of records) {
  const job = jobsById.get(record.id);
  const modelId = job?.model_id ?? record.id.split('__')[0];
  if (!perModel.has(modelId)) perModel.set(modelId, []);
  const stages = record.stages.map((stage, index) => {
    const source = stage.actual ?? stage.plan;
    const sum = entries(source);
    return {
      stage: index,
      device: stage.device,
      cut: job?.stages?.[index]?.cut ?? null,
      outcome: stage.outcome,
      elapsed_ms: stage.elapsed_ms,
      measured: stage.actual ? 'actual' : (stage.plan ? 'plan' : 'none'),
      plan_equals_actual: stage.plan && stage.actual
        ? JSON.stringify(entries(stage.plan)) === JSON.stringify(entries(stage.actual))
        : null,
      execution_shape: source?.execution_shape ?? null,
      fits_current_free: source?.fits_current_free ?? null,
      entries: sum,
      buffers: stage.buffers,
      graph: stage.graph,
      error: stage.error?.length ? stage.error : undefined,
    };
  });
  const usable = stages.filter((s) => s.entries);
  const total = (scope, field) => usable.reduce((sum, s) => sum
    + s.entries.filter((e) => e.scope === scope).reduce((n, e) => n + e[field], 0), 0);
  perModel.get(modelId).push({
    id: record.id,
    mode: record.mode,
    at: record.at,
    strategy: job?.strategy ?? null,
    context: job?.context ?? null,
    seq_max: job?.seq_max ?? null,
    stage_count: record.stages.length,
    ok: stages.every((s) => s.outcome === 'loaded' || s.outcome === 'exit:0'),
    host_free_before_bytes: record.host_free_before,
    host_free_after_bytes: record.host_free_after,
    host_total_bytes: record.host_total,
    totals: usable.length ? {
      device_model_bytes: total('device', 'model_bytes'),
      device_context_bytes: total('device', 'context_bytes'),
      device_compute_bytes: total('device', 'compute_bytes'),
      device_required_bytes: total('device', 'required_bytes'),
      host_model_bytes: total('host', 'model_bytes'),
      host_context_bytes: total('host', 'context_bytes'),
      host_compute_bytes: total('host', 'compute_bytes'),
      host_required_bytes: total('host', 'required_bytes'),
    } : null,
    stages,
  });
}

fs.mkdirSync(outDir, { recursive: true });
const index = [];
for (const [modelId, runs] of perModel) {
  const model = byId.get(modelId) ?? { id: modelId };
  runs.sort((a, b) => (a.context ?? 0) - (b.context ?? 0) || String(a.mode).localeCompare(String(b.mode)));
  const loaded = runs.filter((r) => r.mode === 'load' && r.ok);
  const document = {
    schema: 'p4-model-catalog/1',
    id: modelId,
    identity: {
      publisher: model.publisher ?? null,
      repository: model.repository ?? null,
      base_name: model.base_name ?? null,
      architecture: model.architecture ?? null,
      name: model.name ?? null,
      size_label: model.size_label ?? null,
      files: model.files ?? null,
      first_shard: model.first_shard ?? null,
      file_bytes: model.file_bytes ?? null,
      file_gib: gib(model.file_bytes),
    },
    shape: {
      block_count: model.block_count ?? null,
      nextn_predict_layers: model.nextn_predict_layers ?? null,
      trunk_layers: model.trunk_layers ?? null,
      expert_count: model.expert_count ?? null,
      expert_used_count: model.expert_used_count ?? null,
      embedding_length: model.embedding_length ?? null,
      context_length: model.context_length ?? null,
      head_count_kv: model.head_count_kv ?? null,
      key_length: model.key_length ?? null,
      value_length: model.value_length ?? null,
      full_attention_interval: model.full_attention_interval ?? null,
      tensor_bytes: model.tensor_bytes ?? null,
      expert_bytes: model.expert_bytes ?? null,
      other_block_bytes: model.other_block_bytes ?? null,
      non_block_bytes: model.non_block_bytes ?? null,
    },
    prompt: {
      eos_token_id: model.eos_token_id ?? null,
      bos_token_id: model.bos_token_id ?? null,
      chat_template_markers: model.chat_template_markers ?? [],
    },
    runs,
    loaded_contexts: loaded.map((r) => r.context),
  };
  fs.writeFileSync(path.join(outDir, `${modelId}.json`), `${JSON.stringify(document, null, 2)}\n`);
  index.push({
    id: modelId,
    architecture: model.architecture ?? null,
    file_gib: gib(model.file_bytes),
    trunk_layers: model.trunk_layers ?? null,
    experts: model.expert_count ?? null,
    runs: runs.length,
    loaded: loaded.length,
    max_loaded_context: loaded.length ? Math.max(...loaded.map((r) => r.context)) : null,
  });
}

index.sort((a, b) => (a.file_gib ?? 0) - (b.file_gib ?? 0));
fs.writeFileSync(path.join(outDir, '_index.json'), `${JSON.stringify(index, null, 2)}\n`);
process.stdout.write(`${JSON.stringify({ outDir, models: index.length, records: records.length })}\n`);

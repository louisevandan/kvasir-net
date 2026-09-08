// Turns fit summaries into real-load jobs.
//
//   node test/benchmarks/model-catalog/build-load-jobs.mjs --fit a.json,b.json --jobs plan-jobs.json,plan2-jobs.json \
//     --group large --out target/model-catalog/load-large.json
//
// Loading reads every owned weight off the model share, so the cost is the
// model's size and not its context. Small models are therefore loaded at all
// four context points; the large ones are loaded at the context that is hardest
// on memory, and their other points stay on the plan the same run verified.

import fs from 'node:fs';
import path from 'node:path';

const argument = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
};

const fitFiles = argument('--fit', '').split(',').filter(Boolean);
const jobFiles = argument('--jobs', '').split(',').filter(Boolean);
const inventoryFile = argument('--inventory', path.join('target', 'model-catalog', 'inventory.json'));
const out = argument('--out', path.join('target', 'model-catalog', 'load-jobs.json'));
const group = argument('--group', 'all');
const minGib = Number(argument('--min-gib', '0'));
const maxGib = Number(argument('--max-gib', '100000'));
const timeout = Number(argument('--timeout-s', '7200'));
const ladder = argument('--ladder', '4096,32768,102400').split(',').map(Number);
const policy = argument('--policy', 'auto');
const localRoot = argument('--local-root', null);

const inventory = JSON.parse(fs.readFileSync(inventoryFile, 'utf8'));
const size = new Map(inventory.models.map((m) => [m.id, m.file_bytes ?? 0]));
const byId = new Map(inventory.models.map((m) => [m.id, m]));

const planJobs = new Map();
for (const file of jobFiles) {
  for (const job of JSON.parse(fs.readFileSync(file, 'utf8'))) planJobs.set(job.id, job);
}

const best = new Map();
for (const file of fitFiles) {
  for (const model of JSON.parse(fs.readFileSync(file, 'utf8'))) {
    if (!model.max_fitting_context) continue;
    const previous = best.get(model.model_id);
    // A later strategy wins only if it reaches a larger context.
    if (!previous || model.max_fitting_context > previous.max_fitting_context) best.set(model.model_id, model);
  }
}

/// Point a plan template at the host's local copy of the model. The share is a
/// 1 Gbps link and a four-context sweep would cross it four times per model.
function localise(stage, model) {
  if (!localRoot) return stage;
  const plan = [...stage.plan];
  const index = plan.indexOf('--model');
  if (index >= 0) {
    const name = plan[index + 1].replace(/"/g, '').split('\\').pop();
    plan[index + 1] = `"${localRoot}\\${model.repository}\\${name}"`;
  }
  return { ...stage, plan };
}

const jobs = [];
for (const [modelId, fit] of best) {
  const bytes = size.get(modelId) ?? 0;
  const gib = bytes / 2 ** 30;
  if (gib < minGib || gib > maxGib) continue;
  const contexts = new Set([fit.max_fitting_context]);
  const wantLadder = policy === 'ladder' || (policy === 'auto' && gib <= 32);
  const wantFloor = policy === 'floor' || (policy === 'auto' && gib > 32);
  if (wantLadder) for (const context of ladder) if (fit.fitting_contexts.includes(context)) contexts.add(context);
  if (wantFloor) contexts.add(Math.min(...fit.fitting_contexts));
  for (const context of [...contexts].sort((a, b) => a - b)) {
    const source = fit.entries.find((e) => e.context === context);
    const template = planJobs.get(source?.id ?? '');
    if (!template) continue;
    const model = byId.get(modelId);
    jobs.push({
      ...template,
      stages: template.stages.map((stage) => localise(stage, model)),
      id: template.id.replace('__ctx', '__load__ctx'),
      mode: 'load',
      timeout_s: timeout,
      group,
      model_gib: Number(gib.toFixed(2)),
    });
  }
}

jobs.sort((a, b) => (b.model_gib - a.model_gib) || (a.context - b.context));
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, `${JSON.stringify(jobs, null, 2)}\n`);
process.stdout.write(`${JSON.stringify({
  out,
  jobs: jobs.length,
  models: new Set(jobs.map((j) => j.model_id)).size,
  gib_total: Number(jobs.reduce((sum, j) => sum + j.model_gib, 0).toFixed(1)),
})}\n`);

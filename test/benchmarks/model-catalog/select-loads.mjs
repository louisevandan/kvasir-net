// Reads plan results and picks, per model, the contexts worth loading for real.
//
//   node test/benchmarks/model-catalog/select-loads.mjs --results <file> --jobs <file> [--out <file>]
//
// A stage server's own `fits_current_free` answers for one stage only. Several
// stages share one host, so the host side is summed across the stages of a run
// and compared with the free memory the probe recorded at that moment.

import fs from 'node:fs';
import path from 'node:path';

const argument = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
};

const resultFile = argument('--results', null);
const jobFile = argument('--jobs', null);
const out = argument('--out', null);
const headroom = Number(argument('--headroom', '0.92'));
if (!resultFile || !jobFile) throw new Error('needs --results and --jobs');

const jobs = new Map(JSON.parse(fs.readFileSync(jobFile, 'utf8')).map((j) => [j.id, j]));
const records = fs.readFileSync(resultFile, 'utf8').split(/\r?\n/).filter(Boolean).map((l) => JSON.parse(l));

const perModel = new Map();
for (const record of records) {
  const job = jobs.get(record.id);
  if (!job) continue;
  const stages = record.stages;
  const plans = stages.map((s) => s.plan);
  // `--inspect-memory-plan` exits 7 when the plan is complete but does not fit
  // the memory free right now, which is an answer rather than a failure. Only a
  // stage that produced no plan at all is blocked.
  const complete = plans.every((p) => p?.entries);
  const failure = stages.find((s) => !s.plan?.entries);
  let deviceOk = null;
  let hostRequired = null;
  let deviceRequired = null;
  if (complete) {
    deviceOk = plans.every((p) => p.entries.filter((e) => e.scope === 'device')
      .every((e) => e.required <= e.free));
    hostRequired = plans.reduce((sum, p) => sum
      + p.entries.filter((e) => e.scope === 'host').reduce((n, e) => n + e.required, 0), 0);
    deviceRequired = Math.max(...plans.map((p) => p.entries.filter((e) => e.scope === 'device')
      .reduce((n, e) => n + e.required, 0)));
  }
  const hostOk = hostRequired == null ? null : hostRequired <= record.host_free_before * headroom;
  const entry = {
    id: record.id,
    context: job.context,
    strategy: job.strategy,
    stage_count: job.stage_count,
    complete,
    device_ok: deviceOk,
    host_ok: hostOk,
    fits: Boolean(complete && deviceOk && hostOk),
    device_required_bytes: deviceRequired,
    host_required_bytes: hostRequired,
    host_free_bytes: record.host_free_before,
    outcome: stages.map((s) => s.outcome).join(','),
    blocker: failure ? (failure.error?.[0] ?? failure.outcome) : null,
  };
  if (!perModel.has(job.model_id)) perModel.set(job.model_id, []);
  perModel.get(job.model_id).push(entry);
}

const summary = [];
for (const [modelId, entries] of perModel) {
  entries.sort((a, b) => a.context - b.context);
  const fitting = entries.filter((e) => e.fits);
  summary.push({
    model_id: modelId,
    strategy: entries[0]?.strategy ?? null,
    probed_contexts: entries.map((e) => e.context),
    fitting_contexts: fitting.map((e) => e.context),
    max_fitting_context: fitting.length ? Math.max(...fitting.map((e) => e.context)) : null,
    first_blocker: entries.find((e) => !e.fits)?.blocker ?? null,
    entries,
  });
}

summary.sort((a, b) => String(a.model_id).localeCompare(String(b.model_id)));
const text = `${JSON.stringify(summary, null, 2)}\n`;
if (out) {
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, text);
}
for (const model of summary) {
  process.stdout.write(`${[
    model.model_id.padEnd(60),
    (model.strategy ?? '-').padEnd(13),
    `fit=${model.fitting_contexts.join('/') || 'none'}`,
    model.max_fitting_context ? `max=${model.max_fitting_context}` : `blocked=${(model.first_blocker ?? '?').slice(0, 60)}`,
  ].join(' ')}\n`);
}

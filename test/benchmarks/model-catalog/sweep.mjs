// Loads every model the copy has finished, and keeps up as more arrive.
//
//   node test/benchmarks/model-catalog/sweep.mjs --fit a.json,b.json --jobs p1.json,p2.json \
//     --out target/model-catalog --local-root D:\models [--rounds 40]
//
// The share and the host's SSD are different resources, so a load reading the
// local copy costs the copy nothing. This walks the staged set, loads whatever
// is newly complete, and stops when the copy is done and nothing is left.

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const argument = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
};

const here = path.dirname(new URL(import.meta.url).pathname.replace(/^\//, ''));
const fitFiles = argument('--fit', '');
const jobFiles = argument('--jobs', '');
const outDir = argument('--out', path.join('target', 'model-catalog'));
const localRoot = argument('--local-root', 'D:\\models');
const rounds = Number(argument('--rounds', '60'));
const waitMs = Number(argument('--wait-s', '120')) * 1000;

const node = (script, args) => spawnSync(process.execPath, [path.join(here, script), ...args],
  { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });

const skipFile = argument('--already', null);
// Models already measured in an earlier run of this sweep.
const done = new Set(skipFile && fs.existsSync(skipFile)
  ? fs.readFileSync(skipFile, 'utf8').trim().split(',').filter(Boolean)
  : []);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

for (let round = 1; round <= rounds; round += 1) {
  const checked = node('staged-check.mjs', ['--dest', localRoot]);
  let state;
  try {
    state = JSON.parse(checked.stdout.trim().split(/\r?\n/).pop());
  } catch {
    process.stderr.write(`SWEEP round ${round}: staged-check failed\n${checked.stdout}${checked.stderr}\n`);
    await sleep(waitMs);
    continue;
  }
  const fresh = state.staged.filter((id) => !done.has(id));
  process.stdout.write(`SWEEP round ${round} copying=${state.copying} staged=${state.staged.length} fresh=${fresh.length}\n`);
  if (!fresh.length) {
    if (!state.copying) {
      process.stdout.write('SWEEP complete: copy finished and every staged model was loaded\n');
      break;
    }
    await sleep(waitMs);
    continue;
  }
  const jobFile = path.join(outDir, `sweep-round${round}.json`);
  const built = node('build-load-jobs.mjs', ['--fit', fitFiles, '--jobs', jobFiles,
    '--local-root', localRoot, '--policy', 'ladder', '--group', `sweep${round}`, '--out', jobFile]);
  if (built.status !== 0) {
    process.stderr.write(`SWEEP round ${round}: job build failed\n${built.stderr}\n`);
    break;
  }
  const all = JSON.parse(fs.readFileSync(jobFile, 'utf8'));
  const selected = all.filter((job) => fresh.includes(job.model_id));
  if (!selected.length) {
    for (const id of fresh) done.add(id);
    continue;
  }
  fs.writeFileSync(jobFile, `${JSON.stringify(selected, null, 2)}\n`);
  const tag = `sweep${round}`;
  const started = node('remote-probe.mjs', ['run', '--jobs', jobFile, '--tag', tag]);
  process.stdout.write(`SWEEP round ${round}: ${selected.length} jobs over ${fresh.length} models -> ${started.stdout.trim()}\n`);
  // Wait for this round's probe task to finish before starting the next.
  for (;;) {
    await sleep(30_000);
    const status = node('remote-probe.mjs', ['status', '--tag', tag]).stdout;
    if (/task=Ready/.test(status)) break;
  }
  node('remote-probe.mjs', ['fetch', '--tag', tag, '--out', path.join(outDir, `${tag}.jsonl`)]);
  // The measurements are recorded, so the host's SSD can take the next models.
  const reclaimed = { stdout: '' };
  for (let i = 0; i < fresh.length; i += 3) {
    const part = node('stage-models.mjs', ['remove', '--models', fresh.slice(i, i + 3).join(',')]);
    reclaimed.stdout += part.stdout;
  }
  process.stdout.write(`SWEEP round ${round} reclaimed: ${reclaimed.stdout.trim().split(/\r?\n/).pop()}\n`);
  for (const id of fresh) done.add(id);
  process.stdout.write(`SWEEP round ${round} finished; loaded models so far: ${done.size}\n`);
}

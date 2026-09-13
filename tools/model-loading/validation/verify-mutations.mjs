import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';

// Mutate independent copies only. A fresh Node process executes each copied source.
const root = path.resolve(import.meta.dirname, '..');
const optionIndex = process.argv.indexOf('--out');
const dest = optionIndex < 0
  ? path.join(root, 'target', `mutations-${Date.now()}`)
  : path.resolve(process.argv[optionIndex + 1] ?? '');
if (fs.existsSync(dest)) throw new Error('mutation output already exists; choose a fresh directory');
const planner = 'src/model-loading-planner.ts';
const placement = 'src/placement-policy.ts';
const files = [planner, placement, 'src/model-loading-policy.ts',
  'tests/loading-evaluation.test.ts', 'validation/loading-reference.ts', 'validation/loading-judgments.ts'];
const mutations = [
  ['availability', planner, 'Math.min(pool.capacityBytes, pool.availableBytes ?? pool.capacityBytes)', 'pool.capacityBytes', 'analyst judgment: (busy-fast-device|one-byte-short)'],
  ['unified-host', planner, 'machine.ram.availableBytes ?? capacityBytes', 'capacityBytes', 'analyst judgment: shared-ram-pressure'],
  ['legal-cuts', planner, 'legalCuts: input.model.legalCuts', 'legalCuts: undefined', 'analyst judgment: indivisible-storage'],
  ['prefix-objective', placement, 'const best = partitionOptional(permitted, request.layerCount, minimumMachines, request.legalCuts, primary.bottleneck)!;', 'const best = primary;', 'analyst judgment: later-bottleneck'],
  ['disabled-device', planner, 'pools.filter((pool) => pool.enabled !== false).map', 'pools.filter((pool) => true).map', 'analyst judgment: operator-disabled'],
];
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const results = [];
for (const [name, target, from, to, pattern] of mutations) {
  const copy = path.join(dest, name);
  for (const file of files) {
    const out = path.join(copy, file);
    fs.mkdirSync(path.dirname(out), { recursive: true });
    fs.copyFileSync(path.join(root, file), out);
  }
  const file = path.join(copy, target), source = fs.readFileSync(file, 'utf8');
  if (source.split(from).length !== 2) throw new Error('mutation match is not unique: ' + name);
  fs.writeFileSync(file, source.replace(from, to));
  const syntax = spawnSync(process.execPath, ['--check', file], { encoding: 'utf8' });
  if (syntax.status !== 0) throw new Error(syntax.stderr);
  const run = spawnSync(process.execPath, ['--test', '--test-name-pattern=' + pattern,
    path.join(copy, 'tests/loading-evaluation.test.ts')], { encoding: 'utf8', timeout: 30000 });
  fs.writeFileSync(path.join(copy, 'test.log'), run.stdout + run.stderr);
  if (run.status !== 1 || !run.stdout.includes('AssertionError')) throw new Error('mutation did not trigger assertion: ' + name);
  results.push({ name, exit: run.status, syntaxExit: syntax.status,
    baselineSources: Object.fromEntries(files.map(file => [file, sha(path.join(root, file))])),
    sourceSha256: sha(file), nodeSha256: sha(process.execPath), logSha256: sha(path.join(copy, 'test.log')),
    note: 'Fresh Node process strips and executes mutated TypeScript source directly; no compiled baseline cache.' });
}
fs.writeFileSync(path.join(dest, 'results.json'), JSON.stringify(results, null, 2) + '\n', { flag: 'wx' });
console.log(JSON.stringify({ output: dest, detected: results.length, results }));

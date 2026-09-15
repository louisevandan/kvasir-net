import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';

// Each mutation executes from an independent source copy in a fresh process.
const root = path.resolve(import.meta.dirname, '..');
const optionIndex = process.argv.indexOf('--out');
const dest = optionIndex < 0
  ? path.join(root, 'target', `native-deployment-mutations-${Date.now()}`)
  : path.resolve(process.argv[optionIndex + 1] ?? '');
if (fs.existsSync(dest)) throw new Error('mutation output already exists; choose a fresh directory');
const target = 'src/native-deployment.ts';
const files = [target, 'index.ts', 'src/model-loading-planner.ts', 'src/model-loading-policy.ts',
  'src/placement-policy.ts', 'tests/native-deployment.test.ts'];
const mutations = [
  ['shared-pool-sum', 'pool.requiredBytes += entry.required;', 'pool.requiredBytes = entry.required;',
    'native PLAN authorizes'],
  ['actual-allocation', 'isDeepStrictEqual(allocationShape(actual), allocationShape(planned))', 'true',
    'post-LOAD evidence'],
  ['runtime-source', 'fail(sourceIdentity === currentSourceIdentity,', 'fail(true,',
    'preflight fails closed'],
  ['legal-cut', 'fail(input.model.legalCuts.includes(stage.layerEnd),', 'fail(true,',
    'preflight fails closed'],
];
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const results = [];
for (const [name, from, to, pattern] of mutations) {
  const copy = path.join(dest, name);
  for (const relative of files) {
    const output = path.join(copy, relative);
    fs.mkdirSync(path.dirname(output), { recursive: true });
    fs.copyFileSync(path.join(root, relative), output);
  }
  const file = path.join(copy, target);
  const source = fs.readFileSync(file, 'utf8');
  if (source.split(from).length !== 2) throw new Error(`mutation match is not unique: ${name}`);
  fs.writeFileSync(file, source.replace(from, to));
  const syntax = spawnSync(process.execPath, ['--check', file], { encoding: 'utf8' });
  if (syntax.status !== 0) throw new Error(syntax.stderr);
  const testFile = path.join(copy, 'tests/native-deployment.test.ts');
  const run = spawnSync(process.execPath, ['--test', `--test-name-pattern=${pattern}`, testFile],
    { encoding: 'utf8', timeout: 30000 });
  fs.writeFileSync(path.join(copy, 'test.log'), run.stdout + run.stderr);
  if (run.status !== 1 || !run.stdout.includes('AssertionError')) {
    throw new Error(`mutation did not trigger an assertion: ${name}`);
  }
  results.push({ name, exit: run.status, syntaxExit: syntax.status,
    baselineSourceSha256: sha(path.join(root, target)), sourceSha256: sha(file),
    nodeSha256: sha(process.execPath), logSha256: sha(path.join(copy, 'test.log')),
    note: 'Fresh Node process executed an independent mutated TypeScript source copy.' });
}
fs.writeFileSync(path.join(dest, 'results.json'), JSON.stringify(results, null, 2) + '\n', { flag: 'wx' });
console.log(JSON.stringify({ output: dest, detected: results.length, results }));

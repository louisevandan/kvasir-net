import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { compose, sha256 } from './compose.mjs';

const read = name => JSON.parse(fs.readFileSync(new URL(name, import.meta.url), 'utf8'));
function input() {
  return {
    runId: 'test-long-1', generation: 101,
    model: read('./models/hy3-no-think.json'), policy: read('./policies/decode2-open8.json'),
    workload: { ...read('./workloads/long8.json'), record_counts: [4, 7], waves: [{ after_ms: 0, count: 2 }] },
    runtime: { sources: ['a'.repeat(40)], binding_file_sha256: 'b'.repeat(64) },
    cluster: { ingress_agent: 'tcp://127.0.0.1:23000', timeout_ms: 3000, nodes: [
      { node: 'old', generation: 7, agent: 'tcp://127.0.0.1:23000', endpoint: '127.0.0.1:23001',
        plan: '--model model.gguf --device CUDA0 --cache-type-k q4_0 --override-tensor weights=CPU',
        context_size: 102400, sequence_capacity: 8, n_batch: 512, n_ubatch: 256 },
    ] },
  };
}

test('profiles change model rendering without changing cluster placement or EOS contract', () => {
  const a = input(), before = structuredClone(a), hy3 = compose(a);
  a.model = read('./models/step37-no-think.json');
  const step = compose(a);
  assert.notEqual(hy3.config.prompts[0], step.config.prompts[0]);
  for (const key of ['plan', 'endpoint', 'agent', 'context_size', 'sequence_capacity', 'n_batch', 'n_ubatch']) {
    assert.equal(hy3.config.nodes[0][key], before.cluster.nodes[0][key]);
    assert.equal(step.config.nodes[0][key], before.cluster.nodes[0][key]);
  }
  assert.deepEqual(hy3.config.acceptance.allowed_stop_reasons, ['eos']);
  assert.equal(JSON.parse(hy3.config.options).ignore_eos, false);
  assert.equal(hy3.manifest.tokenizer_verified, false);
  assert.deepEqual(hy3.manifest.requests[0].expected[0], {
    id: 'R0001', current_A: 10, resistance_milliohm: 20, duration_hours: 1,
    power_W: 2, energy_Wh: 2, pressure_kPa: 80, pressure_alarm: false,
  });
  assert.deepEqual(a.cluster, before.cluster);
});

test('exact prompt counts bind context and reject overflow or counts from another prompt', () => {
  const a = input(), prepared = compose(a);
  a.tokenCounts = prepared.manifest.requests.map(r => ({ prompt_sha256: r.prompt_sha256, prompt_tokens: 98304 }));
  assert.equal(compose(a).manifest.tokenizer_verified, true);
  a.tokenCounts[1].prompt_tokens++;
  assert.throws(() => compose(a), /exceed context/);
  a.tokenCounts[1].prompt_tokens--;
  a.tokenCounts[1].prompt_sha256 = 'c'.repeat(64);
  assert.throws(() => compose(a), /another prompt/);
});

test('rejects missing identities, incompatible waves and duplicate CPU settings', () => {
  for (const change of [a => a.runtime.sources = ['main'], a => a.workload.waves[0].count++,
    a => a.policy.environment.P4_STAGED_DECODE_MEMBERS = 9]) {
    const a = input(); change(a); assert.throws(() => compose(a));
  }
  const a = input(); a.policy = read('./policies/decode4-min4-cpu4.json');
  const result = compose(a);
  assert.match(result.config.nodes[0].plan, /--threads 4 --threads-batch 4$/);
  a.cluster.nodes[0].plan += ' --threads-batch 8';
  assert.throws(() => compose(a), /already owns native threads/);
});

test('real CLI writes bound input and refuses both overwrite and invalid work before output creation', () => {
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'p4-cluster-compose-'));
  try {
    const a = input(), spec = { runId: a.runId, generation: a.generation };
    a.policy = read('./policies/pipeline-open8.json');
    a.policy.environment.P4_STAGED_MIXED_BATCH_ROWS = 32;
    for (const key of ['cluster', 'model', 'policy', 'workload', 'runtime']) {
      spec[key] = key + '.json'; fs.writeFileSync(path.join(temporary, spec[key]), JSON.stringify(a[key]));
    }
    const specFile = path.join(temporary, 'experiment.json'), output = path.join(temporary, 'arm');
    fs.writeFileSync(specFile, JSON.stringify(spec));
    const invoke = destination => spawnSync(process.execPath, [new URL('./compose.mjs', import.meta.url).pathname.replace(/^\/([A-Z]:)/, '$1'), specFile, destination], { encoding: 'utf8', windowsHide: true });
    assert.equal(invoke(output).status, 0);
    const config = fs.readFileSync(path.join(output, 'config.json'));
    const manifest = readFile(path.join(output, 'manifest.json'));
    assert.equal(manifest.config_sha256, sha256(config));
    assert.equal(manifest.agent_environment.P4_STAGED_PIPELINE_BATCHING, '1');
    assert.equal(manifest.agent_environment.P4_STAGED_MIXED_BATCH_ROWS, '32');
    assert.notEqual(invoke(output).status, 0);
    assert.deepEqual(fs.readFileSync(path.join(output, 'config.json')), config);
    a.workload.waves[0].count++;
    fs.writeFileSync(path.join(temporary, 'workload.json'), JSON.stringify(a.workload));
    const rejected = path.join(temporary, 'rejected');
    assert.notEqual(invoke(rejected).status, 0);
    assert.equal(fs.existsSync(rejected), false);
    a.workload.waves[0].count--;
    fs.writeFileSync(path.join(temporary, 'workload.json'), JSON.stringify(a.workload));
    for (const [name, value] of [['P4_STAGED_MAX_OPEN_BATCHES', 0], ['P4_STAGED_MIXED_PREFILL_ROWS', 0],
      ['P4_STAGED_PREFILL_FRAGMENTS', 2], ['P4_STAGED_PIPELINE_BATCHING', 2],
      ['P4_STAGED_MIXED_BATCH_ROWS', 0], ['P4_STAGED_PIPELINE_BATCHING', 0]]) {
      const bad = structuredClone(a.policy); bad.environment[name] = value;
      fs.writeFileSync(path.join(temporary, 'policy.json'), JSON.stringify(bad));
      assert.notEqual(invoke(rejected).status, 0);
      assert.equal(fs.existsSync(rejected), false);
      assert.deepEqual(fs.readFileSync(path.join(output, 'config.json')), config);
    }
  } finally {
    assert.ok(path.resolve(temporary).startsWith(path.resolve(os.tmpdir()) + path.sep + 'p4-cluster-compose-'));
    fs.rmSync(temporary, { recursive: true, force: true });
  }
});

function readFile(file) { return JSON.parse(fs.readFileSync(file, 'utf8')); }

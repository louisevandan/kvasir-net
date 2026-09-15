import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { contract, validateManifest, verifyFiles } from './manifest.mjs';

const hash = data => crypto.createHash('sha256').update(data).digest('hex');
function fixture(root) {
  const artifacts = [];
  const add = (id, bytes = Buffer.from(id)) => {
    artifacts.push({ id, path: id + '.bin', bytes: bytes.length, sha256: hash(bytes) });
    if (root) fs.writeFileSync(path.join(root, id + '.bin'), bytes);
    return id;
  };
  const hosts = Array.from({ length: 3 }, (_, i) => ({ identity: hash('host' + i),
    inspect: add('inspect' + i), inspected_unix_ms: 1789320000000,
    pools: [{ id: 'shared', available_bytes: 20000, reserve_bytes: 1000 }] }));
  const cuts = [0,16,32,48];
  const stages = cuts.slice(1).map((end, i) => ({ host: hosts[i].identity,
    layer_begin: cuts[i], layer_end: end, plan: add('plan' + i), native: add('native' + i),
    agent: add('agent' + i), allocation_conformance: add('actual' + i),
    state_abi: hash('state'), wire_abi: hash('wire'), n_batch: 128, n_ubatch: 64,
    kv_unified: true, k_type: 'f16', v_type: 'f16', flash_attention: true,
    allocations: [{ pool: 'shared', weight_bytes: 1000, state_bytes: 500, scratch_bytes: 100 }],
    output_pool: 'shared', receipt_pool: 'shared', edge_pool: 'shared',
    bounds: { physical_result_bytes: 100, retained_output_count: 2, retained_output_bytes: 200,
      receipt_count: 2, receipt_each_bytes: 30, receipt_bytes: 60,
      edge_count: 2, edge_each_bytes: 100, edge_bytes: 200 } }));
  const requests = ['short','short','short','short','medium','medium','long','long'].map((kind,i) => {
    const tokens = kind === 'short' ? 4000 : kind === 'medium' ? 32000 : 100038;
    return { id: 'r' + i, class: kind, after_ms: 0, prompt: add('prompt' + i),
      token_ids: add('tokens' + i, Buffer.alloc(tokens * 4, 1)), oracle: add('oracle' + i),
      source_facts: add('facts' + i), input_tokens: tokens, serialized_bytes: 600000 };
  });
  return { schema: contract.schema, mode: 'cold', run_id: 'test-arm', epoch: 1,
    contract: structuredClone(contract), artifacts, sources: ['1234567890'.repeat(4)],
    model: { id: contract.target_id, architecture: 'qwen35moe', layer_count: contract.model_layers,
      shards: Array.from({length:contract.model_shards},(_,i) => add('shard' + i)),
      metadata: add('metadata'), template: add('template') },
    tokenizer: { binary: add('tokenizer'), add_special: true, parse_special: true },
    trust_boundary: 'approved-private-lan', hosts, stages, requests,
    profile: { artifact: add('profile'), spec: 'none', prefill_fragments: 1, service_controller: false, prefix_reuse: false },
    semantic_review: add('review'), legacy_regression_manifest: add('legacy') };
}

test('strict materialized specification has an explicit execution and quality boundary', () => {
  const m = fixture();
  assert.deepEqual(validateManifest(m), { valid: true, requests: 8, timeout_ms: 1800000, runtime_acceptance: false });
  for (const mutate of [
    m => delete m.stages[0].bounds.physical_result_bytes,
    m => m.stages[0].bounds.receipt_each_bytes = Infinity,
    m => m.stages[0].bounds.receipt_bytes--,
    m => m.stages[0].bounds.edge_each_bytes = -1,
    // One stage owns 1,600 allocation + 200 output + 60 receipt + 200 edge
    // bytes in addition to the 1,000-byte host reserve: 3,059 is one short.
    m => m.hosts[0].pools[0].available_bytes = 3059,
    m => m.stages[0].allocations[0].weight_bytes = Number.MAX_SAFE_INTEGER,
    m => m.stages[0].output_pool = 'unbound',
    m => m.requests[0].serialized_bytes = contract.request_bytes + 1,
    m => m.requests[6].input_tokens--,
    m => m.requests[0].after_ms = 180000,
    m => m.model.shards[0] = m.model.shards[1],
    m => m.model.id = 'historical-550b',
    m => m.model.layer_count = 49,
    m => m.stages[2].host = m.stages[1].host,
    m => m.contract.deadline_ms.long++,
    m => m.artifacts[0].sha256 = '0'.repeat(64),
  ]) { const bad = structuredClone(m); mutate(bad); assert.throws(() => validateManifest(bad)); }
});

test('actual CLI binds file bytes and rejects replacement before returning a valid result', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'p4-release-a-manifest-'));
  try {
    const m = fixture(root); const input = path.join(root, 'manifest.json');
    fs.writeFileSync(input, JSON.stringify(m));
    const cli = fileURLToPath(new URL('./manifest.mjs', import.meta.url));
    const ok = spawnSync(process.execPath, [cli, input], { encoding: 'utf8' });
    assert.equal(ok.status, 0, ok.stderr);
    assert.equal(JSON.parse(ok.stdout).runtime_acceptance, false);
    fs.appendFileSync(path.join(root, m.artifacts[0].path), 'changed');
    assert.throws(() => verifyFiles(m, root), /artifact changed/);
    const bad = spawnSync(process.execPath, [cli, input], { encoding: 'utf8' });
    assert.notEqual(bad.status, 0); assert.equal(bad.stdout, '');
  } finally {
    assert.equal(path.dirname(path.resolve(root)), path.resolve(os.tmpdir()));
    assert(path.basename(root).startsWith('p4-release-a-manifest-'));
    fs.rmSync(root, { recursive: true, force: true });
  }
});

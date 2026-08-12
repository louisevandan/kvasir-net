import assert from 'node:assert/strict';
import test from 'node:test';
import { measuredModelLoadOptions } from './model-load-policy.mjs';

test('selects a distinct physical batch ceiling for each verified 35B node', async () => {
  const options = await measuredModelLoadOptions({
    model: 'unsloth/Ornith-1.0-35B-GGUF/model.gguf',
    nodes: [
      { id: 'local-4080' },
      { id: 'local-3090' },
      { id: 'remote-3090-a' },
      { id: 'remote-3090-b' }
    ],
    parallel: 100,
    batch: 100,
    ubatch: 100
  });
  assert.equal(options.batching.max_sequences, 64);
  assert.equal(options.batching.calculation.result, 64);
  assert.equal(options.batching.calculation.terms.length, 7);
  assert.deepEqual(
    options.batching.node_limits.map(({ node_id, max_sequences }) => ({
      node_id, max_sequences
    })),
    [
      { node_id: 'local-4080', max_sequences: 64 },
      { node_id: 'local-3090', max_sequences: 100 },
      { node_id: 'remote-3090-a', max_sequences: 100 },
      { node_id: 'remote-3090-b', max_sequences: 100 }
    ]
  );
});

test('applies explicit node batch overrides as hard caps', async () => {
  const options = await measuredModelLoadOptions({
    model: 'unsloth/Ornith-1.0-35B-GGUF/model.gguf',
    nodes: [
      { id: 'local-4080' },
      { id: 'local-3090' },
      { id: 'remote-3090-a' },
      { id: 'remote-3090-b' }
    ],
    parallel: 100,
    batch: 100,
    ubatch: 100,
    nodeBatchLimits: {
      'local-4080': 48,
      'local-3090': 128
    }
  });
  assert.equal(options.batching.max_sequences, 48);
  assert.deepEqual(
    options.batching.node_limits.map(({ node_id, max_sequences }) => ({
      node_id,
      max_sequences
    })),
    [
      { node_id: 'local-4080', max_sequences: 48 },
      { node_id: 'local-3090', max_sequences: 100 },
      { node_id: 'remote-3090-a', max_sequences: 100 },
      { node_id: 'remote-3090-b', max_sequences: 100 }
    ]
  );
});

test('rejects node override for unknown nodes', async () => {
  await assert.rejects(
    measuredModelLoadOptions({
      model: 'unsloth/Ornith-1.0-35B-GGUF/model.gguf',
      nodes: [{ id: 'local-4080' }],
      parallel: 16,
      batch: 16,
      ubatch: 16,
      nodeBatchLimits: { 'local-4090': 10 }
    }),
    /nodeBatchLimits references unknown node/
  );
});

test('falls back to one for an unmeasured stage', async () => {
  const options = await measuredModelLoadOptions({
    model: 'unknown.gguf',
    nodes: [{ id: 'future-gpu' }],
    parallel: 32,
    batch: 32,
    ubatch: 32
  });
  assert.equal(options.batching.max_sequences, 1);
  assert.equal(options.batching.node_limits[0].max_sequences, 1);
  assert.match(options.batching.calculation.terms.at(-1).source, /unverified/);
});

test('a verified group reports no unverified stage', async () => {
  const options = await measuredModelLoadOptions({
    model: 'unsloth/Ornith-1.0-35B-GGUF/model.gguf',
    nodes: [{ id: 'local-4080' }, { id: 'local-3090' }],
    parallel: 32,
    batch: 256,
    ubatch: 128
  });
  assert.deepEqual(options.batching.unverified_nodes, []);
  assert.equal(options.batching.max_sequences, 32);
});

test('an unmatched model names every stage it silently capped at one', async () => {
  const options = await measuredModelLoadOptions({
    model: 'Qwen2.5-1.5B-Instruct-Q8_0.gguf',
    nodes: [{ id: 'p4-gpu-3090' }, { id: 'p4-gpu-4080' }],
    parallel: 32,
    batch: 256,
    ubatch: 128
  });
  assert.equal(options.batching.max_sequences, 1);
  assert.deepEqual(options.batching.unverified_nodes, ['p4-gpu-3090', 'p4-gpu-4080']);
});

test('an explicit override verifies a stage the profile does not cover', async () => {
  const options = await measuredModelLoadOptions({
    model: 'Qwen2.5-1.5B-Instruct-Q8_0.gguf',
    nodes: [{ id: 'p4-gpu-3090' }],
    parallel: 8,
    batch: 256,
    ubatch: 128,
    nodeBatchLimits: { 'p4-gpu-3090': 8 }
  });
  assert.equal(options.batching.max_sequences, 8);
  assert.deepEqual(options.batching.unverified_nodes, []);
});

import assert from 'node:assert/strict';
import test from 'node:test';
import { compatibleRingCapabilities } from './pipeline-capabilities.mjs';

const inventory = (agentId, adapterAbi, capabilityBits) => ({ endpoint: `${agentId}:19201`, agentId, snapshot: { adapters: [{ adapter_id: 'adapter-local', descriptor: JSON.stringify({ schema: 'p4.adapter-capability/v1', pipeline_runtime: { available: true, protocol: 'linker-stage-v1', adapter_abi: adapterAbi, capability_bits: capabilityBits } }) }] } });

test('accepts equal native Pipeline contracts', () => {
  assert.equal(compatibleRingCapabilities([inventory('a', 16, 8081), inventory('b', 16, 8081)]).length, 2);
});

test('rejects ABI drift before a model load', () => {
  assert.throws(() => compatibleRingCapabilities([inventory('a', 16, 8081), inventory('b', 15, 913)]), /model load was not attempted/);
});

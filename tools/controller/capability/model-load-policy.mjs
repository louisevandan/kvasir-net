import { readFile } from 'node:fs/promises';

const profileUrl = new URL('./measured-batch-profiles.json', import.meta.url);

function parseNodeLimits(nodeBatchLimits, nodes) {
  const entries = nodeBatchLimits === undefined ? [] : Object.entries(nodeBatchLimits);
  const nodeIds = new Set(nodes.map((node) => node.id));
  const normalized = {};

  for (const [nodeId, rawLimit] of entries) {
    if (typeof nodeId !== 'string' || !nodeId.trim()) {
      throw new Error('nodeBatchLimits keys must be non-empty node IDs');
    }
    const limit = Number(rawLimit);
    if (!Number.isInteger(limit) || limit < 1) {
      throw new Error(`nodeBatchLimits[${nodeId}] must be a positive integer`);
    }
    if (!nodeIds.has(nodeId)) {
      throw new Error(`nodeBatchLimits references unknown node ${nodeId}`);
    }
    if (normalized[nodeId] !== undefined && normalized[nodeId] !== limit) {
      throw new Error(`nodeBatchLimits has conflicting limits for ${nodeId}`);
    }
    normalized[nodeId] = limit;
  }

  return normalized;
}

export async function measuredModelLoadOptions({
  model,
  nodes,
  parallel,
  batch,
  ubatch,
  flashAttention = 'enabled',
  mmap = false,
  cacheTypeK = 'q8_0',
  cacheTypeV = 'q8_0',
  kvOffload = true,
  adapterOptions = {},
  nodeBatchLimits
}) {
  const document = JSON.parse(await readFile(profileUrl, 'utf8'));
  const normalizedNodeLimits = parseNodeLimits(nodeBatchLimits, nodes);
  const normalizedModel = model.toLowerCase();
  const profile = document.profiles.find((candidate) =>
    normalizedModel.includes(candidate.model_pattern.toLowerCase())
  );
  const terms = [
    { name: 'requested_parallel', value: parallel, source: 'controller request' },
    { name: 'context_batch_tokens', value: batch, source: 'controller model-load option' },
    { name: 'context_ubatch_tokens', value: ubatch, source: 'controller model-load option' }
  ];
  const nodeLimits = [];
  const calculationTerms = [...terms];
  for (const node of nodes) {
    const measured = profile?.node_limits.find((candidate) =>
      node.id.toLowerCase().includes(candidate.node_pattern.toLowerCase())
    );
    const overrideValue = normalizedNodeLimits[node.id];
    const overrideTerm = overrideValue === undefined
      ? undefined
      : {
          name: `override_stage:${node.id}`,
          value: overrideValue,
          source: `node batch override from controller policy input: ${overrideValue}`
        };
    const measuredTerm = measured
      ? {
          name: `verified_stage:${node.id}`,
          value: measured.max_sequences,
          source: `${measured.device}; ${measured.evidence}; ${profile.source}`
        }
      : {
          name: `unverified_stage:${node.id}`,
          value: 1,
          source: 'unverified stage; no matching measured profile; conservative compatibility limit'
        };
    const nodeTerms = [...terms.slice(0, 3), measuredTerm];
    if (overrideTerm !== undefined) nodeTerms.push(overrideTerm);
    const nodeResult = Math.min(...nodeTerms.map((term) => term.value));
    calculationTerms.push({ name: `effective_node_limit:${node.id}`, value: nodeResult, source: nodeTerms[nodeTerms.length - 1].source });
    nodeLimits.push({
      node_id: node.id,
      max_sequences: nodeResult,
      calculation: { method: 'minimum', terms: nodeTerms, result: nodeResult }
    });
  }
  const result = Math.min(...calculationTerms.map((term) => term.value));
  return {
    flash_attention: flashAttention,
    mmap,
    kv_cache: { type_k: cacheTypeK, type_v: cacheTypeV, offload: kvOffload },
    batching: {
      strategy: 'ready-queue-dynamic',
      max_sequences: result,
      node_limits: nodeLimits,
      context_batch_tokens: batch,
      context_ubatch_tokens: ubatch,
      calculation: { method: 'minimum', terms: calculationTerms, result }
    },
    adapter_options: adapterOptions
  };
}

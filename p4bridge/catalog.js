'use strict';
/**
 * The model catalog.
 *
 * p4 carries no model names: an agent snapshot lists node ids, generations and
 * lifecycle state, nothing else. So the mapping from an OpenAI-style model name
 * to a pipeline of loaded stages lives here, in a file an operator edits, and
 * every entry is checked against a live INSPECT before it is served.
 *
 * catalog.json:
 * {
 *   "ingress_agent": "tcp://127.0.0.1:42011",
 *   "models": [{
 *     "id": "step-3.7-flash",
 *     "name": "Step-3.7-Flash (428B MoE)",
 *     "load_generation": 1,
 *     "stages": [
 *       {"agent": "tcp://127.0.0.1:42011", "node": "step37-s0", "generation": 1},
 *       {"agent": "tcp://127.0.0.1:42011", "node": "step37-s1", "generation": 1}
 *     ]
 *   }]
 * }
 */
const fs = require('node:fs');
const path = require('node:path');

function load(file) {
  const resolved = path.resolve(file);
  const raw = JSON.parse(fs.readFileSync(resolved, 'utf8'));
  if (!raw.ingress_agent) throw new Error('catalog needs an ingress_agent');
  const models = (raw.models ?? []).map((model) => {
    if (!model.id || !Array.isArray(model.stages) || model.stages.length < 1) {
      throw new Error(`catalog model ${model.id ?? '(unnamed)'} needs an id and at least one stage`);
    }
    for (const stage of model.stages) {
      if (!stage.agent || !stage.node || !stage.generation) {
        throw new Error(`catalog model ${model.id} has an incomplete stage`);
      }
    }
    return {
      id: model.id,
      name: model.name ?? model.id,
      loadGeneration: model.load_generation ?? model.stages[0].generation,
      stages: model.stages.map((stage) => ({
        agent: stage.agent, node: stage.node, generation: stage.generation,
      })),
      contextSize: model.context_size ?? null,
      maxTokens: model.max_tokens ?? 1024,
      options: model.options ?? '',
    };
  });
  return { file: resolved, ingressAgent: raw.ingress_agent, models };
}

/**
 * Confirm a model's stages are loaded right now.
 *
 * Returns `{ serving, stages: [{node, agent, found, state}] }`. A stage that
 * INSPECT does not list, or lists in another state, makes the model not
 * serving: better an empty catalog than a model that 500s on first call.
 */
async function verify(model, snapshots) {
  const stages = model.stages.map((stage) => {
    const snapshot = snapshots.get(stage.agent);
    const node = snapshot?.nodes?.find((row) => row.node_id === stage.node);
    return {
      agent: stage.agent,
      node: stage.node,
      generation: stage.generation,
      found: Boolean(node),
      state: node?.state ?? null,
      generationMatches: node ? Number(node.generation) === Number(stage.generation) : false,
      adapterKind: node?.adapter_kind ?? null,
    };
  });
  const serving = stages.every((stage) => stage.found && stage.state === 'loaded' && stage.generationMatches);
  return { serving, stages };
}

/** Every distinct agent address a catalog refers to. */
function agents(catalog) {
  const set = new Set([catalog.ingressAgent]);
  for (const model of catalog.models) for (const stage of model.stages) set.add(stage.agent);
  return [...set];
}

module.exports = { load, verify, agents };

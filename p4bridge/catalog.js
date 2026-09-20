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
 *     "load_generation": 1789148231396,       // from the placement plan
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
    // The load generation is the operator's, and it cannot be guessed. The
    // adapter compares it for exact equality and rejects a mismatch — which
    // leaves every stage reporting `failed:session load generation is stale`
    // until the model is loaded again. A node's own `generation` is a different
    // number; using it as a fallback is how that mistake gets made. So the
    // catalog must carry the value from the placement plan, or serve nothing.
    if (typeof model.load_generation !== 'number' || !Number.isInteger(model.load_generation) || model.load_generation <= 0) {
      throw new Error(
        `catalog model ${model.id} needs load_generation from the placement plan ` +
        '(an integer the loader set; it is not discoverable over OUTER and must not be guessed)',
      );
    }
    return {
      id: model.id,
      name: model.name ?? model.id,
      loadGeneration: model.load_generation,
      stages: model.stages.map((stage) => ({
        agent: stage.agent, node: stage.node, generation: stage.generation,
      })),
      contextSize: model.context_size ?? null,
      maxTokens: model.max_tokens ?? 1024,
      options: model.options ?? '',
      // p4 hands the stage server an opaque prompt and applies no chat
      // template of its own — the staged adapter carries only a probe for
      // reading one out of a GGUF. Rendering the model's turn format is
      // therefore OUTER's job, and it has to be stated per model: a ChatML
      // template rendered for a Llama-3 model produces fluent nonsense, not
      // an error. Unstated means 'raw', which is what this bridge did before
      // the field existed.
      promptFormat: model.prompt_format ?? 'raw',
      // A reasoning model opens its reply with a thinking block. Kept out of
      // `content` so a chat client shows the answer, and returned alongside.
      reasoning: model.reasoning === true,
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
    const reported = node?.load_generation ?? null;
    return {
      agent: stage.agent,
      node: stage.node,
      generation: stage.generation,
      found: Boolean(node),
      state: node?.state ?? null,
      generationMatches: node ? Number(node.generation) === Number(stage.generation) : false,
      // Present only on engines that report it. When it is there it is the
      // whole answer; when it is not, we are guessing and say so.
      loadGeneration: reported,
      holdsOurLoad: reported === null ? null : Number(reported) === Number(model.loadGeneration),
    };
  });

  // The protocol document is explicit that `state` is the adapter's opaque
  // vocabulary and that callers must display it without interpreting it. This
  // used to gate serving on `state === 'loaded'`, which is exactly that — and it
  // takes a model out of service for a string that one refused command rewrote,
  // even though the stages still hold the load and would accept a correct
  // session. So serving is decided by the load the node reports holding.
  const reports = stages.every((stage) => stage.loadGeneration !== null);
  const serving = reports
    ? stages.every((stage) => stage.found && stage.holdsOurLoad)
    // Older engines do not report it. Then the most that can be said is that the
    // stage is registered at the generation the catalog names.
    : stages.every((stage) => stage.found && stage.generationMatches);
  return { serving, stages, loadGenerationReported: reports };
}

/** Every distinct agent address a catalog refers to. */
function agents(catalog) {
  const set = new Set([catalog.ingressAgent]);
  for (const model of catalog.models) for (const stage of model.stages) set.add(stage.agent);
  return [...set];
}

module.exports = { load, verify, agents };

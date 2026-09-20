#!/usr/bin/env node
/**
 * Drive a load, and write down the number that makes it usable.
 *
 * The llamacpp adapter compares `load_generation` for exact equality on every
 * session, inference, settlement — and on UNLOAD. It is chosen by whoever
 * loads, is not in any snapshot, and OUTER cannot ask for it. Lose it and the
 * model is stranded: it will refuse every session, and you cannot even unload
 * it to start again.
 *
 * That is not hypothetical. A wrong value put four live stages into
 * `failed:session load generation is stale`, and the real number existed
 * nowhere on the machines. So this tool has one rule above the rest:
 *
 *   the generation is written to disk BEFORE the first LOAD leaves,
 *
 * so a load that fails halfway still leaves the operator able to unload.
 *
 *   node load.mjs --plan plan.json --confirm     load, then record the catalog
 *   node load.mjs --unload --confirm             unload what the record names
 *   node load.mjs --plan plan.json --dry-run     print what would be sent
 *
 * This loads a model on real machines. It refuses to act without --confirm.
 */
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import path from 'node:path';
import { connect, agentEndpoint } from './wire.js';

const HERE = path.dirname(new URL(import.meta.url).pathname);
const STATE_DIR = path.join(HERE, 'state');
const RECORD = path.join(STATE_DIR, 'last-load.json');
const CATALOG = process.env.P4_BRIDGE_CATALOG ?? path.join(HERE, 'catalog.json');

const ADAPTER = 'llamacpp';

// LOAD and UNLOAD are backend-neutral node lifecycle commands addressed to the
// AGENT, not to the node: the node does not exist until LOAD creates it, and an
// event for an unregistered node is forwarded outbound instead of handled
// (layers/agent/src/event_broker/mod.rs:466). The adapter's own command rides
// inside as an opaque body. Sending the bare llamacpp command to a node
// endpoint — what this tool did before — is silently dropped.
const NODE_LOAD = 'application/vnd.p4.node.load-v1';
const NODE_UNLOAD = 'application/vnd.p4.node.unload-v1';
const NODE_RESULT = 'application/vnd.p4.node.lifecycle-result-v1';
const LIFECYCLE_SCHEMA = 1;

const LOAD = 'application/vnd.p4.llamacpp.load-v4+json';
const LOADED = 'application/vnd.p4.llamacpp.loaded-v4+json';
const UNLOAD = 'application/vnd.p4.llamacpp.unload-v3+json';
const UNLOADED = 'application/vnd.p4.llamacpp.unloaded-v3+json';

// The allocation the retired CREATE path used; the adapter's resource profile
// enforces the smaller per-request limits within it.
const QUEUE_CAPACITY = 65_536;
const COMPLETION_CAPACITY = 65_536;
const RETAINED_CAPACITY = 65_536;
const RETAINED_BYTES = 256 * 1024 * 1024;

const argv = process.argv.slice(2);
const flag = (name) => argv.includes(`--${name}`);
const value = (name) => { const i = argv.indexOf(`--${name}`); return i >= 0 ? argv[i + 1] : null; };

const json = (v) => Buffer.from(JSON.stringify(v), 'utf8');
const nodeAddress = (agent) => agent.replace(/^tcp:\/\//, '').split(':');

/** Lifecycle payload: u32le metadata length, the metadata JSON, then the body. */
function encodeLifecycle(metadata, opaque) {
  const meta = json(metadata);
  const length = Buffer.alloc(4);
  length.writeUInt32LE(meta.length, 0);
  return Buffer.concat([length, meta, opaque]);
}

function decodeLifecycle(payload) {
  if (payload.length < 4) throw new Error('lifecycle payload is truncated');
  const length = payload.readUInt32LE(0);
  if (payload.length < 4 + length) throw new Error('lifecycle metadata is truncated');
  return {
    metadata: JSON.parse(payload.subarray(4, 4 + length).toString('utf8')),
    opaque: payload.subarray(4 + length),
  };
}

/** The lifecycle envelope for one stage's LOAD or UNLOAD. */
function lifecycleMetadata(stage, operation) {
  const allocating = operation === 'load';
  return {
    schema: LIFECYCLE_SCHEMA,
    node_id: stage.node,
    node_generation: stage.generation,
    adapter_kind: ADAPTER,
    adapter_content_type: allocating ? LOAD : UNLOAD,
    ...(allocating
      ? {
          queue_capacity: QUEUE_CAPACITY,
          completion_capacity: COMPLETION_CAPACITY,
          retained_capacity: RETAINED_CAPACITY,
          retained_bytes: RETAINED_BYTES,
        }
      : {}),
  };
}

/** One connection per agent; a stage is only reachable through its own. */
async function clientsFor(agents) {
  const clients = new Map();
  for (const agent of agents) {
    const [host, port] = nodeAddress(agent);
    clients.set(agent, await connect({
      host, port: Number(port), address: agent,
      channel: `kvr-load-${Date.now().toString(16)}`, deadlineMs: 1_800_000,
    }));
  }
  return clients;
}

/**
 * Wait for every stage's lifecycle result.
 *
 * Every stage is heard out rather than aborting on the first refusal. One
 * stage's rejection usually means all four share the cause, and each carries
 * its own detail — the physical result bound, for one, is only knowable from
 * what the stage server reports at READY, so a run that collects all four
 * failures tells you every number you need to correct the plan.
 */
function awaitAll(clients, correlationId, wanted, stages, timeoutMs) {
  return new Promise((resolve, reject) => {
    const seen = new Set();
    const failures = [];
    const offs = [...clients.values()].map((client) => client.subscribe(correlationId, (event) => {
      if (event.error) return finish(event.error);
      if (event.meta.contentType !== NODE_RESULT) return;
      let metadata; let opaque;
      try { ({ metadata, opaque } = decodeLifecycle(event.payload)); }
      catch (error) { return finish(error); }
      const node = metadata.node_id ?? `stage-${seen.size}`;
      if (seen.has(node)) return;
      seen.add(node);
      if (metadata.status !== 'succeeded' || metadata.adapter_content_type !== wanted) {
        const detail = metadata.first_error
          ?? metadata.cleanup_error
          ?? opaque.toString('utf8').slice(0, 400)
          ?? metadata.status;
        failures.push(`${node}: ${metadata.status} (${metadata.resource_state}) ${detail}`);
      }
      if (seen.size < stages.length) return;
      if (failures.length) return finish(new Error(`\n  ${failures.join('\n  ')}`));
      finish(null, [...seen]);
    }));
    const timer = setTimeout(
      () => finish(new Error(
        `timed out with ${seen.size}/${stages.length} stages answering`
        + (failures.length ? `\n  ${failures.join('\n  ')}` : ''),
      )),
      timeoutMs,
    );
    function finish(error, result) {
      clearTimeout(timer);
      for (const off of offs) off();
      if (error) reject(error); else resolve(result);
    }
  });
}

function recordGeneration(generation, plan) {
  mkdirSync(STATE_DIR, { recursive: true });
  writeFileSync(RECORD, JSON.stringify({
    load_generation: generation,
    at: new Date().toISOString(),
    ingress_agent: plan.ingress_agent,
    model: plan.model?.id ?? null,
    stages: plan.stages.map((stage) => ({ agent: stage.agent, node: stage.node, generation: stage.generation })),
    note: 'Written before the first LOAD. If the load failed halfway, this is the number UNLOAD needs.',
  }, null, 2) + '\n');
}

async function doLoad() {
  const planFile = value('plan');
  if (!planFile) throw new Error('--plan <file> is required');
  const plan = JSON.parse(readFileSync(path.resolve(planFile), 'utf8'));
  if (!plan.stages?.length) throw new Error('the plan has no stages');

  // The load generation is not free: the adapter compares a RELEASE receipt's
  // source node generation against the receipt's load_generation and stops the
  // node when they differ (v2/transport_owners.rs:1216). A ring loaded with a
  // load generation that is not its node generation serves one request and
  // then loses its head, which reads like a crash rather than a mismatch. So
  // the plan's node generation is the load generation, and a disagreement is
  // refused here rather than discovered after the first request.
  const nodeGenerations = new Set(plan.stages.map((stage) => stage.generation));
  if (nodeGenerations.size !== 1) {
    throw new Error(`every stage must share one node generation; the plan has ${[...nodeGenerations].join(', ')}`);
  }
  const [nodeGeneration] = nodeGenerations;
  const generation = Number(value('generation') ?? plan.load_generation ?? nodeGeneration);
  if (!Number.isInteger(generation) || generation <= 0) throw new Error('generation must be a positive integer');
  if (generation !== nodeGeneration) {
    throw new Error(
      `load generation ${generation} differs from the plan's node generation ${nodeGeneration}; `
      + 'they are one number, and a ring loaded with two loses its head on the first release',
    );
  }

  const commands = plan.stages.map((stage) => ({
    stage,
    payload: {
      load_generation: generation,
      binary: stage.binary ?? plan.defaults?.binary,
      endpoint: stage.endpoint ?? plan.defaults?.endpoint,
      plan: stage.plan ?? plan.defaults?.plan,
      args: stage.args ?? plan.defaults?.args ?? [],
      environment: stage.environment ?? plan.defaults?.environment ?? [],
      n_batch: stage.n_batch ?? plan.defaults?.n_batch,
      n_ubatch: stage.n_ubatch ?? plan.defaults?.n_ubatch,
      context_size: stage.context_size ?? plan.defaults?.context_size,
      total_context_size: stage.total_context_size ?? plan.defaults?.total_context_size,
      sequence_capacity: stage.sequence_capacity ?? plan.defaults?.sequence_capacity,
      // The adapter checks this against the stage server's own READY report and
      // refuses any mismatch, so it belongs in the plan as data, not here.
      resource_profile: stage.resource_profile ?? plan.defaults?.resource_profile,
      ready_timeout_ms: stage.ready_timeout_ms ?? plan.defaults?.ready_timeout_ms ?? 900_000,
      io_timeout_ms: stage.io_timeout_ms ?? plan.defaults?.io_timeout_ms ?? 120_000,
    },
  }));

  for (const { stage, payload } of commands) {
    for (const [key, v] of Object.entries(payload)) {
      if (v === undefined || v === null) throw new Error(`stage ${stage.node} is missing ${key}`);
    }
  }

  if (flag('dry-run')) {
    console.log(`load_generation ${generation} (not sent)`);
    for (const { stage, payload } of commands) {
      console.log(`  ${stage.node} @ ${stage.agent} → ${payload.binary} ${payload.args.join(' ')}`);
    }
    return;
  }
  if (!flag('confirm')) throw new Error('refusing to load without --confirm');

  // Before anything leaves: the number, on disk.
  recordGeneration(generation, plan);
  console.log(`load_generation ${generation} recorded in ${path.relative(process.cwd(), RECORD)}`);

  const clients = await clientsFor(new Set(plan.stages.map((stage) => stage.agent)));
  const correlationId = `load-${generation}`;
  const waiting = awaitAll(clients, correlationId, LOADED, plan.stages,
    Math.max(...commands.map(({ payload }) => payload.ready_timeout_ms)) + 60_000);

  for (const { stage, payload } of commands) {
    clients.get(stage.agent).send(
      agentEndpoint(stage.agent),
      NODE_LOAD, encodeLifecycle(lifecycleMetadata(stage, 'load'), json(payload)),
      { adapterKind: ADAPTER, eventClass: 'control', correlationId },
    );
    console.log(`  LOAD → ${stage.node}`);
  }

  try {
    const loaded = await waiting;
    console.log(`loaded: ${loaded.join(', ')}`);
  } catch (error) {
    console.error(`load did not complete: ${error.message}`);
    console.error(`the generation is in ${RECORD} — UNLOAD needs it, do not delete it`);
    process.exitCode = 1;
    return;
  } finally {
    for (const client of clients.values()) client.close?.();
  }

  // Only now does the catalog claim this model serves.
  const catalog = existsSync(CATALOG) ? JSON.parse(readFileSync(CATALOG, 'utf8')) : { models: [] };
  catalog.ingress_agent = plan.ingress_agent;
  const entry = {
    id: plan.model.id,
    name: plan.model.name ?? plan.model.id,
    load_generation: generation,
    context_size: plan.model.context_size ?? null,
    max_tokens: plan.model.max_tokens ?? 512,
    n_embd: plan.model.n_embd ?? null,
    n_layer: plan.model.n_layer ?? null,
    n_expert: plan.model.n_expert ?? null,
    prompt_format: plan.model.prompt_format ?? 'raw',
    reasoning: plan.model.reasoning === true,
    stages: plan.stages.map((stage) => ({ agent: stage.agent, node: stage.node, generation: stage.generation })),
  };
  const index = (catalog.models ?? []).findIndex((model) => model.id === entry.id);
  if (index >= 0) catalog.models[index] = entry; else (catalog.models ??= []).push(entry);
  writeFileSync(CATALOG, JSON.stringify(catalog, null, 2) + '\n');
  console.log(`catalog updated: ${entry.id} at load generation ${generation}`);
}

async function doUnload() {
  // Normally the record written before the load names both the generation and
  // the stages. When there is no record — a load someone else drove — the
  // generation has to be supplied by whoever drove it, and the stages come from
  // the catalog. This is the only way back for a model loaded from elsewhere.
  let record;
  if (existsSync(RECORD)) {
    record = JSON.parse(readFileSync(RECORD, 'utf8'));
  } else {
    const supplied = Number(value('generation'));
    if (!Number.isInteger(supplied) || supplied <= 0) {
      throw new Error(
        `no load record at ${RECORD}. The generation is not recoverable from the machines — ` +
        'pass --generation <n> from whoever drove the load, and the stages will be read from the catalog',
      );
    }
    if (!existsSync(CATALOG)) throw new Error('no catalog to read the stages from');
    const catalog = JSON.parse(readFileSync(CATALOG, 'utf8'));
    const model = (catalog.models ?? []).find((entry) => !value('model') || entry.id === value('model'));
    if (!model) throw new Error('the catalog has no model to unload');
    record = { load_generation: supplied, model: model.id, stages: model.stages };
    console.log(`no record; unloading ${model.id} at the generation you supplied`);
  }
  const generation = Number(value('generation') ?? record.load_generation);
  if (!Number.isInteger(generation) || generation <= 0) throw new Error('no usable load generation');

  if (flag('dry-run')) {
    console.log(`would UNLOAD generation ${generation} on ${record.stages.map((s) => s.node).join(', ')} (not sent)`);
    return;
  }
  if (!flag('confirm')) throw new Error('refusing to unload without --confirm');

  const clients = await clientsFor(new Set(record.stages.map((stage) => stage.agent)));
  const correlationId = `unload-${generation}`;
  const waiting = awaitAll(clients, correlationId, UNLOADED, record.stages, 300_000);
  for (const stage of record.stages) {
    clients.get(stage.agent).send(
      agentEndpoint(stage.agent),
      NODE_UNLOAD,
      encodeLifecycle(lifecycleMetadata(stage, 'unload'), json({ load_generation: generation })),
      { adapterKind: ADAPTER, eventClass: 'control', correlationId },
    );
    console.log(`  UNLOAD → ${stage.node}`);
  }
  try {
    const done = await waiting;
    console.log(`unloaded: ${done.join(', ')}`);
  } finally {
    for (const client of clients.values()) client.close?.();
  }

  // The catalog must stop claiming a model that is no longer loaded.
  if (existsSync(CATALOG)) {
    const catalog = JSON.parse(readFileSync(CATALOG, 'utf8'));
    for (const model of catalog.models ?? []) {
      if (model.id === record.model) model.load_generation = null;
    }
    writeFileSync(CATALOG, JSON.stringify(catalog, null, 2) + '\n');
    console.log('catalog: load generation cleared');
  }
}

(flag('unload') ? doUnload() : doLoad()).catch((error) => {
  console.error(error.message);
  process.exit(1);
});

'use strict';
/**
 * p4 bridge — the HTTP face of the p4 engine.
 *
 * The settlement gateway (solana/staking-service) speaks the linkcpp hub's
 * HTTP contract: a controller catalog, `/c/{id}/v1/chat/completions`, a runtime
 * summary and a contribution ledger. p4 speaks none of that: it is a TCP event
 * protocol with no model names, no token counts in a reply, and no HTTP at all.
 *
 * This service implements the hub contract on top of p4, so the money path
 * keeps its shape while the engine underneath changes:
 *
 *   GET  /api/controllers                 catalog, verified against live INSPECT
 *   GET  /api/runtime                     operator wallet + engine summary
 *   GET  /api/contributions               per-node units from stage rows
 *   POST /c/:id/v1/chat/completions       OpenAI chat, streaming or not
 *   GET  /c/:id/v1/models                 the one model that controller serves
 *   POST /api/controllers/:id/unload      refused: placement is an operator act
 *   GET  /health                          liveness for the container
 *
 * Deliberately not implemented: LOAD/serve. p4 loads a model from a placement
 * plan an operator prepares; inventing one from an HTTP call would put the
 * engine's integrity gates in the hands of a web request.
 */
const http = require('node:http');
const crypto = require('node:crypto');
const { connect } = require('./wire');
const { Pipeline } = require('./pipeline');
const catalogModule = require('./catalog');

const UNITS_PER_ROW = Number(process.env.P4_BRIDGE_UNITS_PER_KTOKEN ?? 1) / 1000;
// Optional shared secret. p4 itself has no auth, so when the bridge is not on
// loopback this is the only thing between the engine and the network. Unset in
// a private deployment; set it and every path but /health needs the header.
const SERVICE_TOKEN = (process.env.P4_BRIDGE_TOKEN ?? '').trim();
const HEADERS = ['x-kvasir-service-token', 'x-linkcpp-service-token'];

function authorized(req) {
  if (!SERVICE_TOKEN) return true;
  const presented = HEADERS.map((name) => req.headers[name])
    .concat((req.headers.authorization ?? '').replace(/^Bearer\s+/i, ''))
    .find((value) => typeof value === 'string' && value.length);
  if (!presented) return false;
  const a = Buffer.from(presented);
  const b = Buffer.from(SERVICE_TOKEN);
  return a.length === b.length && crypto.timingSafeEqual(a, b);
}

class Bridge {
  constructor({ catalogFile, operatorWallet = '', inspectIntervalMs = 15_000 }) {
    this.catalogFile = catalogFile;
    this.operatorWallet = operatorWallet;
    this.inspectIntervalMs = inspectIntervalMs;
    this.catalog = catalogModule.load(catalogFile);
    this.snapshots = new Map();       // agent address -> snapshot
    this.serving = new Map();         // model id -> {serving, stages}
    this.pipelines = new Map();       // model id -> Pipeline
    this.contributions = new Map();   // node id -> {units, rows, requests, agent}
    this.clients = new Map();         // agent address -> OUTER client
    this.lastInspectError = null;
    this.startedAt = Date.now();
  }

  /**
   * One OUTER connection per agent, reconnected on loss.
   *
   * An agent answers only for its own nodes — INSPECT for a peer times out — so
   * a pipeline that spans machines needs a connection to each of them.
   */
  async ensureClient(agentAddress) {
    const agent = agentAddress ?? this.catalog.ingressAgent;
    const existing = this.clients.get(agent);
    if (existing && !existing.closed) return existing;
    const [host, port] = agent.replace(/^tcp:\/\//, '').split(':');
    const client = await connect({
      host,
      port: Number(port),
      address: agent,
      channel: `kvr-bridge-${crypto.randomBytes(6).toString('hex')}`,
      deadlineMs: 300_000,
    });
    this.clients.set(agent, client);
    this.pipelines.clear();           // sessions do not survive a new connection
    return client;
  }

  /** Drop every connection; the next refresh rebuilds them. */
  resetClients() {
    for (const client of this.clients.values()) client.close?.();
    this.clients.clear();
    this.pipelines.clear();
  }

  async refresh() {
    try {
      for (const agent of catalogModule.agents(this.catalog)) {
        const client = await this.ensureClient(agent);
        this.snapshots.set(agent, await client.inspect(agent, { timeoutMs: 10_000 }));
      }
      for (const model of this.catalog.models) {
        this.serving.set(model.id, await catalogModule.verify(model, this.snapshots));
      }
      this.lastInspectError = null;
    } catch (error) {
      this.lastInspectError = error.message;
      for (const model of this.catalog.models) this.serving.set(model.id, { serving: false, stages: [] });
      this.resetClients();
    }
  }

  async pipelineFor(model) {
    const existing = this.pipelines.get(model.id);
    if (existing) return existing;
    for (const agent of new Set(model.stages.map((stage) => stage.agent))) await this.ensureClient(agent);
    const pipeline = new Pipeline({
      clientFor: (agent) => this.clients.get(agent),
      stages: model.stages,
      loadGeneration: model.loadGeneration,
    });
    await pipeline.install();
    this.pipelines.set(model.id, pipeline);
    return pipeline;
  }

  /**
   * Credit the rows each stage actually ran; the gateway reads these units.
   *
   * The perf tier wants tok/s. A pipeline decodes in lockstep, so the request's
   * own decode rate is every participating stage's rate — reported as a rolling
   * mean so one short request does not set a node's tier.
   */
  recordContribution(model, stageRows, { completionTokens = 0, elapsedMs = 0 } = {}) {
    const tps = elapsedMs > 0 && completionTokens > 0 ? (completionTokens / (elapsedMs / 1000)) : null;
    for (const [node, rows] of Object.entries(stageRows)) {
      const stage = model.stages.find((entry) => entry.node === node) ?? model.stages[0];
      const current = this.contributions.get(node) ?? { units: 0, rows: 0, requests: 0, agent: stage.agent, tps: null };
      current.rows += rows;
      current.units += rows * UNITS_PER_ROW;
      current.requests += 1;
      if (tps !== null) current.tps = current.tps === null ? tps : current.tps * 0.7 + tps * 0.3;
      this.contributions.set(node, current);
    }
  }

  controllers() {
    return this.catalog.models.map((model) => {
      const state = this.serving.get(model.id) ?? { serving: false, stages: [] };
      return {
        id: model.id,
        name: model.name,
        active_model: model.id,
        serving: state.serving,
        runtime_loaded: state.serving,
        phase: state.serving ? 'serving' : 'unloaded',
        // Shaped like the hub's last_load so a caller that reads `.model` still
        // finds one; on p4 the load itself came from an operator placement plan.
        last_load: { model: model.id, generation: model.loadGeneration, placement: 'external' },
        engine: 'p4',
        stages: state.stages,
      };
    });
  }
}

/* ---- HTTP -------------------------------------------------------------- */

const send = (res, status, body, headers = {}) => {
  const payload = typeof body === 'string' ? body : JSON.stringify(body);
  res.writeHead(status, { 'content-type': 'application/json; charset=utf-8', ...headers });
  res.end(payload);
};

const readBody = (req, limit = 8 * 1024 * 1024) => new Promise((resolve, reject) => {
  const chunks = [];
  let size = 0;
  req.on('data', (chunk) => {
    size += chunk.length;
    if (size > limit) { reject(new Error('request body too large')); req.destroy(); return; }
    chunks.push(chunk);
  });
  req.on('end', () => resolve(Buffer.concat(chunks)));
  req.on('error', reject);
});

/** The prompt an OpenAI chat body asks for, flattened the way the adapter takes it. */
function promptFrom(body) {
  if (typeof body.prompt === 'string') return body.prompt;
  const messages = Array.isArray(body.messages) ? body.messages : [];
  return messages
    .map((message) => {
      const content = Array.isArray(message.content)
        ? message.content.map((part) => part.text ?? '').join('')
        : String(message.content ?? '');
      const role = message.role ?? 'user';
      return role === 'user' ? content : `${role}: ${content}`;
    })
    .join('\n\n');
}

function chunkFrame(id, model, delta, finishReason = null) {
  return `data: ${JSON.stringify({
    id, object: 'chat.completion.chunk', created: Math.floor(Date.now() / 1000), model,
    choices: [{ index: 0, delta, finish_reason: finishReason }],
  })}\n\n`;
}

function createServer(bridge) {
  return http.createServer(async (req, res) => {
    const url = new URL(req.url, 'http://bridge.local');
    const path = url.pathname.replace(/\/+$/, '') || '/';
    try {
      if (path !== '/health' && path !== '/api/health' && !authorized(req)) {
        return send(res, 401, { error: { message: 'service token required', type: 'unauthorized' } });
      }
      if (req.method === 'GET' && (path === '/health' || path === '/api/health')) {
        const serving = [...bridge.serving.values()].filter((state) => state.serving).length;
        return send(res, serving > 0 ? 200 : 503, {
          status: serving > 0 ? 'ok' : 'unhealthy',
          service: 'p4-bridge',
          engine: 'p4',
          serving_models: serving,
          inspect_error: bridge.lastInspectError,
          uptime_ms: Date.now() - bridge.startedAt,
        });
      }

      if (req.method === 'GET' && path === '/api/controllers') {
        return send(res, 200, { controllers: bridge.controllers() });
      }

      if (req.method === 'GET' && path === '/api/runtime') {
        const machines = [...bridge.snapshots.entries()].map(([agent, snapshot]) => ({
          agent,
          gpus: snapshot?.machine?.capability?.gpus?.map((gpu) => ({
            name: gpu.name, backend: gpu.backend, memory_total_bytes: gpu.memory_total_bytes,
          })) ?? [],
          nodes: snapshot?.nodes?.map((node) => ({
            node_id: node.node_id, state: node.state, adapter_kind: node.adapter_kind,
          })) ?? [],
        }));
        return send(res, 200, {
          engine: 'p4',
          operator_wallet: bridge.operatorWallet,
          ingress_agent: bridge.catalog.ingressAgent,
          machines,
          inspect_error: bridge.lastInspectError,
        });
      }

      if (req.method === 'GET' && path === '/api/contributions') {
        const contributions = [...bridge.contributions.entries()].map(([node, entry]) => ({
          node_id: node,
          owner: bridge.operatorWallet,
          units: Number(entry.units.toFixed(6)),
          node_name: node,
          backend: 'p4',
          os: 'linux',
          accelerator: 'gpu',
          device_kind: 'server',
          perf_tps: entry.tps === null || entry.tps === undefined ? null : Number(entry.tps.toFixed(2)),
          rows: entry.rows,
          requests: entry.requests,
          agent: entry.agent,
        }));
        return send(res, 200, { contributions });
      }

      const controllerMatch = path.match(/^\/c\/([^/]+)(\/.*)$/);
      if (controllerMatch) {
        const [, id, rest] = controllerMatch;
        const model = bridge.catalog.models.find((entry) => entry.id === id);
        if (!model) return send(res, 404, { error: { message: `unknown controller ${id}` } });
        const state = bridge.serving.get(model.id);
        if (!state?.serving) {
          return send(res, 503, {
            error: { message: `controller ${id} is not serving`, type: 'ring_recovering', stages: state?.stages ?? [] },
          });
        }
        if (req.method === 'GET' && rest === '/v1/models') {
          return send(res, 200, { object: 'list', data: [{ id: model.id, object: 'model', owned_by: 'kvasir', name: model.name }] });
        }
        if (req.method === 'POST' && rest === '/v1/chat/completions') {
          return await chatCompletions(bridge, model, req, res);
        }
        return send(res, 404, { error: { message: `unsupported controller path ${rest}` } });
      }

      if (req.method === 'POST' && /^\/api\/controllers\/[^/]+\/(serve|unload)$/.test(path)) {
        return send(res, 409, {
          error: {
            message: 'p4 placement is an operator action: load stages with the placement plan, then update the bridge catalog',
            type: 'placement_is_external',
          },
        });
      }

      return send(res, 404, { error: { message: `unknown path ${path}` } });
    } catch (error) {
      return send(res, 500, { error: { message: error.message } });
    }
  });
}

async function chatCompletions(bridge, model, req, res) {
  const body = JSON.parse((await readBody(req)).toString('utf8') || '{}');
  const prompt = promptFrom(body);
  if (!prompt) return send(res, 400, { error: { message: 'a prompt or messages are required' } });
  const maxTokens = Number(body.max_tokens ?? model.maxTokens);
  const id = `chatcmpl-${crypto.randomBytes(12).toString('hex')}`;
  const stream = Boolean(body.stream);
  const abort = new AbortController();
  req.on('close', () => abort.abort());

  let pipeline;
  try {
    pipeline = await bridge.pipelineFor(model);
  } catch (error) {
    return send(res, 503, { error: { message: `p4 session failed: ${error.message}`, type: 'ring_recovering' } });
  }

  if (!stream) {
    try {
      const result = await pipeline.generate({ prompt, maxTokens, options: model.options, abort: abort.signal });
      bridge.recordContribution(model, result.stageRows, result);
      return send(res, 200, {
        id, object: 'chat.completion', created: Math.floor(Date.now() / 1000), model: model.id,
        choices: [{ index: 0, message: { role: 'assistant', content: result.text }, finish_reason: result.finishReason }],
        usage: {
          prompt_tokens: result.promptTokens ?? 0,
          completion_tokens: result.completionTokens,
          total_tokens: (result.promptTokens ?? 0) + result.completionTokens,
        },
        kvasir: { engine: 'p4', request_id: result.requestId, first_token_ms: result.firstTokenMs, stage_rows: result.stageRows },
      });
    } catch (error) {
      return send(res, 502, { error: { message: error.message, type: 'engine_error' } });
    }
  }

  res.writeHead(200, {
    'content-type': 'text/event-stream; charset=utf-8',
    'cache-control': 'no-cache, no-transform',
    connection: 'keep-alive',
  });
  res.write(chunkFrame(id, model.id, { role: 'assistant', content: '' }));
  try {
    const result = await pipeline.generate({
      prompt, maxTokens, options: model.options, abort: abort.signal,
      onToken: ({ text }) => res.write(chunkFrame(id, model.id, { content: text })),
    });
    bridge.recordContribution(model, result.stageRows, result);
    res.write(chunkFrame(id, model.id, {}, result.finishReason));
    // The gateway bills from the last frame that carries usage.
    res.write(`data: ${JSON.stringify({
      id, object: 'chat.completion.chunk', created: Math.floor(Date.now() / 1000), model: model.id, choices: [],
      usage: {
        prompt_tokens: result.promptTokens ?? 0,
        completion_tokens: result.completionTokens,
        total_tokens: (result.promptTokens ?? 0) + result.completionTokens,
      },
    })}\n\n`);
    res.write('data: [DONE]\n\n');
  } catch (error) {
    res.write(`data: ${JSON.stringify({ error: { message: error.message, type: 'engine_error' } })}\n\n`);
  }
  res.end();
}

async function main() {
  const catalogFile = process.env.P4_BRIDGE_CATALOG ?? './catalog.json';
  const port = Number(process.env.P4_BRIDGE_PORT ?? 19100);
  const bridge = new Bridge({
    catalogFile,
    operatorWallet: process.env.P4_BRIDGE_OPERATOR_WALLET ?? '',
  });
  await bridge.refresh();
  setInterval(() => { bridge.refresh().catch(() => {}); }, bridge.inspectIntervalMs).unref?.();
  createServer(bridge).listen(port, () => {
    const serving = bridge.controllers().filter((controller) => controller.serving).map((controller) => controller.id);
    console.log(`p4-bridge listening on :${port} · ingress ${bridge.catalog.ingressAgent} · serving [${serving.join(', ') || 'none'}]`);
  });
}

if (require.main === module) {
  main().catch((error) => { console.error(`p4-bridge failed to start: ${error.message}`); process.exit(1); });
}

module.exports = { Bridge, createServer, promptFrom };

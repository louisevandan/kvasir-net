'use strict';
/**
 * p4 bridge — the HTTP face of the p4 engine.
 *
 * The settlement gateway (solana/staking-service) speaks this bridge's
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
const { NodeAuth } = require('./nodeauth');
const { Participation } = require('./participation');
const wsrelay = require('./wsrelay');

const UNITS_PER_ROW = Number(process.env.P4_BRIDGE_UNITS_PER_KTOKEN ?? 1) / 1000;
// Optional shared secret. p4 itself has no auth, so when the bridge is not on
// loopback this is the only thing between the engine and the network. Unset in
// a private deployment; set it and every path but /health needs the header.
const SERVICE_TOKEN = (process.env.P4_BRIDGE_TOKEN ?? '').trim();
const HEADERS = ['x-kvasir-service-token'];

// How far the bridge is allowed to go in spending an agent's connection slots.
// See admitDial(). The defaults assume the observed steady state, where a
// healthy ring opens no new connections at all: 20 h of normal traffic cost 0.
const DIAL_TIMEOUT_MS = Number(process.env.P4_BRIDGE_DIAL_TIMEOUT_MS ?? 10_000);
const DIAL_HOURLY_CAP = Number(process.env.P4_BRIDGE_DIAL_HOURLY_CAP ?? 6);
const DIAL_LIFETIME_CAP = Number(process.env.P4_BRIDGE_DIAL_LIFETIME_CAP ?? 64);
const DIAL_BACKOFF_MS = [60_000, 120_000, 240_000, 480_000, 900_000, 1_800_000];
const DIAL_STEADY_MS = 300_000;
// How long to leave a failing agent alone before probing it again. An INSPECT
// that our side gave up on is still an unresolved event on the agent's, and a
// stalled agent answers none of them: at the 15 s refresh cadence that is 240
// an hour against a 256-deep receipt store. See refresh().
const INSPECT_BACKOFF_MS = [30_000, 60_000, 120_000, 300_000];

/** The expert-shard reader: see shard-server.py. Loopback by default. */
const SHARD_ORIGIN = (process.env.P4_SHARD_URL ?? 'http://127.0.0.1:42300').replace(/\/+$/, '');
const SHARD_TOKEN = (process.env.P4_SHARD_TOKEN ?? '').trim();

/** Paths the participation module owns, including its own authentication. */
const PARTICIPATION_PATHS =
  /^\/api\/(auth\/(challenge|node-token)|expert-(demand|volunteer|coverage)|proxy\/models\/[^/]+\/expert-shard|expert-relay\/sessions)$/;

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
    this.connecting = new Map();      // agent address -> in-flight dial promise
    this.inspecting = new Map();      // agent address -> in-flight INSPECT promise
    this.probe = new Map();           // agent address -> {failures, nextAt}
    this.guards = new Map();          // agent address -> see guardFor()
    this.lastInspectError = null;
    this.startedAt = Date.now();
  }

  /** The guard state, in the shape an operator reads it: /api/runtime. */
  dialLedger() {
    return [...this.guards.entries()].map(([agent, guard]) => ({
      agent,
      slots_spent: guard.spent,
      unproductive_dials: guard.unproductive,
      dials_last_hour: guard.recent.filter((at) => Date.now() - at < 3_600_000).length,
      next_dial_in_s: guard.nextDialAt > Date.now() ? Math.ceil((guard.nextDialAt - Date.now()) / 1000) : 0,
      failed_probes: this.probe.get(agent)?.failures ?? 0,
      next_probe_in_s: (() => { const p = this.probe.get(agent); return p && p.nextAt > Date.now() ? Math.ceil((p.nextAt - Date.now()) / 1000) : 0; })(),
      connected: Boolean(this.clients.get(agent) && !this.clients.get(agent).closed),
      stopped: guard.stoppedWhy,
    }));
  }

  /**
   * What this bridge has spent of one agent's connection slots, and whether it
   * is still allowed to spend more.
   *
   *   spent        slots taken from this agent since the bridge started. Never
   *                refunded: a successful connect does not give the previous
   *                one back.
   *   unproductive consecutive dials that bought nothing -- the dial failed, or
   *                the connection it opened never served STEADY_MS of INSPECTs.
   *                Drives the backoff, and resets when a connection does earn
   *                its slot.
   *   recent       timestamps of dials inside the last hour.
   */
  guardFor(agent) {
    let guard = this.guards.get(agent);
    if (!guard) {
      guard = { spent: 0, unproductive: 0, recent: [], nextDialAt: 0, healthySince: 0, stoppedWhy: null };
      this.guards.set(agent, guard);
    }
    return guard;
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

    // One dial per agent at a time. The refresh timer fires every 15 s whether
    // or not the last refresh finished (see the bottom of this file), and a
    // client is only recorded once connect() resolves -- so without this, a
    // dial that is merely slow collects a fresh companion every 15 s, and each
    // one the agent accepts is a slot gone for good. pipelineFor() dials too,
    // so the guard lives here rather than in refresh().
    const inflight = this.connecting.get(agent);
    if (inflight) return inflight;

    this.admitDial(agent);
    const promise = this.dial(agent)
      .then((client) => {
        this.clients.set(agent, client);
        // Sessions do not survive a new connection, but only the pipelines that
        // actually run on this agent are affected.
        for (const model of this.catalog.models) {
          if (model.stages.some((stage) => stage.agent === agent)) this.pipelines.delete(model.id);
        }
        const guard = this.guardFor(agent);
        guard.healthySince = 0;
        return client;
      })
      .catch((error) => { this.noteUnproductive(agent); throw error; })
      .finally(() => { this.connecting.delete(agent); });
    this.connecting.set(agent, promise);
    return promise;
  }

  /**
   * Three limits, because a connection slot is a consumable.
   *
   * An agent never reclaims the slot of a connection whose owner went away
   * (transport.rs:900-902, Semaphore(256) at mod.rs:48). Anything that dials on
   * a timer is therefore a leak with a clock on it, and backoff alone only
   * slows the clock: retrying every 30 minutes still costs 48 slots a day.
   *
   *   backoff     spaces out consecutive dials that bought nothing.
   *   hourly cap  bounds a connection that flaps -- opens, serves a while, dies
   *              -- which backoff never sees, because each dial succeeded.
   *   lifetime    stops dialling altogether. An agent that has taken a quarter
   *               of its slots from this bridge and given nothing lasting back
   *               will not be fixed by one more; at that point a person should
   *               look. Nothing clears this but a bridge restart or an operator,
   *               because no field in the INSPECT snapshot identifies an agent
   *               PROCESS generation -- node generations are a different thing
   *               (catalog.js:43) -- so the bridge cannot tell on its own that
   *               the agent restarted and the slots came back.
   */
  admitDial(agent) {
    const guard = this.guardFor(agent);
    const now = Date.now();
    guard.recent = guard.recent.filter((at) => now - at < 3_600_000);

    if (guard.spent >= DIAL_LIFETIME_CAP) {
      guard.stoppedWhy = `spent ${guard.spent} of this agent's connection slots; not dialling again without an operator`;
      throw new Error(`${agent}: ${guard.stoppedWhy}`);
    }
    if (guard.recent.length >= DIAL_HOURLY_CAP) {
      throw new Error(`${agent}: ${guard.recent.length} dials in the last hour, cap is ${DIAL_HOURLY_CAP}`);
    }
    if (now < guard.nextDialAt) {
      throw new Error(`${agent}: ${guard.unproductive} unproductive dials, waiting ${Math.ceil((guard.nextDialAt - now) / 1000)} s`);
    }

    // Spent before the dial, never after. A connect this side gives up on may
    // already have been accepted on the other, and that slot is gone either way.
    guard.spent += 1;
    guard.recent.push(now);
    guard.stoppedWhy = null;
  }

  noteUnproductive(agent) {
    const guard = this.guardFor(agent);
    guard.unproductive += 1;
    guard.healthySince = 0;
    guard.nextDialAt = Date.now() + DIAL_BACKOFF_MS[Math.min(guard.unproductive - 1, DIAL_BACKOFF_MS.length - 1)];
  }

  /**
   * A connection earns its slot by serving INSPECTs for STEADY_MS, not by
   * answering one. A node that recovers for thirty seconds and falls over again
   * would otherwise walk straight back into the fast end of the backoff.
   */
  noteSteady(agent) {
    const guard = this.guardFor(agent);
    if (!guard.healthySince) { guard.healthySince = Date.now(); return; }
    if (guard.unproductive && Date.now() - guard.healthySince >= DIAL_STEADY_MS) {
      guard.unproductive = 0;
      guard.nextDialAt = 0;
    }
  }

  async dial(agent) {
    const [host, port] = agent.replace(/^tcp:\/\//, '').split(':');
    return connect({
      host,
      port: Number(port),
      address: agent,
      channel: `kvr-bridge-${crypto.randomBytes(6).toString('hex')}`,
      deadlineMs: 300_000,
      connectTimeoutMs: DIAL_TIMEOUT_MS,
      // Direct ingress, not the receipt-framed hop transport. The agent writes
      // an admission record only for events that arrive directly
      // (transport.rs:1022); hop-framed events get an inbound record instead
      // (transport.rs:1228). The head binds every submission against its
      // admission, so a hop-framed prefill fails at the head with ENOENT
      // before any work is done. Set P4_BRIDGE_HOP=1 to go back.
      hop: process.env.P4_BRIDGE_HOP === '1',
    });
  }

  /**
   * Drop one agent's connection, and only the pipelines that span it.
   *
   * An agent never reclaims a connection slot whose owner went away
   * (transport.rs:900-902, Semaphore(256) at mod.rs:48), so every reconnect
   * costs that agent a slot for good. Dropping a healthy agent because a
   * different one is unreachable therefore spends the healthy agent's slots at
   * the refresh cadence: 15 s apart, 256 gone in about an hour. That is how
   * GB10 #1's agent died on 2026-09-23 while #2 was the machine that had
   * failed -- #1 kept accepting, so #1 kept paying.
   */
  dropClient(agent) {
    this.clients.get(agent)?.close?.();
    this.clients.delete(agent);
    this.snapshots.delete(agent);
    // Sessions do not survive the connection they were opened on, but only the
    // pipelines that actually touch this agent are affected.
    for (const model of this.catalog.models) {
      if (model.stages.some((stage) => stage.agent === agent)) this.pipelines.delete(model.id);
    }
  }

  /**
   * An INSPECT that times out is not a dead connection.
   *
   * exchange() rejects on a timer of its own and deletes the waiter
   * (wire.js:433-447); the socket is never touched, and a reply that arrives
   * afterwards finds no waiter and falls through to the listeners
   * (wire.js:_deliver). Correlation ids are per-event, so a late reply cannot
   * be mistaken for the next one. Closing such a connection was pure loss: the
   * slot is spent for good and the reconnect spends another, every 15 s, for as
   * long as the agent is merely slow.
   *
   * So the two questions are answered separately. The SNAPSHOT is stale either
   * way -- it is dropped, and verify() reads a missing snapshot as a stage it
   * could not find, so a model that spans this agent comes out serving:false.
   * The CONNECTION is dropped only when it is actually gone.
   */
  async refresh() {
    const failures = [];
    for (const agent of catalogModule.agents(this.catalog)) {
      // One probe in flight per agent. The refresh timer does not wait for the
      // last refresh to finish, so without this an agent that is slow to answer
      // collects a second INSPECT while the first is still out.
      if (this.inspecting.has(agent)) continue;
      // And an agent that has stopped answering is left alone for a while.
      // Keeping the connection through a timeout costs no slot (see below), but
      // the REQUEST is still outstanding on the agent: its receive loop is what
      // returns receipts (transport.rs:1157-1215), so a process that is wedged
      // returns none, and send() does not check outstanding against the agreed
      // maxOutstanding (wire.js:417-422). Probing a wedged agent every 15 s
      // fills its 256-deep store in about an hour; when it wakes, the overflow
      // comes back as `hop receipt store full` and takes the connection with
      // it. Found by GB10 #1 in the agent source, 2026-09-25.
      const probe = this.probe.get(agent);
      if (probe && Date.now() < probe.nextAt) continue;
      try {
        const client = await this.ensureClient(agent);
        // Re-check: ensureClient yields even when the client is already open,
        // so two refreshes both pass the test above before either sets the map.
        // There is no await between here and the set, so this window closes it.
        if (this.inspecting.has(agent)) continue;
        const pending = client.inspect(agent, { timeoutMs: 10_000 });
        this.inspecting.set(agent, pending);
        try {
          this.snapshots.set(agent, await pending);
        } finally {
          this.inspecting.delete(agent);
        }
        this.probe.delete(agent);
        this.noteSteady(agent);
      } catch (error) {
        const seen = this.probe.get(agent) ?? { failures: 0, nextAt: 0 };
        seen.failures += 1;
        seen.nextAt = Date.now() + INSPECT_BACKOFF_MS[Math.min(seen.failures - 1, INSPECT_BACKOFF_MS.length - 1)];
        this.probe.set(agent, seen);
        failures.push(`${agent}: ${error.message}`);
        const guard = this.guardFor(agent);
        guard.healthySince = 0;
        this.snapshots.delete(agent);
        for (const model of this.catalog.models) {
          if (model.stages.some((stage) => stage.agent === agent)) this.pipelines.delete(model.id);
        }
        const client = this.clients.get(agent);
        if (client && !client.closed) continue;   // slow, not dead: keep the slot
        this.dropClient(agent);
      }
    }
    // verify() reads a missing snapshot as a stage it could not find, so a model
    // that spans the failed agent still comes out serving:false -- without
    // taking the models that do not span it out of service too.
    for (const model of this.catalog.models) {
      this.serving.set(model.id, await catalogModule.verify(model, this.snapshots));
    }
    this.lastInspectError = failures.length ? failures.join('; ') : null;
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
   * Credit a phone for the bytes its relay carried.
   *
   * This is a different kind of contribution from a stage's rows and is paid to
   * a different wallet: the ring stages here are ours, but a participating node
   * is someone else's device and earns for the wallet that authenticated it.
   * The gateway reads both out of the same ledger, so the owner has to travel
   * with the entry rather than be assumed.
   */
  creditRelay(nodeId, units, { owner = '', model = '', os = '', deviceKind = '', accelerator = '', backend = '' } = {}) {
    if (!(units > 0)) return;
    // The defaults date from when the only relay participants were phones. They
    // are now what a machine is called when it does not say — and everything
    // current does say, so they should be reached rarely and never silently
    // relabel a server. The values are applied on every credit rather than only
    // at creation, so a node that was first seen before its client reported a
    // platform is corrected on its next contribution instead of staying a phone
    // for the life of the row.
    const current = this.contributions.get(nodeId) ?? {
      units: 0, rows: 0, requests: 0, agent: null, tps: null,
      owner, model, backend: 'relay', os: 'mobile', accelerator: 'gpu', deviceKind: 'phone',
    };
    current.units += units;
    if (owner) current.owner = owner;
    if (os) current.os = os;
    if (deviceKind) current.deviceKind = deviceKind;
    if (accelerator) current.accelerator = accelerator;
    if (backend) current.backend = backend;
    this.contributions.set(nodeId, current);
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

const textOf = (message) => (Array.isArray(message.content)
  ? message.content.map((part) => part.text ?? '').join('')
  : String(message.content ?? ''));

/**
 * The prompt an OpenAI chat body asks for, in the turn format the model was
 * trained on.
 *
 * `raw` is the old behaviour: the messages flattened into one string. It reads
 * as a document to the model, so an instruct model continues it instead of
 * answering — the reply wanders past the answer into invented follow-up turns,
 * and never emits its end-of-turn token, so generation runs to max_tokens.
 *
 * `chatml` renders `<|im_start|>role\n…<|im_end|>`, which is what the
 * end-of-turn token belongs to. A reasoning model additionally opens the
 * assistant turn with a thinking block.
 */
function promptFrom(body, format = 'raw', reasoning = false, thinkingOpen = reasoning) {
  if (typeof body.prompt === 'string') return body.prompt;
  const messages = Array.isArray(body.messages) ? body.messages : [];
  if (format === 'chatml') {
    const turns = messages
      .map((message) => `<|im_start|>${message.role ?? 'user'}\n${textOf(message)}<|im_end|>\n`)
      .join('');
    // A caller can turn thinking off, and the settlement gateway does: a
    // reasoning pass can spend the whole token budget and leave `content`
    // empty, which bills the payer for a blank reply. The template has no
    // switch — it always opens `<think>` — so when thinking is off the block
    // is opened AND closed here, and the model writes its answer after it.
    const think = !reasoning ? '' : thinkingOpen ? '<think>\n' : '<think>\n\n</think>\n\n';
    return `${turns}<|im_start|>assistant\n${think}`;
  }
  return messages
    .map((message) => {
      const content = textOf(message);
      const role = message.role ?? 'user';
      return role === 'user' ? content : `${role}: ${content}`;
    })
    .join('\n\n');
}

/**
 * Split a reasoning model's thinking block off the answer.
 *
 * Only call this when the prompt left `<think>` open: then the model's text
 * starts inside the block and the closing tag is the boundary. When thinking
 * was disabled the prompt already closed the block, so nothing in the output
 * is reasoning — splitting there would file the whole answer as a thought and
 * hand the caller an empty `content`.
 *
 * An unterminated block means the reply was cut off while still thinking:
 * there is no answer to show, so the whole thing is returned as reasoning
 * rather than half a thought presented as a reply.
 */
function splitReasoning(text) {
  const end = text.indexOf('</think>');
  if (end === -1) return { content: '', reasoning_content: text.trim() };
  return {
    content: text.slice(end + '</think>'.length).trim(),
    reasoning_content: text.slice(0, end).trim(),
  };
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
      // The participation surface authenticates itself: /api/auth/* is open by
      // necessity, and everything after it takes a node token rather than the
      // machine-to-machine secret. It therefore has to be offered the request
      // before the blanket gate below, which knows only the secret.
      if (bridge.participation && PARTICIPATION_PATHS.test(path)) {
        let body = {};
        if (req.method === 'POST') {
          try { body = JSON.parse((await readBody(req)).toString('utf8') || '{}'); }
          catch { return send(res, 400, { error: 'request body is not JSON' }); }
        }
        if (await bridge.participation.handle(req, res, path, url.searchParams, body)) return;
      }
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
          dials_stopped: bridge.dialLedger().filter((row) => row.stopped).map((row) => row.agent),
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
          dial_ledger: bridge.dialLedger(),
        });
      }

      if (req.method === 'GET' && path === '/api/contributions') {
        const contributions = [...bridge.contributions.entries()].map(([node, entry]) => ({
          node_id: node,
          // A stage this bridge runs earns for the operator; a phone that
          // carried bytes over a relay earns for the wallet that opened it.
          owner: entry.owner || bridge.operatorWallet,
          units: Number(entry.units.toFixed(6)),
          node_name: node,
          backend: entry.backend ?? 'p4',
          os: entry.os ?? 'linux',
          accelerator: entry.accelerator ?? 'gpu',
          device_kind: entry.deviceKind ?? 'server',
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
  const thinkingOpen = Boolean(model.reasoning) && body.chat_template_kwargs?.enable_thinking !== false;
  const prompt = promptFrom(body, model.promptFormat, model.reasoning, thinkingOpen);
  if (!prompt) return send(res, 400, { error: { message: 'a prompt or messages are required' } });
  // Clamp rather than forward: a request above the loaded resource profile is
  // refused by the adapter outright, and a caller asking for more than the ring
  // was loaded to give should get a shorter answer, not an engine error.
  const maxTokens = Math.min(Number(body.max_tokens ?? model.maxTokens), model.maxTokens);
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
        choices: [{
          index: 0,
          message: thinkingOpen
            ? { role: 'assistant', ...splitReasoning(result.text) }
            : { role: 'assistant', content: result.text },
          finish_reason: result.finishReason,
        }],
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

/**
 * Splice the two relay sockets. A phone cannot be dialled — carrier NAT and a
 * 443-only edge mean neither side can open a socket to the other — so both
 * dial out and meet here.
 */
function attachRelays(server, bridge) {
  server.on('upgrade', (req, socket, head) => {
    const url = new URL(req.url, 'http://bridge.local');
    const path = url.pathname.replace(/\/+$/, '');
    if (!bridge.participation || !['/api/expert-relay', '/api/ring-relay'].includes(path)) {
      socket.destroy();
      return;
    }
    const target = bridge.participation.resolveUpgrade(path, url.searchParams);
    if (!target || target.code) {
      // A close code rather than a reset: the clients log it, and 4401 from
      // 4404 is the difference between "your token expired" and "nothing has
      // claimed that session yet", which are diagnosed very differently.
      return wsrelay.refuse(req, socket, target?.code ?? 4404, target?.reason ?? 'unknown relay');
    }
    wsrelay.bridge(req, socket, head, target, {
      onBytes: (direction, bytes) =>
        bridge.participation.noteRelayBytes(target.session, direction, bytes),
      onClose: () => bridge.participation.noteRelayClosed(target.session),
      log: (line) => console.log(`[relay ${target.session}] ${line}`),
    });
  });
}

async function main() {
  const catalogFile = process.env.P4_BRIDGE_CATALOG ?? './catalog.json';
  const port = Number(process.env.P4_BRIDGE_PORT ?? 19100);
  // Loopback by default. Without P4_BRIDGE_TOKEN this service is unauthenticated
  // and will run inference on the ring for anyone who can reach it, so the
  // public path is a tunnel that terminates here — not an open bind.
  const host = process.env.P4_BRIDGE_HOST ?? '127.0.0.1';
  const bridge = new Bridge({
    catalogFile,
    operatorWallet: process.env.P4_BRIDGE_OPERATOR_WALLET ?? '',
  });
  await bridge.refresh();

  // Participation is optional: without a token secret a node token could not
  // outlive a restart, and a fleet of phones silently dropping off is worse
  // than a surface that is plainly absent. Say which it is at startup.
  const tokenSecret = (process.env.KVR_NODE_TOKEN_SECRET ?? '').trim();
  if (tokenSecret) {
    const minKvr = Number(process.env.KVR_PARTICIPATION_MIN_KVR ?? 0);
    bridge.participation = new Participation({
      auth: new NodeAuth({
        secret: tokenSecret,
        serviceToken: SERVICE_TOKEN,
        // Operating a bridge and contributing compute to one are different
        // things. The retired hub conflated them and refused every phone that
        // ever asked; participation is open here unless an operator sets a
        // floor, and that floor is a separate knob from operator eligibility.
        eligible: minKvr > 0 ? async () => false : null,
      }),
      credit: (nodeId, units, meta) => bridge.creditRelay(nodeId, units, meta),
      // The reader that owns the model file. It runs beside this process —
      // the bridge, the agents and the GGUF are all on the same machine — so
      // this is loopback and the weights never touch a network on the way out
      // of the file. Without both settings shard download is not offered.
      shard: SHARD_ORIGIN && SHARD_TOKEN
        ? { origin: SHARD_ORIGIN, token: SHARD_TOKEN }
        : null,
      // A model is offered to volunteers only once its expert layout has been
      // read off the GGUF. Dimensions alone are not enough — they were what let
      // the market offer a dense layer as an expert window.
      models: () => bridge.catalog.models
        .filter((m) => m.nEmbd && m.nLayer && m.nExpert && m.expertLayers?.length)
        .map((m) => ({
          id: m.id, name: m.name, nEmbd: m.nEmbd, nLayer: m.nLayer, nExpert: m.nExpert,
          expertLayers: m.expertLayers, bytesPerExpert: m.bytesPerExpert,
        })),
    });
  }

  setInterval(() => { bridge.refresh().catch(() => {}); }, bridge.inspectIntervalMs).unref?.();
  const server = createServer(bridge);
  attachRelays(server, bridge);
  server.listen(port, host, () => {
    const serving = bridge.controllers().filter((controller) => controller.serving).map((controller) => controller.id);
    const auth = SERVICE_TOKEN ? 'token required' : 'NO TOKEN — anyone who can reach this can use the ring';
    const market = bridge.participation
      ? `participation open for [${bridge.catalog.models.filter((m) => m.nEmbd).map((m) => m.id).join(', ') || 'no model with recorded dimensions'}]`
      : 'participation off (set KVR_NODE_TOKEN_SECRET to serve it)';
    console.log(`p4-bridge listening on ${host}:${port} · ${auth} · ingress ${bridge.catalog.ingressAgent} · serving [${serving.join(', ') || 'none'}] · ${market}`);
  });
}

if (require.main === module) {
  main().catch((error) => { console.error(`p4-bridge failed to start: ${error.message}`); process.exit(1); });
}

module.exports = { Bridge, createServer, attachRelays, promptFrom };

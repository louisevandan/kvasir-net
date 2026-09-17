'use strict';
/**
 * A loaded p4 pipeline, driven from OUTER.
 *
 * p4 splits the work that a hub used to hide: OUTER owns placement, session
 * identity and every number a biller needs. This module is that OUTER for one
 * pipeline — install a session across its stages, submit a request to stage 0,
 * gather the token stream, and derive usage from the observations the stages
 * emit. Nothing here decides *which* nodes make a pipeline; that comes from the
 * catalog (see catalog.js), because the engine carries no model names.
 *
 * Content types are the llama.cpp staged adapter's v2 vocabulary
 * (`layers/adapters/llamacpp/staged/adapter/src/v2/mod.rs`).
 */
const crypto = require('node:crypto');
const { nodeEndpoint } = require('./wire');

const ADAPTER = 'llamacpp';
const SESSION = 'application/vnd.p4.llamacpp.session-v4+json';
const SESSION_READY = 'application/vnd.p4.llamacpp.session-ready-v4+json';
const PREFILL = 'application/vnd.p4.llamacpp.prefill-v3+json';
const OUTPUT_V5 = 'application/vnd.p4.llamacpp.output-v5+json';
const OUTPUT_V4 = 'application/vnd.p4.llamacpp.output-v4+json';
const RELEASE_RECEIPT = 'application/vnd.p4.llamacpp.release-receipt-v1+json';
const BATCH_OBSERVATION = 'application/vnd.p4.llamacpp.batch-observation-v4+json';
const STAGE_SPAN = 'application/vnd.p4.llamacpp.stage-span-v4+json';
const ERROR_V2 = 'application/vnd.p4.llamacpp.error-v2+json';

const json = (value) => Buffer.from(JSON.stringify(value), 'utf8');

class Pipeline {
  /**
   * A pipeline can span several agents, and an agent only answers for its own
   * nodes: an event for a stage must go out on the connection to *that* stage's
   * agent. So the caller passes a resolver, not one client.
   *
   * Replies come back on the connection the request left by, because the
   * envelope's return route names that connection's ingress agent and channel.
   * Only the tail stage is different: it answers the prefill's return route, so
   * output lands on the head stage's connection.
   *
   * @param {object} options
   * @param {(agent:string)=>import('./wire').OuterClient} options.clientFor connection for an agent
   * @param {Array<{agent:string,node:string,generation:number}>} options.stages ordered stages
   * @param {number} options.loadGeneration generation the stages were loaded with
   * @param {string} [options.sessionId] reuse an installed session
   */
  constructor({ clientFor, client, stages, loadGeneration, sessionId }) {
    if (!Array.isArray(stages) || stages.length < 1) {
      throw new Error('a p4 pipeline needs at least one stage');
    }
    this.clientFor = clientFor ?? (() => client);
    this.stages = stages;
    this.loadGeneration = loadGeneration;
    this.sessionId = sessionId ?? `kvr-${crypto.randomBytes(6).toString('hex')}`;
    this.installed = false;
  }

  /** Every distinct connection this pipeline speaks over. */
  clients() {
    const seen = new Map();
    for (const stage of this.stages) {
      const client = this.clientFor(stage.agent);
      if (!seen.has(client)) seen.set(client, stage.agent);
    }
    return [...seen.keys()];
  }

  /** Subscribe to one correlation on every connection; unsubscribe all at once. */
  subscribeAll(correlationId, handler) {
    const offs = this.clients().map((client) => client.subscribe(correlationId, handler));
    return () => { for (const off of offs) off(); };
  }

  /** Install the session on every stage and wait for each to report ready. */
  async install({ timeoutMs = 60_000 } = {}) {
    if (this.installed) return this.sessionId;
    const correlationId = `session-${this.sessionId}`;
    const ready = new Set();
    const done = new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        unsubscribe();
        reject(new Error(`session install timed out with ${ready.size}/${this.stages.length} stages ready`));
      }, timeoutMs);
      const unsubscribe = this.subscribeAll(correlationId, (event) => {
        if (event.error) { clearTimeout(timer); unsubscribe(); reject(event.error); return; }
        const type = event.meta.contentType;
        if (type === ERROR_V2) {
          clearTimeout(timer); unsubscribe();
          reject(new Error(`session refused: ${event.payload.toString('utf8').slice(0, 400)}`));
          return;
        }
        if (type !== SESSION_READY) return;
        ready.add(event.meta.source.node ?? ready.size);
        if (ready.size >= this.stages.length) {
          clearTimeout(timer); unsubscribe(); resolve();
        }
      });
    });
    for (let index = 0; index < this.stages.length; index += 1) {
      const stage = this.stages[index];
      this.clientFor(stage.agent).send(
        nodeEndpoint(stage.agent, stage.node, stage.generation),
        SESSION,
        json({
          load_generation: this.loadGeneration,
          session_id: this.sessionId,
          stages: this.stages.map((s) => ({ agent: s.agent, node: s.node, generation: s.generation })),
          stage_index: index,
        }),
        { adapterKind: ADAPTER, eventClass: 'control', correlationId },
      );
    }
    await done;
    this.installed = true;
    return this.sessionId;
  }

  /**
   * Run one request. Tokens are delivered to `onToken` as they arrive; the
   * promise resolves with the text and the usage the stages reported.
   *
   * p4 has no cancel: `abort` stops us reading, it does not stop the work.
   */
  async generate({ prompt, tokens, maxTokens = 256, options = '', sessionKey = null, requestId, onToken, timeoutMs = 300_000, abort }) {
    if (!this.installed) await this.install();
    const id = requestId ?? `req-${crypto.randomBytes(6).toString('hex')}`;
    const started = Date.now();
    const result = {
      requestId: id,
      text: '',
      tokenCount: 0,
      promptTokens: 0,
      finishReason: null,
      firstTokenMs: null,
      stages: new Map(),          // node -> rows contributed
      observations: new Set(),    // observation ids already counted
    };

    const finished = new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        unsubscribe();
        reject(new Error(`p4 request ${id} timed out after ${timeoutMs} ms`));
      }, timeoutMs);
      const finish = (error) => {
        clearTimeout(timer);
        unsubscribe();
        if (abortHandler) abort?.removeEventListener?.('abort', abortHandler);
        if (error) reject(error); else resolve();
      };
      const unsubscribe = this.subscribeAll(id, (event) => {
        if (event.error) { finish(event.error); return; }
        const type = event.meta.contentType;
        if (type === OUTPUT_V5 || type === OUTPUT_V4) {
          const body = JSON.parse(event.payload.toString('utf8'));
          const outcome = body.outcome ?? body;                 // v5 wraps the v4 outcome
          if (result.firstTokenMs === null) result.firstTokenMs = Date.now() - started;
          if (typeof outcome.text === 'string' && outcome.text.length) {
            result.text += outcome.text;
            result.tokenCount += 1;
            onToken?.({ text: outcome.text, position: outcome.position, token: outcome.token });
          }
          if (outcome.stop) {
            result.finishReason = outcome.stop;
            // The terminal outcome closes the request; the release receipt and
            // late observations may still arrive, so keep reading briefly.
            setTimeout(() => finish(), 250);
          }
          return;
        }
        if (type === BATCH_OBSERVATION) {
          const body = JSON.parse(event.payload.toString('utf8'));
          if (body.observation_id && result.observations.has(body.observation_id)) return;
          if (body.observation_id) result.observations.add(body.observation_id);
          for (const batch of body.physical_batches ?? []) {
            for (const owned of batch.owned_requests ?? []) {
              if (owned.request_id === id) result.promptTokens += owned.prefill_rows ?? 0;
            }
          }
          return;
        }
        if (type === STAGE_SPAN) {
          const body = JSON.parse(event.payload.toString('utf8'));
          const node = event.meta.source.node ?? 'unknown';
          result.stages.set(node, (result.stages.get(node) ?? 0) + (body.rows ?? 0));
          return;
        }
        if (type === RELEASE_RECEIPT) { finish(); return; }
        if (type === ERROR_V2) {
          finish(new Error(`p4 refused request ${id}: ${event.payload.toString('utf8').slice(0, 400)}`));
        }
      });
      var abortHandler = null;
      if (abort) {
        abortHandler = () => finish(new Error('client aborted the request'));
        abort.addEventListener?.('abort', abortHandler, { once: true });
      }
    });

    const head = this.stages[0];
    this.clientFor(head.agent).send(
      nodeEndpoint(head.agent, head.node, head.generation),
      PREFILL,
      json({
        load_generation: this.loadGeneration,
        session_id: this.sessionId,
        request_id: id,
        tokens: tokens ?? [],
        prompt: tokens ? null : prompt,
        options,
        session_key: sessionKey,
        max_tokens: maxTokens,
      }),
      { adapterKind: ADAPTER, eventClass: 'data', correlationId: id, deadlineMs: timeoutMs },
    );

    await finished;
    return {
      requestId: id,
      text: result.text,
      finishReason: result.finishReason ?? 'stop',
      completionTokens: result.tokenCount,
      // Prompt tokens are only knowable from the prefill rows the stages
      // report. When no observation arrived we say so instead of guessing.
      promptTokens: result.promptTokens || null,
      firstTokenMs: result.firstTokenMs,
      elapsedMs: Date.now() - started,
      stageRows: Object.fromEntries(result.stages),
    };
  }
}

module.exports = {
  Pipeline,
  contentTypes: {
    SESSION, SESSION_READY, PREFILL, OUTPUT_V4, OUTPUT_V5,
    RELEASE_RECEIPT, BATCH_OBSERVATION, STAGE_SPAN, ERROR_V2,
  },
};

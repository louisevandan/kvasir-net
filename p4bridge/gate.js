'use strict';
/**
 * How many requests a model runs at once, and who waits.
 *
 * The stage servers hold one KV cache for the whole ring: `--ctx-size 32768
 * --n-seq-max 8 --kv-unified` is 32768 tokens shared by up to eight sequences,
 * not 32768 each. A request has all of it only when it runs alone; eight at
 * once are guaranteed about 4096 each, and a long agent prompt then fails deep
 * in the engine instead of at the door. So the bridge admits at most
 * `max_concurrent` requests per model, streaming or not, and the rest wait in
 * one FIFO queue — bounded, and for a bounded time, because a client stuck
 * behind a wedged ring is better told "busy, retry" than left hanging.
 *
 * A slot belongs to a request until the ENGINE is done with it: answered,
 * failed, or past the request timeout. A client leaving is not one of those —
 * p4 has no cancel, so its sequence keeps generating and keeps its share of
 * the KV pool; the bridge reads the reply to its end and drops it. A client
 * that leaves while still queued is simply taken out of the queue. release()
 * counts only once, so the timeout and a late engine answer cannot hand the
 * same slot back twice.
 */

class Busy extends Error {
  constructor(message, retryAfterS) {
    super(message);
    this.code = 'ring_busy';
    this.retryAfterS = retryAfterS;
  }
}

class Aborted extends Error {
  constructor() {
    super('client went away while waiting');
    this.code = 'aborted';
  }
}

class Gate {
  /**
   * @param {object} options
   * @param {number|null} options.limit requests in flight at once; null for no limit
   * @param {number} [options.maxQueue] requests allowed to wait
   * @param {number} [options.timeoutMs] how long one may wait
   */
  constructor({ limit = null, maxQueue = 16, timeoutMs = 120_000 } = {}) {
    this.limit = limit;
    this.maxQueue = maxQueue;
    this.timeoutMs = timeoutMs;
    this.inFlight = 0;
    this.queue = [];
  }

  stats() {
    return { max_concurrent: this.limit, in_flight: this.inFlight, queued: this.queue.length, max_queue: this.maxQueue };
  }

  /** Resolves with a release function once a slot is free. */
  acquire(signal) {
    if (signal?.aborted) return Promise.reject(new Aborted());
    if (this.limit === null || this.inFlight < this.limit) {
      this.inFlight += 1;
      return Promise.resolve(this.releaser());
    }
    if (this.queue.length >= this.maxQueue) {
      return Promise.reject(new Busy(`${this.queue.length} requests are already waiting for this model`, 5));
    }
    return new Promise((resolve, reject) => {
      const entry = { resolve, reject, signal };
      const leave = () => {
        const at = this.queue.indexOf(entry);
        if (at !== -1) this.queue.splice(at, 1);
        clearTimeout(entry.timer);
        signal?.removeEventListener?.('abort', entry.onAbort);
      };
      entry.leave = leave;
      entry.timer = setTimeout(() => {
        leave();
        reject(new Busy(`no slot freed within ${Math.round(this.timeoutMs / 1000)} s`, 10));
      }, this.timeoutMs);
      entry.onAbort = () => { leave(); reject(new Aborted()); };
      signal?.addEventListener?.('abort', entry.onAbort, { once: true });
      this.queue.push(entry);
    });
  }

  releaser() {
    let released = false;
    return () => {
      if (released) return false;
      released = true;
      this.inFlight -= 1;
      this.next();
      return true;
    };
  }

  next() {
    while (this.queue.length && (this.limit === null || this.inFlight < this.limit)) {
      const entry = this.queue[0];
      entry.leave();
      this.inFlight += 1;
      entry.resolve(this.releaser());
    }
  }
}

/** The gate for a model, made on first use from its catalog entry. */
function gateFor(bridge, model) {
  if (!bridge.gates) bridge.gates = new Map();
  let gate = bridge.gates.get(model.id);
  if (!gate) {
    gate = new Gate({ limit: model.maxConcurrent ?? null, maxQueue: model.maxQueue ?? 16, timeoutMs: model.queueTimeoutMs ?? 120_000 });
    bridge.gates.set(model.id, gate);
  }
  return gate;
}

/** Every model's queue, for /health and /api/runtime. */
function gateStats(bridge) {
  return Object.fromEntries((bridge.catalog?.models ?? []).map((model) => [model.id, gateFor(bridge, model).stats()]));
}

/**
 * Requests per model the ring can hold at full context. Stated in the catalog
 * as `max_concurrent`, or derived from `ring_context_size / context_size`.
 * With neither there is no limit, as before.
 */
function maxConcurrentFor(entry) {
  if (entry.max_concurrent !== undefined && entry.max_concurrent !== null) {
    const n = Number(entry.max_concurrent);
    if (!Number.isInteger(n) || n < 1) throw new Error(`catalog model ${entry.id} has max_concurrent ${entry.max_concurrent}; it must be a positive integer`);
    return n;
  }
  if (entry.ring_context_size && entry.context_size) {
    return Math.max(1, Math.floor(Number(entry.ring_context_size) / Number(entry.context_size)));
  }
  return null;
}

/**
 * Tokens a prompt may take, without a tokenizer, erring high. ASCII runs about
 * four characters a token and is counted at three. Anything else — Korean,
 * Chinese, emoji — is counted a token per character, which is roughly what a
 * BPE vocabulary gives Hangul and never much less; at three characters a token
 * a Korean prompt would be counted at a third of its size and let through to
 * fail in the engine.
 */
function estimateTokens(prompt) {
  let ascii = 0;
  let other = 0;
  for (const ch of String(prompt)) {
    if (ch.codePointAt(0) < 0x80) ascii += 1; else other += 1;
  }
  return Math.ceil(ascii / 3) + other;
}

module.exports = { Gate, Busy, Aborted, gateFor, gateStats, maxConcurrentFor, estimateTokens };

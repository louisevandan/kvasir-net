'use strict';
/**
 * Per-request numbers, to decide whether prefix-KV reuse is worth building.
 *
 * Every chat completion logs one line of numbers: tokens, queue wait, time to
 * first token (the prefill), decode time and rates, and how much of its
 * prompt it shares with a recent one. The last is the question: an agent
 * resends its whole conversation every turn, and if most of each prompt was
 * already prefilled a moment ago, reusing that KV would save most of the
 * prefill — which at ~70 tok/s is most of the wait.
 *
 * Nothing of the prompt is kept or written. The bridge has no tokenizer, so a
 * prompt is reduced to a chain of 8-byte hashes, one per 64 characters of the
 * rendered prompt, each covering everything before it. Two prompts share the
 * first k chunks exactly when their k-th hashes are equal, so the common
 * prefix is a binary search, and an entry costs 8 bytes per 64 characters:
 * a 40K-character prompt is 5 KB, and the cache holds at most 64 of them for
 * at most ten minutes. Characters are turned into tokens with the engine's own
 * count for that prompt when it reported one.
 *
 * P4_BRIDGE_MEASURE=0 turns it off; it is on otherwise.
 */
const crypto = require('node:crypto');

const CHUNK_CHARS = 64;
const HASH_BYTES = 8;
/** Characters per token when the engine did not say: gate.js's high estimate. */
const FALLBACK_CHARS_PER_TOKEN = 3;

const enabled = () => !/^(0|false|no|off)$/i.test(String(process.env.P4_BRIDGE_MEASURE ?? '1'));

/** The chained chunk hashes of a string: entry k covers chars [0, 64(k+1)). */
function chunkChain(text) {
  const chunks = Math.floor(text.length / CHUNK_CHARS);
  const chain = Buffer.alloc(chunks * HASH_BYTES);
  let previous = Buffer.alloc(0);
  for (let k = 0; k < chunks; k += 1) {
    const digest = crypto.createHash('sha256')
      .update(previous)
      .update(text.slice(k * CHUNK_CHARS, (k + 1) * CHUNK_CHARS))
      .digest()
      .subarray(0, HASH_BYTES);
    digest.copy(chain, k * HASH_BYTES);
    previous = digest;
  }
  return chain;
}

const wholeHash = (text) => crypto.createHash('sha256').update(text).digest().subarray(0, 16);

/** Fingerprint of a prompt: nothing in it can be turned back into text. */
function fingerprint(prompt, systemChars = 0) {
  const text = String(prompt ?? '');
  const system = Math.max(0, Math.min(systemChars, text.length));
  return {
    length: text.length,
    chain: chunkChain(text),
    whole: wholeHash(text),
    system,
    systemHash: system > 0 ? wholeHash(text.slice(0, system)) : null,
  };
}

/** Characters two fingerprints share from the start, to 64-character grain. */
function commonPrefixChars(a, b) {
  if (a.length === b.length && a.whole.equals(b.whole)) return a.length;
  const n = Math.min(a.chain.length, b.chain.length) / HASH_BYTES;
  const same = (k) => a.chain.compare(b.chain, k * HASH_BYTES, (k + 1) * HASH_BYTES,
    k * HASH_BYTES, (k + 1) * HASH_BYTES) === 0;
  // Chained, so "chunk k matches" holds for every k up to the first mismatch.
  let lo = 0;
  let hi = n;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (same(mid)) lo = mid + 1; else hi = mid;
  }
  return lo * CHUNK_CHARS;
}

/**
 * How many characters of the rendered prompt the system turn and tools take.
 * `render(bodyWithOnlySystem)` renders the same template with every other
 * message removed; the two agree up to where the system block ends.
 */
function systemBlockChars(prompt, systemOnlyRender) {
  if (!systemOnlyRender) return 0;
  const n = Math.min(prompt.length, systemOnlyRender.length);
  let i = 0;
  while (i < n && prompt.charCodeAt(i) === systemOnlyRender.charCodeAt(i)) i += 1;
  return i;
}

/** A bounded, expiring set of recent prompt fingerprints. */
class PrefixCache {
  constructor({ capacity = 64, ttlMs = 10 * 60_000, now = () => Date.now() } = {}) {
    this.capacity = capacity;
    this.ttlMs = ttlMs;
    this.now = now;
    this.entries = new Map();     // seq -> {fp, at}; insertion order is recency
    this.seq = 0;
  }

  prune() {
    const cutoff = this.now() - this.ttlMs;
    for (const [key, entry] of this.entries) {
      if (entry.at < cutoff) this.entries.delete(key);
    }
  }

  /** The best match among cached prompts, without adding this one. */
  match(fp) {
    this.prune();
    let best = 0;
    let bestKey = null;
    let systemMatched = false;
    for (const [key, entry] of this.entries) {
      const chars = commonPrefixChars(fp, entry.fp);
      if (chars > best) { best = chars; bestKey = key; }
      if (fp.systemHash && entry.fp.system === fp.system && entry.fp.systemHash.equals(fp.systemHash)) {
        systemMatched = true;
      }
    }
    if (bestKey !== null) {
      // A prompt that keeps being extended is the one worth keeping.
      const entry = this.entries.get(bestKey);
      this.entries.delete(bestKey);
      this.entries.set(bestKey, entry);
    }
    return { prefixChars: best, systemMatched, cached: this.entries.size };
  }

  add(fp) {
    this.prune();
    this.seq += 1;
    this.entries.set(this.seq, { fp, at: this.now() });
    while (this.entries.size > this.capacity) {
      this.entries.delete(this.entries.keys().next().value);
    }
  }
}

function percentile(values, p) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)];
}

const round = (value, digits = 1) => (value === null || value === undefined || !Number.isFinite(value)
  ? null : Math.round(value * 10 ** digits) / 10 ** digits);

/** The per-bridge meter: the prefix cache, the recent window, the log line. */
class Measure {
  constructor({ capacity = 64, ttlMs = 10 * 60_000, window = 256, log = (line) => console.log(line), now } = {}) {
    this.cache = new PrefixCache({ capacity, ttlMs, now });
    this.window = window;
    this.recent = [];
    this.count = 0;
    this.log = log;
  }

  /**
   * Fold one finished request in and log its line.
   *
   * @param {object} m
   * @param {string} m.id @param {string} m.model @param {boolean} m.stream
   * @param {string} m.outcome ok, ring_busy, client_left, engine_error, ...
   * @param {string} [m.prompt] the rendered prompt; hashed here, never kept
   * @param {number} [m.systemChars]
   * @param {object} [m.result] what the engine returned
   * @param {number} [m.queueMs] @param {number} [m.totalMs] @param {number} [m.lastTokenMs]
   */
  record(m) {
    this.count += 1;
    const result = m.result ?? null;
    const promptChars = typeof m.prompt === 'string' ? m.prompt.length : 0;
    let prefix = { prefixChars: 0, systemMatched: false, cached: this.cache.entries.size };
    let systemChars = 0;
    if (promptChars) {
      const fp = fingerprint(m.prompt, m.systemChars ?? 0);
      systemChars = fp.system;
      prefix = this.cache.match(fp);
      // Only a prompt the engine prefilled could have left KV behind to reuse.
      if (result) this.cache.add(fp);
    }
    const engineTokens = result?.promptTokens ?? null;
    const promptTokens = engineTokens ?? (promptChars ? Math.ceil(promptChars / FALLBACK_CHARS_PER_TOKEN) : null);
    const charsPerToken = engineTokens && promptChars ? promptChars / engineTokens : FALLBACK_CHARS_PER_TOKEN;
    const completionTokens = result?.completionTokens ?? null;
    const ttftMs = result?.firstTokenMs ?? null;
    const endMs = m.lastTokenMs ?? result?.elapsedMs ?? null;
    const decodeMs = ttftMs !== null && endMs !== null ? Math.max(0, endMs - ttftMs) : null;
    const prefillTps = ttftMs && promptTokens ? promptTokens / (ttftMs / 1000) : null;
    const decodeTps = decodeMs && completionTokens > 1 ? (completionTokens - 1) / (decodeMs / 1000) : null;
    const prefixRatio = promptChars ? prefix.prefixChars / promptChars : null;

    const fields = {
      id: m.id ?? '-',
      model: m.model,
      stream: m.stream ? 1 : 0,
      outcome: m.outcome,
      prompt_chars: promptChars,
      prompt_tokens: promptTokens,
      prompt_tokens_src: engineTokens ? 'engine' : 'est',
      completion_tokens: completionTokens,
      queue_ms: round(m.queueMs, 0),
      ttft_ms: ttftMs,
      decode_ms: round(decodeMs, 0),
      total_ms: round(m.totalMs, 0),
      prefill_tps: round(prefillTps),
      decode_tps: round(decodeTps),
      prefix_chars: prefix.prefixChars,
      prefix_tokens: promptChars ? Math.round(prefix.prefixChars / charsPerToken) : null,
      prefix_ratio: round(prefixRatio, 3),
      system_chars: systemChars,
      system_match: prefix.systemMatched ? 1 : 0,
      cached: prefix.cached,
    };
    // Numbers and fixed words only. The id is ours (random), the model id
    // comes from the catalog; nothing a caller wrote reaches this line.
    this.log(`[measure] ${Object.entries(fields).map(([k, v]) => `${k}=${v ?? '-'}`).join(' ')}`);

    if (m.outcome === 'ok') {
      this.recent.push({ ttftMs, prefillTps, prefixRatio });
      if (this.recent.length > this.window) this.recent.shift();
    }
    return fields;
  }

  /** For /health: the last `window` successful requests. */
  stats() {
    const ttft = this.recent.map((r) => r.ttftMs).filter(Number.isFinite);
    const tps = this.recent.map((r) => r.prefillTps).filter(Number.isFinite);
    const ratios = this.recent.map((r) => r.prefixRatio).filter(Number.isFinite);
    return {
      count: this.count,
      window: this.recent.length,
      ttft_ms: { p50: percentile(ttft, 50), p90: percentile(ttft, 90) },
      prefill_tps: { p50: round(percentile(tps, 50)), p90: round(percentile(tps, 90)) },
      prefix_ratio_mean: ratios.length ? round(ratios.reduce((a, b) => a + b, 0) / ratios.length, 3) : null,
      cached_prompts: this.cache.entries.size,
    };
  }
}

/** The bridge's meter, made on first use; null while measuring is off. */
function measureFor(bridge) {
  if (!enabled()) return null;
  if (!bridge.measure) bridge.measure = new Measure(bridge.measureOptions ?? {});
  return bridge.measure;
}

module.exports = {
  Measure, PrefixCache, fingerprint, commonPrefixChars, systemBlockChars, chunkChain,
  measureFor, enabled, CHUNK_CHARS,
};

'use strict';
/**
 * The measurement line: is prefix-KV reuse worth building?
 *
 * What matters most here is what is NOT in the output: no prompt text, ever.
 */
const test = require('node:test');
const assert = require('node:assert/strict');

const {
  Measure, PrefixCache, fingerprint, commonPrefixChars, systemBlockChars, CHUNK_CHARS,
} = require('../measure.js');
const { createServer } = require('../server.js');

const text = (seed, length) => {
  let out = '';
  let i = 0;
  while (out.length < length) out += `${seed}-${i++} `;
  return out.slice(0, length);
};

test('prefix: identical prompts share all of it', () => {
  const p = text('a', 1000);
  assert.equal(commonPrefixChars(fingerprint(p), fingerprint(p)), 1000);
});

test('prefix: a partial match counts whole 64-character chunks up to the divergence', () => {
  const shared = text('s', 700);
  const a = fingerprint(shared + text('x', 500));
  const b = fingerprint(shared + text('y', 500));
  assert.equal(commonPrefixChars(a, b), Math.floor(700 / CHUNK_CHARS) * CHUNK_CHARS);
  // An agent's next turn: the old prompt plus more.
  const turn1 = text('t', 1300);
  const turn2 = turn1 + text('more', 400);
  assert.equal(commonPrefixChars(fingerprint(turn1), fingerprint(turn2)), 1280);
});

test('prefix: disjoint prompts share nothing, and short ones are not over-counted', () => {
  assert.equal(commonPrefixChars(fingerprint(text('p', 900)), fingerprint(text('q', 900))), 0);
  assert.equal(commonPrefixChars(fingerprint('short'), fingerprint('shorter')), 0);
  // A difference in the very last character of a chunk is still a difference.
  const a = 'z'.repeat(128);
  const b = `${'z'.repeat(127)}!`;
  assert.equal(commonPrefixChars(fingerprint(a), fingerprint(b)), 64);
});

test('prefix: the system block is located and matched on its own', () => {
  const system = '<|im_start|>system\nYou are terse.<|im_end|>\n';
  const prompt = `${system}<|im_start|>user\nhi<|im_end|>\n<|im_start|>assistant\n`;
  const alone = `${system}<|im_start|>assistant\n`;
  const chars = systemBlockChars(prompt, alone);
  assert.ok(chars >= system.length && chars < prompt.length);
  assert.equal(systemBlockChars(prompt, ''), 0);

  const cache = new PrefixCache();
  cache.add(fingerprint(prompt, chars));
  const other = `${system}<|im_start|>user\nsomething else entirely<|im_end|>\n<|im_start|>assistant\n`;
  assert.equal(cache.match(fingerprint(other, systemBlockChars(other, alone))).systemMatched, true);
  const changed = other.replace('terse', 'wordy');
  assert.equal(cache.match(fingerprint(changed, systemBlockChars(changed, alone.replace('terse', 'wordy')))).systemMatched, false);
});

test('cache: bounded, least recently matched goes first', () => {
  const cache = new PrefixCache({ capacity: 3 });
  const prompts = [0, 1, 2, 3, 4].map((i) => text(`p${i}`, 640));
  for (const p of prompts) cache.add(fingerprint(p));
  assert.equal(cache.entries.size, 3);
  assert.equal(cache.match(fingerprint(prompts[0])).prefixChars, 0, 'evicted');
  assert.equal(cache.match(fingerprint(prompts[4])).prefixChars, 640);
  // Matching p2 makes it recent, so the next add evicts p3.
  cache.match(fingerprint(prompts[2]));
  cache.add(fingerprint(text('p5', 640)));
  assert.equal(cache.match(fingerprint(prompts[3])).prefixChars, 0);
  assert.equal(cache.match(fingerprint(prompts[2])).prefixChars, 640);
});

test('cache: entries expire after the TTL', () => {
  let now = 1_000_000;
  const cache = new PrefixCache({ ttlMs: 600_000, now: () => now });
  const p = text('ttl', 640);
  cache.add(fingerprint(p));
  now += 599_000;
  assert.equal(cache.match(fingerprint(p)).prefixChars, 640);
  now += 2_000;
  assert.equal(cache.match(fingerprint(p)).prefixChars, 0);
  assert.equal(cache.entries.size, 0);
});

test('cache: a fingerprint is 8 bytes per 64 characters, and holds no text', () => {
  const p = `CANARY-${text('c', 6400)}`;
  const fp = fingerprint(p);
  assert.equal(fp.chain.length, (6400 + 7) >> 6 << 3);
  const dump = JSON.stringify(fp) + fp.chain.toString('latin1') + fp.whole.toString('latin1');
  assert.ok(!dump.includes('CANARY'));
});

test('aggregates: p50/p90 of TTFT and prefill rate, mean prefix ratio', () => {
  const lines = [];
  const m = new Measure({ log: (line) => lines.push(line) });
  const shared = text('sys', 640);
  for (let i = 1; i <= 10; i += 1) {
    m.record({
      id: `r${i}`, model: 'm', stream: true, outcome: 'ok',
      prompt: shared + text(`u${i}`, 640), queueMs: 0, totalMs: 1000 * i,
      result: { promptTokens: 400, completionTokens: 11, firstTokenMs: 1000 * i, elapsedMs: 1000 * i + 1000 },
    });
  }
  m.record({ id: 'busy', model: 'm', stream: false, outcome: 'ring_busy', prompt: shared, queueMs: 75_000 });
  const s = m.stats();
  assert.equal(s.count, 11);
  assert.equal(s.window, 10, 'only successful requests enter the window');
  assert.deepEqual(s.ttft_ms, { p50: 5000, p90: 9000 });
  assert.deepEqual(s.prefill_tps, { p50: 66.7, p90: 200 });
  // The first prompt matched nothing, the other nine matched half.
  assert.equal(s.prefix_ratio_mean, 0.45);
  assert.equal(lines.length, 11);
  assert.match(lines[1], /prefix_chars=640 prefix_tokens=200 prefix_ratio=0.5 /);
  assert.match(lines[1], /prefill_tps=200 decode_tps=10 /);
});

/** Turn `n` of a conversation: its system prompt, then every earlier turn. */
function conversation(name, turns) {
  let prompt = text(`${name}-system`, 1280);
  const out = [];
  for (let i = 0; i < turns; i += 1) {
    prompt += text(`${name}-turn${i}`, 640);
    out.push(prompt);
  }
  return out;
}

const okResult = (prompt) => ({ promptTokens: Math.ceil(prompt.length / 4), completionTokens: 2, firstTokenMs: 100, elapsedMs: 200 });
const field = (line, key) => Number(new RegExp(` ${key}=([\\d.]+)`).exec(line)[1]);

test('reuse: three interleaved conversations defeat two slots, not the upper bound', () => {
  const lines = [];
  const m = new Measure({ log: (line) => lines.push(line) });
  const convs = ['a', 'b', 'c'].map((name) => conversation(name, 6));
  for (let turn = 0; turn < 6; turn += 1) {
    for (const conv of convs) {
      m.record({ id: 'x', model: 'm', stream: true, outcome: 'ok', prompt: conv[turn], result: okResult(conv[turn]) });
    }
  }
  // From the second round on, each request's own previous turn is three back.
  for (const line of lines.slice(3)) {
    assert.ok(field(line, 'prefix_ratio') > 0.6, line);
    assert.equal(field(line, 'prefix_slots2_ratio'), 0, line);
    assert.equal(field(line, 'prefix_prev_ratio'), 0, line);
  }
  const s = m.stats();
  assert.ok(s.prefix_ratio_mean > 0.5);
  assert.equal(s.prefix_slots2_ratio_mean, 0);
  assert.equal(s.prefix_prev_ratio_mean, 0);
});

test('reuse: two interleaved conversations fit two slots but not one', () => {
  const lines = [];
  const m = new Measure({ log: (line) => lines.push(line) });
  const convs = ['a', 'b'].map((name) => conversation(name, 4));
  for (let turn = 0; turn < 4; turn += 1) {
    for (const conv of convs) {
      m.record({ id: 'x', model: 'm', stream: true, outcome: 'ok', prompt: conv[turn], result: okResult(conv[turn]) });
    }
  }
  for (const line of lines.slice(2)) {
    assert.equal(field(line, 'prefix_slots2_ratio'), field(line, 'prefix_ratio'), line);
    assert.equal(field(line, 'prefix_prev_ratio'), 0, line);
  }
});

test('reuse: one conversation at a time keeps prev, slots2 and the upper bound together', () => {
  const lines = [];
  const m = new Measure({ log: (line) => lines.push(line) });
  for (const prompt of conversation('solo', 8)) {
    m.record({ id: 'x', model: 'm', stream: false, outcome: 'ok', prompt, result: okResult(prompt) });
  }
  for (const line of lines.slice(1)) {
    const upper = field(line, 'prefix_ratio');
    assert.ok(upper > 0.6, line);
    assert.equal(field(line, 'prefix_slots2_ratio'), upper, line);
    assert.equal(field(line, 'prefix_prev_ratio'), upper, line);
    assert.equal(field(line, 'prefix_slots2_chars'), field(line, 'prefix_chars'), line);
  }
  const s = m.stats();
  assert.equal(s.prefix_slots2_ratio_mean, s.prefix_ratio_mean);
  assert.equal(s.prefix_prev_ratio_mean, s.prefix_ratio_mean);
});

test('reuse: a request the engine never ran does not occupy a slot', () => {
  const lines = [];
  const m = new Measure({ log: (line) => lines.push(line) });
  const [first, second] = conversation('q', 2);
  m.record({ id: '1', model: 'm', outcome: 'ok', prompt: first, result: okResult(first) });
  m.record({ id: 'busy', model: 'm', outcome: 'ring_busy', prompt: text('other', 2000) });
  m.record({ id: '2', model: 'm', outcome: 'ok', prompt: second, result: okResult(second) });
  assert.ok(field(lines[2], 'prefix_prev_ratio') > 0.5);
});

/** A bridge whose engine echoes a fixed reply; prompts reach it as rendered. */
function stubBridge(logs) {
  const model = {
    id: 'step-3.7-flash', name: 'Step', maxTokens: 64, options: '', promptFormat: 'chatml',
    reasoning: false, toolFormat: 'none', stages: [], contextSize: 16384,
    maxConcurrent: 1, maxQueue: 4, queueTimeoutMs: 60_000, requestTimeoutMs: 60_000,
  };
  return {
    measureOptions: { log: (line) => logs.push(line) },
    catalog: { models: [model] },
    serving: new Map([[model.id, { serving: true, stages: [] }]]),
    recordContribution() {},
    snapshots: new Map(),
    dialLedger: () => [],
    lastInspectError: null,
    operatorWallet: '',
    startedAt: Date.now(),
    async pipelineFor() {
      return {
        generate: async ({ prompt, onToken }) => {
          await new Promise((r) => setTimeout(r, 20));
          onToken?.({ text: 'ok' });
          return {
            requestId: 'r', text: 'ok', finishReason: 'eos', completionTokens: 1,
            promptTokens: Math.ceil(prompt.length / 4), firstTokenMs: 20, elapsedMs: 270, stageRows: {},
          };
        },
      };
    },
  };
}

async function withServer(bridge, fn) {
  const server = createServer(bridge);
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try { return await fn(`http://127.0.0.1:${server.address().port}`); } finally { server.close(); }
}

const complete = (base, messages, stream = false) => fetch(`${base}/c/step-3.7-flash/v1/chat/completions`, {
  method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ messages, stream }),
}).then((r) => r.text());

test('HTTP: one line per request, numbers only — the prompt never reaches the output', async (t) => {
  const logs = [];
  const printed = [];
  // Everything the process prints, not just the meter's own sink.
  for (const name of ['log', 'error', 'warn', 'info']) {
    t.mock.method(console, name, (...args) => printed.push(args.join(' ')));
  }
  const canary = 'CANARY-7f3a-do-not-log';
  const system = { role: 'system', content: `${canary} ${text('rules', 2000)}` };
  await withServer(stubBridge(logs), async (base) => {
    await complete(base, [system, { role: 'user', content: `${canary} first question` }]);
    await complete(base, [system, { role: 'user', content: `${canary} first question` },
      { role: 'assistant', content: 'ok' }, { role: 'user', content: `${canary} follow-up` }], true);
    const health = await (await fetch(`${base}/health`)).json();
    printed.push(JSON.stringify(health));
    assert.equal(health.measure.count, 2);
    assert.equal(health.measure.window, 2);
    assert.equal(health.measure.ttft_ms.p50, 20);
    assert.ok(health.measure.prefill_tps.p50 > 0);
    assert.ok(health.measure.prefix_ratio_mean > 0.4, 'the second turn re-sent the first');
    assert.equal(health.measure.prefix_slots2_ratio_mean, health.measure.prefix_ratio_mean);
    assert.equal(health.measure.prefix_prev_ratio_mean, health.measure.prefix_ratio_mean);
  });
  assert.equal(logs.length, 2);
  for (const line of [...logs, ...printed]) assert.ok(!line.includes(canary), line);
  for (const line of logs) {
    assert.match(line, /^\[measure\] id=chatcmpl-[0-9a-f]+ model=step-3\.7-flash stream=[01] outcome=ok /);
    assert.ok(!/rules|question|follow/.test(line), line);
  }
  assert.match(logs[0], /prefix_chars=0 .*system_match=0/);
  assert.match(logs[1], /stream=1 .*system_match=1/);
  const ratio = Number(/prefix_ratio=([\d.]+)/.exec(logs[1])[1]);
  assert.ok(ratio > 0.8, `follow-up shares most of its prompt (${ratio})`);
  assert.match(logs[1], /prompt_tokens_src=engine/);
});

test('HTTP: P4_BRIDGE_MEASURE=0 turns it off', async (t) => {
  const previous = process.env.P4_BRIDGE_MEASURE;
  process.env.P4_BRIDGE_MEASURE = '0';
  t.after(() => { if (previous === undefined) delete process.env.P4_BRIDGE_MEASURE; else process.env.P4_BRIDGE_MEASURE = previous; });
  const logs = [];
  await withServer(stubBridge(logs), async (base) => {
    await complete(base, [{ role: 'user', content: 'hi' }]);
    const health = await (await fetch(`${base}/health`)).json();
    assert.equal(health.measure, undefined);
  });
  assert.equal(logs.length, 0);
});

'use strict';
/**
 * The per-model limit: one count and one queue for streaming and non-streaming
 * requests, a slot back the moment its client leaves, and never twice.
 *
 * The ring's KV cache is one 32768-token pool for up to eight sequences, so
 * the limit is what keeps a request's context its own. These tests drive the
 * gate directly and through the HTTP server with a pipeline the test controls.
 */
const test = require('node:test');
const assert = require('node:assert/strict');
const http = require('node:http');

const { Gate, maxConcurrentFor, estimateTokens } = require('../gate.js');
const { createServer } = require('../server.js');

const tick = (ms = 10) => new Promise((resolve) => setTimeout(resolve, ms));

async function until(check, what, ms = 2000) {
  const deadline = Date.now() + ms;
  while (!check()) {
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await tick(5);
  }
}

/* ---- the gate alone ----------------------------------------------------- */

test('requests past the limit wait in order and run as slots free', async () => {
  const gate = new Gate({ limit: 2 });
  const a = await gate.acquire();
  const b = await gate.acquire();
  const order = [];
  const c = gate.acquire().then((release) => { order.push('c'); return release; });
  const d = gate.acquire().then((release) => { order.push('d'); return release; });
  assert.deepEqual(gate.stats(), { max_concurrent: 2, in_flight: 2, queued: 2, max_queue: 16 });
  a();
  const releaseC = await c;
  assert.deepEqual(order, ['c']);
  b();
  const releaseD = await d;
  assert.deepEqual(order, ['c', 'd']);
  releaseC(); releaseD();
  assert.equal(gate.stats().in_flight, 0);
});

test('a full queue and a long wait both answer ring_busy', async () => {
  const gate = new Gate({ limit: 1, maxQueue: 1, timeoutMs: 30 });
  const held = await gate.acquire();
  const waiting = gate.acquire();
  await assert.rejects(gate.acquire(), (error) => error.code === 'ring_busy' && error.retryAfterS > 0);
  await assert.rejects(waiting, (error) => error.code === 'ring_busy' && /no slot freed/.test(error.message));
  assert.equal(gate.stats().queued, 0);
  held();
  assert.equal(gate.stats().in_flight, 0);
});

test('an abort while queued leaves the queue and takes no slot', async () => {
  const gate = new Gate({ limit: 1 });
  const held = await gate.acquire();
  const leaving = new AbortController();
  const waiting = gate.acquire(leaving.signal);
  const next = gate.acquire();
  assert.equal(gate.stats().queued, 2);
  leaving.abort();
  await assert.rejects(waiting, (error) => error.code === 'aborted');
  assert.equal(gate.stats().queued, 1);
  held();
  const releaseNext = await next;
  assert.deepEqual([gate.stats().in_flight, gate.stats().queued], [1, 0]);
  releaseNext();
});

test('release counts once, however many paths call it', async () => {
  const gate = new Gate({ limit: 1 });
  const release = await gate.acquire();
  const queued = gate.acquire();
  assert.equal(release(), true);
  assert.equal(release(), false);
  assert.equal(release(), false);
  const second = await queued;
  assert.equal(gate.stats().in_flight, 1);    // not 0, not -1: the second holder still has it
  second();
  assert.equal(gate.stats().in_flight, 0);
});

test('the limit comes from the catalog or from ring and request context', () => {
  assert.equal(maxConcurrentFor({ id: 'm', ring_context_size: 32768, context_size: 16384 }), 2);
  assert.equal(maxConcurrentFor({ id: 'm', ring_context_size: 32768, context_size: 4096 }), 8);
  assert.equal(maxConcurrentFor({ id: 'm', max_concurrent: 3, ring_context_size: 32768, context_size: 16384 }), 3);
  assert.equal(maxConcurrentFor({ id: 'm', context_size: 16384 }), null);
  assert.throws(() => maxConcurrentFor({ id: 'm', max_concurrent: 0 }), /positive integer/);
  assert.equal(estimateTokens('abcdefg'), 3);
});

/* ---- over HTTP ---------------------------------------------------------- */

/**
 * A bridge whose pipeline holds every request until the test lets it go.
 * `hold` true: generate() ignores the abort, as a ring with no cancel would,
 * and stays pending until released. Each call is recorded.
 */
function heldBridge({ limit = 1, maxQueue = 16, queueTimeoutMs = 120_000, contextSize = 16384, requestTimeoutMs = 120_000 } = {}) {
  const model = {
    id: 'step-3.7-flash', name: 'Step', maxTokens: 512, options: '', promptFormat: 'chatml',
    reasoning: true, toolFormat: 'step', stages: [], contextSize,
    maxConcurrent: limit, maxQueue, queueTimeoutMs, requestTimeoutMs,
  };
  const calls = [];
  const bridge = {
    calls,
    model,
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
        generate({ onToken, abort, timeoutMs }) {
          return new Promise((resolve, reject) => {
            const call = {
              abort,
              timeoutMs,
              finish: (text = 'ok') => { onToken?.({ text }); resolve({ requestId: 'r', text, finishReason: 'eos', completionTokens: 1, promptTokens: 1, stageRows: {} }); },
              fail: (message = 'engine fell over') => reject(new Error(message)),
            };
            calls.push(call);
            onToken?.({ text: '' });
          });
        },
      };
    },
  };
  return bridge;
}

async function listen(bridge) {
  const server = createServer(bridge);
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return server;
}

/** Start a request; resolves the response, and exposes a way to cut it off. */
function start(server, body) {
  const payload = JSON.stringify({ messages: [{ role: 'user', content: 'hi' }], ...body });
  let request;
  const done = new Promise((resolve) => {
    request = http.request({
      host: '127.0.0.1', port: server.address().port, method: 'POST',
      path: '/c/step-3.7-flash/v1/chat/completions', headers: { 'content-type': 'application/json' },
    }, (response) => {
      let text = '';
      response.on('data', (chunk) => { text += chunk; });
      response.on('end', () => resolve({ status: response.statusCode, headers: response.headers, text }));
      response.on('error', () => resolve({ status: response.statusCode, aborted: true }));
    });
    request.on('error', () => resolve({ aborted: true }));
    request.end(payload);
  });
  return { done, cut: () => request.destroy() };
}

const stats = (bridge) => bridge.gates.get('step-3.7-flash').stats();

test('HTTP: streaming and non-streaming share one limit and one queue', async () => {
  const bridge = heldBridge({ limit: 2 });
  const server = await listen(bridge);
  try {
    const a = start(server, { stream: true });
    const b = start(server, {});
    await until(() => bridge.calls.length === 2, 'two in flight');
    const c = start(server, { stream: true });
    const d = start(server, {});
    await until(() => stats(bridge).queued === 2, 'two queued');
    assert.deepEqual([stats(bridge).in_flight, bridge.calls.length], [2, 2]);
    // Health shows it too.
    for (const path of ['/api/runtime', '/health']) {
      const shown = await (await fetch(`http://127.0.0.1:${server.address().port}${path}`)).json();
      assert.deepEqual(shown.queues['step-3.7-flash'], { max_concurrent: 2, in_flight: 2, queued: 2, max_queue: 16 }, path);
    }
    bridge.calls[1].finish();           // the non-streaming one: a streaming request takes its place
    await until(() => bridge.calls.length === 3, 'the next one admitted');
    assert.equal(stats(bridge).queued, 1);
    bridge.calls[0].finish();
    await until(() => bridge.calls.length === 4, 'the last one admitted');
    bridge.calls[2].finish(); bridge.calls[3].finish();
    const results = await Promise.all([a.done, b.done, c.done, d.done]);
    assert.deepEqual(results.map((r) => r.status), [200, 200, 200, 200]);
    assert.equal(stats(bridge).in_flight, 0);
  } finally {
    server.close();
  }
});

test('HTTP: a full queue answers 503 ring_busy with Retry-After', async () => {
  const bridge = heldBridge({ limit: 1, maxQueue: 1 });
  const server = await listen(bridge);
  try {
    const a = start(server, {});
    await until(() => bridge.calls.length === 1, 'one in flight');
    const b = start(server, { stream: true });
    await until(() => stats(bridge).queued === 1, 'one queued');
    const c = await start(server, {}).done;
    assert.equal(c.status, 503);
    assert.equal(JSON.parse(c.text).error.type, 'ring_busy');
    assert.ok(Number(c.headers['retry-after']) > 0);
    bridge.calls[0].finish();
    await until(() => bridge.calls.length === 2, 'the queued one admitted');
    bridge.calls[1].finish();
    assert.deepEqual([(await a.done).status, (await b.done).status], [200, 200]);
  } finally {
    server.close();
  }
});

test('HTTP: waiting past the queue timeout answers 503 ring_busy', async () => {
  const bridge = heldBridge({ limit: 1, queueTimeoutMs: 40 });
  const server = await listen(bridge);
  try {
    const a = start(server, {});
    await until(() => bridge.calls.length === 1, 'one in flight');
    const b = await start(server, { stream: true }).done;
    assert.equal(b.status, 503);
    assert.equal(JSON.parse(b.text).error.type, 'ring_busy');
    assert.ok(b.headers['retry-after']);
    bridge.calls[0].finish();
    assert.equal((await a.done).status, 200);
  } finally {
    server.close();
  }
});

test('HTTP: a client that leaves while queued is taken out of the queue', async () => {
  const bridge = heldBridge({ limit: 1 });
  const server = await listen(bridge);
  try {
    const a = start(server, {});
    await until(() => bridge.calls.length === 1, 'one in flight');
    const b = start(server, {});
    await until(() => stats(bridge).queued === 1, 'one queued');
    b.cut();
    await until(() => stats(bridge).queued === 0, 'the queue emptied');
    bridge.calls[0].finish();
    assert.equal((await a.done).status, 200);
    await tick(20);
    assert.equal(bridge.calls.length, 1);      // the one that left never reached the engine
    assert.equal(stats(bridge).in_flight, 0);
  } finally {
    server.close();
  }
});

test('HTTP: a client that leaves in flight keeps its slot until the engine finishes', async () => {
  const bridge = heldBridge({ limit: 1 });
  const server = await listen(bridge);
  try {
    const a = start(server, { stream: true });
    await until(() => bridge.calls.length === 1, 'one in flight');
    const b = start(server, {});
    await until(() => stats(bridge).queued === 1, 'one queued');
    a.cut();
    await tick(50);
    // p4 cannot cancel, so the first sequence still holds its share of the KV
    // pool: the second must not start yet, and the engine was never told to stop.
    assert.equal(bridge.calls[0].abort, undefined);
    assert.deepEqual([bridge.calls.length, stats(bridge).in_flight, stats(bridge).queued], [1, 1, 1]);
    bridge.calls[0].finish('a reply nobody reads');
    await until(() => bridge.calls.length === 2, 'the queued one admitted once the engine finished');
    assert.deepEqual([stats(bridge).in_flight, stats(bridge).queued], [1, 0]);
    bridge.calls[1].finish();
    assert.equal((await b.done).status, 200);
    assert.equal(stats(bridge).in_flight, 0);
  } finally {
    server.close();
  }
});

test('HTTP: an engine error after the client left releases once', async () => {
  const bridge = heldBridge({ limit: 1 });
  const server = await listen(bridge);
  try {
    const a = start(server, {});
    await until(() => bridge.calls.length === 1, 'one in flight');
    const b = start(server, {});
    const c = start(server, {});
    await until(() => stats(bridge).queued === 2, 'two queued');
    a.cut();
    await tick(30);
    assert.equal(bridge.calls.length, 1);
    bridge.calls[0].fail();
    await until(() => bridge.calls.length === 2, 'b admitted');
    await tick(30);
    // One release for a: b in, c still waiting. Two would have let c in too.
    assert.deepEqual([stats(bridge).in_flight, stats(bridge).queued, bridge.calls.length], [1, 1, 2]);
    bridge.calls[1].finish();
    await until(() => bridge.calls.length === 3, 'c admitted');
    bridge.calls[2].finish();
    assert.deepEqual([(await b.done).status, (await c.done).status], [200, 200]);
    assert.equal(stats(bridge).in_flight, 0);
  } finally {
    server.close();
  }
});

test('HTTP: an engine that never answers gives its slot back at the request timeout, once', async () => {
  const bridge = heldBridge({ limit: 1, requestTimeoutMs: 80 });
  const server = await listen(bridge);
  try {
    const a = start(server, {});
    await until(() => bridge.calls.length === 1, 'one in flight');
    assert.equal(bridge.calls[0].timeoutMs, 80);       // the engine read is bounded by the same number
    a.cut();
    const b = start(server, {});
    const c = start(server, {});
    await until(() => stats(bridge).queued === 2, 'two queued');
    await until(() => bridge.calls.length === 2, 'b admitted after the timeout', 1000);
    // The engine answering late must not release a's slot a second time.
    bridge.calls[0].finish();
    await tick(30);
    assert.deepEqual([stats(bridge).in_flight, stats(bridge).queued, bridge.calls.length], [1, 1, 2]);
    bridge.calls[1].finish();
    await until(() => bridge.calls.length === 3, 'c admitted');
    bridge.calls[2].finish();
    assert.deepEqual([(await b.done).status, (await c.done).status], [200, 200]);
  } finally {
    server.close();
  }
});

test('the token estimate counts Korean a token a character', () => {
  assert.equal(estimateTokens('abcdef'), 2);
  assert.equal(estimateTokens('안녕하세요'), 5);
  assert.equal(estimateTokens('ab 안녕'), 3);
});

test('HTTP: a Korean prompt that cannot fit is refused', async () => {
  const bridge = heldBridge({ limit: 1, contextSize: 2048 });
  const server = await listen(bridge);
  try {
    // 1800 Hangul characters: about 600 tokens at 3 a token, which would fit
    // 2048 with 512 for the reply; counted a token each, they do not.
    const r = await start(server, { messages: [{ role: 'user', content: '가'.repeat(1800) }] }).done;
    assert.equal(r.status, 400);
    assert.equal(JSON.parse(r.text).error.code, 'context_length_exceeded');
    assert.equal(bridge.calls.length, 0);
  } finally {
    server.close();
  }
});

test('HTTP: a prompt that cannot fit is refused before it is queued', async () => {
  const bridge = heldBridge({ limit: 1, contextSize: 600 });
  const server = await listen(bridge);
  try {
    // 512 reserved for the reply leaves room for about 88 tokens of prompt.
    const fits = start(server, { max_tokens: 64 });
    await until(() => bridge.calls.length === 1, 'the small one admitted');
    const big = await start(server, { messages: [{ role: 'user', content: 'x'.repeat(2000) }] }).done;
    assert.equal(big.status, 400);
    const error = JSON.parse(big.text).error;
    assert.equal(error.code, 'context_length_exceeded');
    assert.equal(error.type, 'invalid_request_error');
    assert.equal(stats(bridge).queued, 0);
    bridge.calls[0].finish();
    assert.equal((await fits.done).status, 200);
  } finally {
    server.close();
  }
});

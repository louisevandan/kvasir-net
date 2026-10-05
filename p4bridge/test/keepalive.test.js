'use strict';
/**
 * Keeping a slow reply alive through Cloudflare, and answering a queued one
 * before Cloudflare gives up on it.
 *
 * Cloudflare cuts a response that has been silent for about 100 s. A stream
 * is silent through prefill; a queued request is silent until it has a slot.
 */
const test = require('node:test');
const assert = require('node:assert/strict');
const http = require('node:http');
const { EventEmitter } = require('node:events');
const path = require('node:path');

const { createServer, sseKeepalive, SSE_KEEPALIVE, SSE_KEEPALIVE_MS } = require('../server.js');
const { Gate, DEFAULT_QUEUE_TIMEOUT_S } = require('../gate.js');
const catalog = require('../catalog.js');

/** Enough of a ServerResponse for the keepalive: writes, and its end. */
function fakeResponse() {
  const res = new EventEmitter();
  res.writes = [];
  res.writableEnded = false;
  res.destroyed = false;
  res.write = (data) => { res.writes.push(data); return true; };
  res.end = () => { res.writableEnded = true; res.emit('finish'); };
  return res;
}

const keepalives = (res) => res.writes.filter((w) => w === SSE_KEEPALIVE).length;

test('keepalive: 15 s by default, comfortably inside the 100 s cut', () => {
  assert.equal(SSE_KEEPALIVE_MS, 15_000);
  assert.equal(SSE_KEEPALIVE, ': keepalive\n\n');
  // Worst case is just under two intervals of silence.
  assert.ok(2 * SSE_KEEPALIVE_MS < 100_000);
});

test('keepalive: sent every 15 s while nothing else is written', (t) => {
  t.mock.timers.enable({ apis: ['setInterval'] });
  const res = fakeResponse();
  const ka = sseKeepalive(res, 15_000);
  // The role delta has just gone out with writeHead; the first tick is quiet.
  t.mock.timers.tick(15_000);
  assert.equal(keepalives(res), 0);
  t.mock.timers.tick(15_000);
  assert.equal(keepalives(res), 1);
  t.mock.timers.tick(15_000);
  t.mock.timers.tick(15_000);
  assert.equal(keepalives(res), 3, 'and every 15 s after, through a long prefill');
  ka.stop();
});

test('keepalive: a frame in the last 15 s holds it off', (t) => {
  t.mock.timers.enable({ apis: ['setInterval'] });
  const res = fakeResponse();
  const ka = sseKeepalive(res, 15_000);
  for (let i = 0; i < 6; i += 1) {
    t.mock.timers.tick(10_000);
    ka.touch();                       // a token every 10 s
  }
  assert.equal(keepalives(res), 0);
  // Tokens stop; the keepalive resumes.
  t.mock.timers.tick(15_000);
  t.mock.timers.tick(15_000);
  assert.equal(keepalives(res), 1);
  ka.stop();
});

test('keepalive: nothing is written after end, close or error', (t) => {
  t.mock.timers.enable({ apis: ['setInterval'] });
  for (const finish of [
    (res) => res.end(),
    (res) => { res.destroyed = true; res.emit('close'); },
    (res) => res.emit('error', new Error('reset')),
  ]) {
    const res = fakeResponse();
    res.on('error', () => {});
    sseKeepalive(res, 15_000);
    t.mock.timers.tick(30_000);
    assert.equal(keepalives(res), 1);
    finish(res);
    const before = res.writes.length;
    t.mock.timers.tick(120_000);
    assert.equal(res.writes.length, before, String(finish));
  }
});

test('keepalive: an explicit stop() ends it too', (t) => {
  t.mock.timers.enable({ apis: ['setInterval'] });
  const res = fakeResponse();
  const ka = sseKeepalive(res, 15_000);
  ka.stop();
  t.mock.timers.tick(120_000);
  assert.equal(res.writes.length, 0);
});

/** A bridge whose engine stays silent for `silentMs`, then answers. */
function slowBridge(silentMs) {
  const model = {
    id: 'step-3.7-flash', name: 'Step', maxTokens: 64, options: '', promptFormat: 'chatml',
    reasoning: false, toolFormat: 'none', stages: [], contextSize: 16384,
    maxConcurrent: 1, maxQueue: 4, queueTimeoutMs: 60_000, requestTimeoutMs: 60_000,
  };
  return {
    sseKeepaliveMs: 25,
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
        generate: ({ onToken }) => new Promise((resolve) => setTimeout(() => {
          onToken?.({ text: 'hello' });
          resolve({ requestId: 'r', text: 'hello', finishReason: 'eos', completionTokens: 1, promptTokens: 3, stageRows: {} });
        }, silentMs)),
      };
    },
  };
}

test('HTTP: a silent prefill is kept alive, and the stream still ends cleanly', async () => {
  const server = createServer(slowBridge(200));
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const response = await fetch(`http://127.0.0.1:${server.address().port}/c/step-3.7-flash/v1/chat/completions`, {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: 'hi' }], stream: true }),
    });
    assert.equal(response.status, 200);
    const text = await response.text();
    const role = text.indexOf('"role":"assistant"');
    const first = text.indexOf(SSE_KEEPALIVE);
    const token = text.indexOf('hello');
    assert.ok(role >= 0 && first > role && token > first, 'keepalive between the role delta and the first token');
    assert.ok(text.split(SSE_KEEPALIVE).length - 1 >= 2, 'repeated through the silence');
    assert.ok(text.endsWith('data: [DONE]\n\n'), 'nothing after [DONE]');
    // Every non-comment event is still well-formed JSON.
    for (const event of text.split('\n\n').filter(Boolean)) {
      if (event.startsWith(':')) continue;
      const payload = event.replace(/^data: /, '');
      if (payload !== '[DONE]') JSON.parse(payload);
    }
  } finally {
    server.close();
  }
});

test('HTTP: a non-stream reply carries no keepalive bytes', async () => {
  const server = createServer(slowBridge(100));
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const response = await fetch(`http://127.0.0.1:${server.address().port}/c/step-3.7-flash/v1/chat/completions`, {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: 'hi' }] }),
    });
    const text = await response.text();
    assert.equal(response.status, 200);
    assert.equal(text[0], '{');
    assert.equal(JSON.parse(text).choices[0].message.content, 'hello');
  } finally {
    server.close();
  }
});

test('queue timeout: ring_busy is answered before Cloudflare cuts at ~100 s', () => {
  assert.ok(DEFAULT_QUEUE_TIMEOUT_S * 1000 <= 80_000);
  assert.ok(new Gate({ limit: 1 }).timeoutMs <= 80_000, 'the Gate default');
  // A catalog entry that says nothing gets the default...
  const fs = require('node:fs');
  const os = require('node:os');
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'p4-cat-')), 'catalog.json');
  fs.writeFileSync(file, JSON.stringify({ ingress_agent: 'tcp://127.0.0.1:1', models: [{
    id: 'x', load_generation: 1, stages: [{ agent: 'tcp://127.0.0.1:1', node: 'n', generation: 1 }],
  }] }));
  assert.ok(catalog.load(file).models[0].queueTimeoutMs <= 80_000);
  // ...and the shipped step-3.7-flash entries say so explicitly.
  for (const name of ['catalog.json', 'catalog.example.json']) {
    const entry = catalog.load(path.join(__dirname, '..', name)).models.find((m) => m.id === 'step-3.7-flash');
    assert.ok(entry.queueTimeoutMs <= 80_000, name);
  }
});

test('queue timeout: a request still queued at the timeout gets ring_busy with Retry-After', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const gate = new Gate({ limit: 1 });
  const held = await gate.acquire();
  const waiting = gate.acquire();
  t.mock.timers.tick(DEFAULT_QUEUE_TIMEOUT_S * 1000);
  await assert.rejects(waiting, (error) => error.code === 'ring_busy' && error.retryAfterS > 0);
  held();
});

'use strict';
/**
 * What the gateway does with each kind of bridge answer.
 *
 * A fake bridge answers with a chosen status and body; a bare http server
 * stands in for the gateway route and calls forwardCompletion with stubs for
 * the ring reload and the debit, so the test can see which of them ran.
 */
const test = require('node:test');
const assert = require('node:assert/strict');
const http = require('node:http');

const { forwardCompletion, classifyFailure } = require('../bridgeForward.js');

async function serve(handler) {
  const server = http.createServer(handler);
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return { server, url: `http://127.0.0.1:${server.address().port}` };
}

/** A gateway whose bridge answers `answer(req, res)`; returns what happened. */
async function call(answer, { stream = false } = {}) {
  const bridge = await serve(answer);
  const seen = { reloads: 0, debits: [], oks: 0 };
  // ringOutage as the gateway has it, with the reload stubbed.
  const ringOutage = (res, _m, detail) => {
    seen.reloads += 1;
    res.statusCode = 503;
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify({ error: { type: 'hub_unavailable', code: 'ring_recovering', detail } }));
  };
  const gateway = await serve((req, res) => {
    forwardCompletion({
      res, m: { bridgeUrl: bridge.url, cid: 'step-3.7-flash' }, fwd: { stream }, wantStream: stream,
      headers: { 'Content-Type': 'application/json' }, ringOutage,
      debit: (usage) => seen.debits.push(usage), markOk: () => { seen.oks += 1; },
    });
  });
  try {
    const response = await fetch(`${gateway.url}/v1/chat/completions`, { method: 'POST', body: '{}' });
    const text = await response.text();
    return { ...seen, status: response.status, retryAfter: response.headers.get('retry-after'), text };
  } finally {
    gateway.server.close();
    bridge.server.close();
  }
}

const json = (status, body, headers = {}) => (_req, res) => {
  res.writeHead(status, { 'content-type': 'application/json', ...headers });
  res.end(JSON.stringify(body));
};

const TOO_LONG = { error: { message: 'too long', type: 'invalid_request_error', code: 'context_length_exceeded' } };
const BUSY = { error: { message: 'model step-3.7-flash is busy', type: 'ring_busy' } };

for (const stream of [false, true]) {
  const mode = stream ? 'stream' : 'non-stream';

  test(`${mode}: a bridge 400 is passed through unchanged, with no reload and no debit`, async () => {
    const r = await call(json(400, TOO_LONG), { stream });
    assert.equal(r.status, 400);
    assert.deepEqual(JSON.parse(r.text), TOO_LONG);
    assert.deepEqual([r.reloads, r.debits.length], [0, 0]);
  });

  test(`${mode}: a bridge 503 ring_busy is passed through with Retry-After, no reload, no debit`, async () => {
    const r = await call(json(503, BUSY, { 'retry-after': '5' }), { stream });
    assert.equal(r.status, 503);
    assert.deepEqual(JSON.parse(r.text), BUSY);
    assert.equal(r.retryAfter, '5');
    assert.deepEqual([r.reloads, r.debits.length], [0, 0]);
  });

  test(`${mode}: a bridge 500 is still an outage and reloads the ring`, async () => {
    const r = await call(json(500, { error: { message: 'boom' } }), { stream });
    assert.equal(r.status, 503);
    assert.equal(JSON.parse(r.text).error.code, 'ring_recovering');
    assert.deepEqual([r.reloads, r.debits.length], [1, 0]);
  });

  test(`${mode}: a 503 that is not ring_busy is still an outage`, async () => {
    const r = await call(json(503, { error: { message: 'controller is not serving', type: 'ring_recovering' } }), { stream });
    assert.equal(r.reloads, 1);
  });
}

test('non-stream: a good answer is relayed and debited once', async () => {
  const usage = { prompt_tokens: 3, completion_tokens: 2, total_tokens: 5 };
  const r = await call(json(200, { choices: [{ message: { content: 'hi' } }], usage }));
  assert.equal(r.status, 200);
  assert.deepEqual(r.debits, [usage]);
  assert.deepEqual([r.reloads, r.oks], [0, 1]);
});

test('stream: a good stream is relayed and debited from its usage frame', async () => {
  const usage = { prompt_tokens: 3, completion_tokens: 2, total_tokens: 5 };
  const r = await call((_req, res) => {
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.write('data: {"choices":[{"delta":{"content":"hi"}}]}\n\n');
    res.end(`data: {"choices":[],"usage":${JSON.stringify(usage)}}\n\ndata: [DONE]\n\n`);
  }, { stream: true });
  assert.equal(r.status, 200);
  assert.ok(r.text.includes('[DONE]'));
  assert.deepEqual(r.debits, [usage]);
  assert.equal(r.reloads, 0);
});

test('stream: a 200 whose first frame is an error is still an outage', async () => {
  const r = await call((_req, res) => {
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.end('data: {"error":{"message":"ring down"}}\n\n');
  }, { stream: true });
  assert.deepEqual([r.status, r.reloads, r.debits.length], [503, 1, 0]);
});

test('classifyFailure', () => {
  assert.equal(classifyFailure(400, '{}'), 'client');
  assert.equal(classifyFailure(429, ''), 'client');
  assert.equal(classifyFailure(503, JSON.stringify(BUSY)), 'busy');
  assert.equal(classifyFailure(503, 'not json'), 'outage');
  assert.equal(classifyFailure(502, JSON.stringify(BUSY)), 'outage');
});

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

const { forwardCompletion, classifyFailure, anthropicErrorType, BRIDGE_TIMEOUT_MS } = require('../bridgeForward.js');

const tick = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function serve(handler) {
  const server = http.createServer(handler);
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return { server, url: `http://127.0.0.1:${server.address().port}` };
}

/**
 * A gateway whose bridge answers `answer(req, res)`. Resolves with what the
 * caller received and what the gateway did; `done` resolves when the
 * gateway's handler has returned (which, after a caller hangs up, is later).
 */
async function call(answer, { stream = false, timeoutMs = 5000, bridgeUrl, hangUpAfter } = {}) {
  const bridge = bridgeUrl ? null : await serve(answer);
  const seen = { reloads: 0, debits: [], oks: 0, logs: [] };
  const ringOutage = (res, _m, detail) => {
    seen.reloads += 1;
    res.statusCode = 503;
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify({ error: { type: 'hub_unavailable', code: 'ring_recovering', detail } }));
  };
  let handled;
  const finished = new Promise((resolve) => { handled = resolve; });
  const gateway = await serve((req, res) => {
    forwardCompletion({
      res, m: { bridgeUrl: bridgeUrl ?? bridge.url, cid: 'step-3.7-flash' }, fwd: { stream }, wantStream: stream,
      headers: { 'Content-Type': 'application/json' }, ringOutage, timeoutMs,
      debit: (usage) => seen.debits.push(usage), markOk: () => { seen.oks += 1; },
      log: (line) => seen.logs.push(line),
    }).finally(handled);
  });
  try {
    if (hangUpAfter) {
      // Read until the marker, then hang up.
      await new Promise((resolve) => {
        const req = http.request(`${gateway.url}/v1/chat/completions`, { method: 'POST' }, (response) => {
          let text = '';
          response.on('data', (chunk) => {
            text += chunk;
            if (text.includes(hangUpAfter)) { req.destroy(); resolve(); }
          });
          response.on('error', resolve);
        });
        req.on('error', resolve);
        req.end('{}');
      });
      await finished;
      return { ...seen };
    }
    const response = await fetch(`${gateway.url}/v1/chat/completions`, { method: 'POST', body: '{}' });
    const text = await response.text();
    await finished;
    return { ...seen, status: response.status, retryAfter: response.headers.get('retry-after'), text };
  } finally {
    gateway.server.close();
    bridge?.server.close();
    bridge?.server.closeAllConnections?.();
  }
}

const json = (status, body, headers = {}) => (_req, res) => {
  res.writeHead(status, { 'content-type': 'application/json', ...headers });
  res.end(JSON.stringify(body));
};

const TOO_LONG = { error: { message: 'too long', type: 'invalid_request_error', code: 'context_length_exceeded' } };
const BUSY = { error: { message: 'model step-3.7-flash is busy', type: 'ring_busy' } };
const USAGE = { prompt_tokens: 3, completion_tokens: 2, total_tokens: 5 };

for (const stream of [false, true]) {
  const mode = stream ? 'stream' : 'non-stream';

  for (const status of [400, 413, 422, 429]) {
    test(`${mode}: a bridge ${status} is passed through unchanged, with no reload and no debit`, async () => {
      const body = status === 400 ? TOO_LONG : { error: { message: `status ${status}`, type: 'x' } };
      const r = await call(json(status, body, status === 429 ? { 'retry-after': '3' } : {}), { stream });
      assert.equal(r.status, status);
      assert.deepEqual(JSON.parse(r.text), body);
      if (status === 429) assert.equal(r.retryAfter, '3');
      assert.deepEqual([r.reloads, r.debits.length, r.oks], [0, 0, 0]);
    });
  }

  test(`${mode}: a bridge 503 ring_busy is passed through with Retry-After, no reload, no debit`, async () => {
    const r = await call(json(503, BUSY, { 'retry-after': '5' }), { stream });
    assert.equal(r.status, 503);
    assert.deepEqual(JSON.parse(r.text), BUSY);
    assert.equal(r.retryAfter, '5');
    assert.deepEqual([r.reloads, r.debits.length], [0, 0]);
  });

  for (const status of [401, 403]) {
    test(`${mode}: a bridge ${status} is the gateway's misconfiguration: 502, logged, no reload`, async () => {
      const r = await call(json(status, { error: { message: 'service token required', type: 'unauthorized' } }), { stream });
      assert.equal(r.status, 502);
      const error = JSON.parse(r.text).error;
      assert.equal(error.type, 'gateway_misconfigured');
      assert.equal(anthropicErrorType(r.status, JSON.parse(r.text)), 'api_error');
      assert.ok(r.logs.some((line) => /GATEWAY MISCONFIGURED/.test(line)));
      assert.deepEqual([r.reloads, r.debits.length], [0, 0]);
    });
  }

  test(`${mode}: a bridge 404 keeps the outage path`, async () => {
    const r = await call(json(404, { error: { message: 'unknown controller step-3.7-flash' } }), { stream });
    assert.equal(r.status, 503);
    assert.equal(JSON.parse(r.text).error.code, 'ring_recovering');
    assert.deepEqual([r.reloads, r.debits.length], [1, 0]);
  });

  test(`${mode}: an unexpected 4xx is a 502 bad_upstream_response, no reload`, async () => {
    const r = await call(json(409, { error: { message: 'conflict' } }), { stream });
    assert.equal(r.status, 502);
    assert.equal(JSON.parse(r.text).error.type, 'bad_upstream_response');
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

  test(`${mode}: a bridge that is slower than the gateway timeout is a 504, not an outage`, async () => {
    // The bridge's queue wait plus its generation, past the gateway's limit.
    const r = await call(async (_req, res) => { await tick(300); json(200, { choices: [], usage: USAGE })(_req, res); },
      { stream, timeoutMs: 80 });
    assert.equal(r.status, 504);
    assert.equal(JSON.parse(r.text).error.type, 'upstream_timeout');
    assert.deepEqual([r.reloads, r.debits.length, r.oks], [0, 0, 0]);
  });

  test(`${mode}: a refused connection is still an outage`, async () => {
    const closed = await serve(() => {});
    const url = closed.url;
    await new Promise((resolve) => closed.server.close(resolve));
    const r = await call(null, { stream, bridgeUrl: url });
    assert.equal(r.status, 503);
    assert.deepEqual([r.reloads, r.debits.length], [1, 0]);
  });
}

test('the default gateway timeout covers the bridge queue and request timeouts', () => {
  assert.ok(BRIDGE_TIMEOUT_MS >= (120 + 300) * 1000);
  assert.equal(BRIDGE_TIMEOUT_MS, 450_000);
});

test('non-stream: a good answer is relayed and debited once', async () => {
  const r = await call(json(200, { choices: [{ message: { content: 'hi' } }], usage: USAGE }));
  assert.equal(r.status, 200);
  assert.deepEqual(r.debits, [USAGE]);
  assert.deepEqual([r.reloads, r.oks], [0, 1]);
});

test('non-stream: a body cut by the gateway timeout is a 504, not debited', async () => {
  const r = await call((_req, res) => {
    res.writeHead(200, { 'content-type': 'application/json' });
    res.write('{"choices":');                                     // and then nothing
  }, { timeoutMs: 80 });
  assert.equal(r.status, 504);
  assert.deepEqual([r.reloads, r.debits.length, r.oks], [0, 0, 0]);
});

test('stream: a good stream is relayed and debited from its usage frame', async () => {
  const r = await call((_req, res) => {
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.write('data: {"id":"chatcmpl-1","choices":[{"delta":{"content":"hi"}}]}\n\n');
    res.end(`data: {"choices":[],"usage":${JSON.stringify(USAGE)}}\n\ndata: [DONE]\n\n`);
  }, { stream: true });
  assert.equal(r.status, 200);
  assert.ok(r.text.includes('[DONE]'));
  assert.deepEqual(r.debits, [USAGE]);
  assert.deepEqual([r.reloads, r.oks], [0, 1]);
});

test('stream: a stream cut by the gateway timeout ends with an error frame and is not debited', async () => {
  const r = await call((_req, res) => {
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.write('data: {"id":"chatcmpl-cut","choices":[{"delta":{"content":"partial"}}]}\n\n');
    // ...and the usage frame never comes in time.
  }, { stream: true, timeoutMs: 120 });
  assert.equal(r.status, 200);
  assert.ok(r.text.includes('partial'));
  assert.ok(r.text.includes('"type":"upstream_timeout"'));
  assert.deepEqual([r.reloads, r.debits.length, r.oks], [0, 0, 0]);
  assert.ok(r.logs.some((line) => line.includes('request=chatcmpl-cut') && /bytes=\d+/.test(line)));
});

test('stream: a bridge stream that breaks mid-way is not debited', async () => {
  const r = await call((_req, res) => {
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.write('data: {"id":"chatcmpl-x","choices":[{"delta":{"content":"partial"}}]}\n\n');
    setTimeout(() => res.socket.destroy(), 20);
  }, { stream: true });
  assert.ok(r.text.includes('upstream_interrupted'));
  assert.deepEqual([r.reloads, r.debits.length, r.oks], [0, 0, 0]);
});

test('stream: a caller who hangs up mid-stream is still debited once when the usage arrives', async () => {
  const r = await call(async (_req, res) => {
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.write('data: {"id":"chatcmpl-h","choices":[{"delta":{"content":"first"}}]}\n\n');
    await tick(100);                              // the caller hangs up in here
    res.write('data: {"id":"chatcmpl-h","choices":[{"delta":{"content":"second"}}]}\n\n');
    res.end(`data: {"choices":[],"usage":${JSON.stringify(USAGE)}}\n\ndata: [DONE]\n\n`);
  }, { stream: true, hangUpAfter: 'first' });
  assert.deepEqual(r.debits, [USAGE]);
  assert.equal(r.reloads, 0);
});

test('stream: bridge keepalive comments reach the caller verbatim, and only usage is debited', async () => {
  const r = await call(async (_req, res) => {
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.write('data: {"id":"chatcmpl-k","choices":[{"delta":{"role":"assistant","content":""}}]}\n\n');
    // A long prefill: nothing but keepalives for a while.
    for (let i = 0; i < 3; i += 1) { await tick(10); res.write(': keepalive\n\n'); }
    res.write('data: {"id":"chatcmpl-k","choices":[{"delta":{"content":"hi"}}]}\n\n');
    res.end(`data: {"choices":[],"usage":${JSON.stringify(USAGE)}}\n\ndata: [DONE]\n\n`);
  }, { stream: true });
  assert.equal(r.status, 200);
  assert.equal(r.text.split(': keepalive\n\n').length - 1, 3, 'every comment passes through');
  assert.ok(r.text.indexOf(': keepalive') < r.text.indexOf('"content":"hi"'), 'in order');
  assert.deepEqual(r.debits, [USAGE], 'debited once, from the usage frame');
  assert.deepEqual([r.reloads, r.oks], [0, 1]);
});

test('stream: keepalives alone are not an answer; a stream cut before usage is not debited', async () => {
  const r = await call(async (_req, res) => {
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.write('data: {"id":"chatcmpl-c","choices":[{"delta":{"role":"assistant","content":""}}]}\n\n');
    for (let i = 0; i < 3; i += 1) { await tick(10); res.write(': keepalive\n\n'); }
    setTimeout(() => res.socket.destroy(), 10);
  }, { stream: true });
  assert.ok(r.text.includes(': keepalive'));
  assert.ok(r.text.includes('upstream_interrupted'));
  assert.deepEqual([r.reloads, r.debits.length, r.oks], [0, 0, 0]);
});

test('stream: a 200 whose first frame is an error is still an outage', async () => {
  const r = await call((_req, res) => {
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.end('data: {"error":{"message":"ring down"}}\n\n');
  }, { stream: true });
  assert.deepEqual([r.status, r.reloads, r.debits.length], [503, 1, 0]);
});

test('classifyFailure', () => {
  for (const status of [400, 413, 422, 429]) assert.equal(classifyFailure(status, '{}'), 'pass');
  assert.equal(classifyFailure(401, ''), 'misconfigured');
  assert.equal(classifyFailure(403, ''), 'misconfigured');
  assert.equal(classifyFailure(404, ''), 'missing');
  assert.equal(classifyFailure(409, ''), 'unexpected');
  assert.equal(classifyFailure(503, JSON.stringify(BUSY)), 'busy');
  assert.equal(classifyFailure(503, 'not json'), 'outage');
  assert.equal(classifyFailure(502, JSON.stringify(BUSY)), 'outage');
});

test('the Anthropic error types', () => {
  assert.equal(anthropicErrorType(502, { error: { type: 'gateway_misconfigured' } }), 'api_error');
  assert.equal(anthropicErrorType(401, { error: { type: 'unauthorized' } }), 'authentication_error');
  assert.equal(anthropicErrorType(503, BUSY), 'overloaded_error');
  assert.equal(anthropicErrorType(429, {}), 'overloaded_error');
  assert.equal(anthropicErrorType(400, TOO_LONG), 'invalid_request_error');
  assert.equal(anthropicErrorType(504, { error: { type: 'upstream_timeout' } }), 'api_error');
});

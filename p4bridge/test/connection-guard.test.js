'use strict';
/**
 * The bridge must not spend an agent's connection slots faster than the agent
 * can afford, because it never gets one back (transport.rs:900-902,
 * Semaphore(256) at mod.rs:48).
 *
 * Every test here fails against the code as it stood on 2026-09-25 before the
 * guard, except the two marked as regression guards. To run the control, point
 * P4_BRIDGE_SERVER at the backup:
 *
 *   P4_BRIDGE_SERVER=../server.js.before-connection-guard node --test test/connection-guard.test.js
 *
 * A test suite that passes against the code it is meant to reject has tested
 * nothing. Check the control before trusting a green run.
 */
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');
const net = require('node:net');
const crypto = require('node:crypto');

const SERVER = process.env.P4_BRIDGE_SERVER
  ? path.resolve(__dirname, process.env.P4_BRIDGE_SERVER)
  : path.join(__dirname, '..', 'server.js');
const { Bridge } = require(SERVER);
const wsrelay = require(path.join(__dirname, '..', 'wsrelay.js'));
const { OuterClient } = require(path.join(__dirname, '..', 'wire.js'));

const A = 'tcp://127.0.0.1:42011';
const B = 'tcp://127.0.0.1:42012';

const CATALOG = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'p4guard-')), 'catalog.json');
fs.writeFileSync(CATALOG, JSON.stringify({
  ingress_agent: A,
  models: [
    { id: 'on-A', load_generation: 111, stages: [{ agent: A, node: 'nA', generation: 1 }] },
    { id: 'on-B', load_generation: 222, stages: [{ agent: B, node: 'nB', generation: 1 }] },
  ],
}));

function makeBridge(dial) {
  const b = new Bridge({ catalogFile: CATALOG });
  b.closedAddrs = [];
  b.dialled = [];
  b.fakeClient = (addr) => ({
    addr,
    closed: false,
    close() { this.closed = true; b.closedAddrs.push(addr); },
    async inspect() {
      if (this.inspectFails) throw new Error('P4 request timed out after 10000 ms');
      return { nodes: [{ node_id: addr === A ? 'nA' : 'nB', state: 'loaded', generation: 1,
                         load_generation: addr === A ? 111 : 222 }] };
    },
  });
  // Only the socket is stubbed. ensureClient is the thing under test.
  b.dial = dial ?? (async (addr) => { b.dialled.push(addr); return b.fakeClient(addr); });
  // The pre-guard build has no dial(); stand in for it so the control runs.
  if (typeof Bridge.prototype.dial !== 'function') {
    b.ensureClient = async (addr) => {
      const agent = addr ?? b.catalog.ingressAgent;
      const have = b.clients.get(agent);
      if (have && !have.closed) return have;
      const client = await b.dial(agent);
      b.clients.set(agent, client);
      b.pipelines.clear();
      return client;
    };
  }
  return b;
}

test('a slow INSPECT keeps its connection -- the slot is not spent', async () => {
  const b = makeBridge();
  await b.refresh();
  assert.equal(b.dialled.length, 2);
  b.clients.get(B).inspectFails = true;          // slow, but the socket is fine
  await b.refresh();
  await b.refresh();
  assert.deepEqual(b.closedAddrs, [], 'closed a connection that was only slow');
  assert.equal(b.dialled.length, 2, 'redialled an agent whose connection was alive');
  assert.equal(b.serving.get('on-B').serving, false, 'a stale snapshot must not read as serving');
  assert.equal(b.serving.get('on-A').serving, true, 'the healthy model stays up');
});

test('a stalled agent is not probed again every 15 s', async () => {
  const b = makeBridge();
  await b.refresh();
  const client = b.clients.get(B);
  let probes = 0;
  client.inspect = async () => { probes += 1; throw new Error('P4 request timed out after 10000 ms'); };
  await b.refresh();                       // 1 probe, then backoff
  await b.refresh();
  await b.refresh();
  assert.equal(probes, 1, `probed a wedged agent ${probes} times; each one is an event it never resolves`);
  assert.equal(b.serving.get('on-B').serving, false, 'a skipped probe must not read as healthy');
});

test('two overlapping refreshes do not double-probe one agent', async () => {
  const b = makeBridge();
  await b.refresh();
  let open;
  const gate = new Promise((r) => { open = r; });
  let probes = 0;
  for (const addr of [A, B]) {
    b.clients.get(addr).inspect = async function () {
      probes += 1;
      await gate;
      return { nodes: [{ node_id: addr === A ? 'nA' : 'nB', state: 'loaded', generation: 1,
                         load_generation: addr === A ? 111 : 222 }] };
    };
  }
  const both = Promise.all([b.refresh(), b.refresh()]);
  await new Promise((r) => setImmediate(r));
  open();
  await both;
  assert.equal(probes, 2, `sent ${probes} INSPECTs for 2 agents`);
});

test('the probe backoff never holds a recovered agent out for more than a minute', async () => {
  const b = makeBridge();
  await b.refresh();
  b.clients.get(B).inspect = async () => { throw new Error('P4 request timed out after 10000 ms'); };
  for (let i = 0; i < 12; i += 1) {
    const seen = b.probe.get(B);
    if (seen) seen.nextAt = 0;                 // let every cycle through
    await b.refresh();
  }
  const wait = b.probe.get(B).nextAt - Date.now();
  assert.ok(wait <= 60_000, `a recovered agent would wait ${Math.round(wait / 1000)} s to be noticed`);
});

test('regression guard: a closed connection is replaced, not reused', async () => {
  const b = makeBridge();
  await b.refresh();
  const dead = b.clients.get(B);
  dead.closed = true;
  await b.refresh();
  assert.notEqual(b.clients.get(B), dead);
  assert.equal(b.dialled.filter((d) => d === B).length, 2);
});

test('regression guard: a connection that dies mid-INSPECT is dropped', async () => {
  const b = makeBridge();
  await b.refresh();
  b.clients.get(B).inspect = async function () {
    this.closed = true;
    throw new Error('P4 connection closed');
  };
  await b.refresh();
  assert.equal(b.clients.has(B), false);
  assert.deepEqual(b.closedAddrs, [B]);
});

test('one dial per agent at a time', async () => {
  let open;
  const gate = new Promise((r) => { open = r; });
  const b = makeBridge(async (addr) => { b.dialled.push(addr); await gate; return b.fakeClient(addr); });
  const all = Promise.all([b.ensureClient(A), b.ensureClient(A), b.ensureClient(A)]);
  open();
  await all;
  assert.equal(b.dialled.filter((d) => d === A).length, 1);
});

test('the hourly cap holds even when every dial fails', async () => {
  const b = makeBridge(async (addr) => { b.dialled.push(addr); throw new Error('refused'); });
  for (let i = 0; i < 20; i += 1) {
    const guard = b.guards?.get(A);
    if (guard) guard.nextDialAt = 0;             // defeat the backoff, keep the cap
    await b.ensureClient(A).catch(() => {});
  }
  assert.ok(b.dialled.length <= 6, `dialled ${b.dialled.length} times in an hour`);
});

test('the lifetime cap stops the bridge dialling, and says so', async () => {
  const b = makeBridge();
  b.guardFor(A).spent = 64;
  await assert.rejects(() => b.ensureClient(A), /not dialling again without an operator/);
  assert.ok(b.dialLedger().find((row) => row.agent === A).stopped);
});

test('a failed dial is not retried immediately', async () => {
  const b = makeBridge(async (addr) => { b.dialled.push(addr); throw new Error('refused'); });
  await assert.rejects(() => b.ensureClient(A));
  await assert.rejects(() => b.ensureClient(A), /unproductive dials, waiting/);
  assert.equal(b.dialled.length, 1);
});

test('a slot spent is never refunded by a later success', async () => {
  const b = makeBridge();
  await b.ensureClient(A);
  b.clients.get(A).closed = true;
  await b.ensureClient(A);
  assert.equal(b.guardFor(A).spent, 2);
});

test('one good INSPECT does not clear the backoff; five minutes of them do', async () => {
  const b = makeBridge();
  await b.refresh();
  b.guardFor(A).unproductive = 3;
  b.guardFor(A).healthySince = 0;
  await b.refresh();
  assert.equal(b.guardFor(A).unproductive, 3, 'cleared on a single success');
  b.guardFor(A).healthySince = Date.now() - 400_000;
  await b.refresh();
  assert.equal(b.guardFor(A).unproductive, 0);
});

test('the connect deadline reaches the socket layer', () => {
  const client = new OuterClient({ host: '127.0.0.1', port: 1, connectTimeoutMs: 1234 });
  assert.equal(client.connectTimeoutMs, 1234);
});

const listen = (server) => new Promise((r) => server.listen(0, '127.0.0.1', () => r(server.address().port)));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test('a client that sends FIN without a close frame takes the upstream with it', async (t) => {
  let upstreamSocket = null;
  let upstreamClosed = false;
  const upstream = net.createServer((socket) => {
    upstreamSocket = socket;
    socket.on('close', () => { upstreamClosed = true; });
    socket.resume();
  });
  const upstreamPort = await listen(upstream);

  const server = http.createServer((_req, res) => res.end());
  server.on('upgrade', (req, socket, head) => {
    wsrelay.bridge(req, socket, head, { host: '127.0.0.1', port: upstreamPort });
  });
  const port = await listen(server);

  const client = net.connect({ host: '127.0.0.1', port });
  t.after(() => { client.destroy(); upstreamSocket?.destroy(); upstream.close(); server.close(); });
  await new Promise((r) => client.once('connect', r));
  client.write(
    `GET /relay HTTP/1.1\r\nHost: test.local\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n`
    + `Sec-WebSocket-Key: ${crypto.randomBytes(16).toString('base64')}\r\nSec-WebSocket-Version: 13\r\n\r\n`);
  const head = await new Promise((r) => client.once('data', r));
  assert.match(head.toString('latin1'), /101/);

  await sleep(200);
  assert.ok(upstreamSocket, 'the relay never opened an upstream connection');
  assert.equal(upstreamClosed, false);

  // An app killed, a radio handover, a tunnel dropped: FIN and nothing else.
  client.end();
  await sleep(500);
  assert.equal(upstreamClosed, true,
    'the relay held its upstream open after the client hung up -- one leaked pair per disconnect');
});

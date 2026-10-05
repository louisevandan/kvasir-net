'use strict';
/**
 * Admitting and removing a wallet through the bridge's own HTTP surface, and
 * the relay upgrade as server.js wires it.
 *
 * The environment is read when server.js loads, so it is set first; node
 * --test runs each file in its own process, which keeps it from leaking.
 */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'p4bridge-admissions-'));
process.env.KVR_PARTICIPATION_LIST_FILE = path.join(dir, 'admitted.json');
process.env.KVR_PARTICIPATION_RESTRICTED = '1';
process.env.P4_BRIDGE_TOKEN = 'svc-secret';

const test = require('node:test');
const assert = require('node:assert');
const crypto = require('node:crypto');
const net = require('node:net');

const { NodeAuth } = require('../nodeauth');
const { Participation } = require('../participation');
const { createServer, attachRelays, participationPolicy } = require('../server');

const B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
function base58(buffer) {
  let n = BigInt('0x' + buffer.toString('hex'));
  let out = '';
  while (n > 0n) { out = B58[Number(n % 58n)] + out; n /= 58n; }
  for (const byte of buffer) { if (byte) break; out = `1${out}`; }
  return out;
}
function makeWallet() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const raw = publicKey.export({ format: 'der', type: 'spki' }).subarray(12);
  return {
    address: base58(raw),
    sign: (message) => crypto.sign(null, Buffer.from(message, 'utf8'), privateKey).toString('base64'),
  };
}

const MODEL = { id: 'm', name: 'M', nEmbd: 8, nLayer: 2, nExpert: 4, expertLayers: [1] };

async function start(t, { eligible = participationPolicy() } = {}) {
  const participation = new Participation({
    auth: new NodeAuth({ secret: 'test-secret', serviceToken: 'svc-secret', eligible }),
    credit: () => {},
    models: () => [MODEL],
  });
  const bridge = { participation };
  const server = createServer(bridge);
  attachRelays(server, bridge);
  const upstream = net.createServer((s) => s.on('error', () => {}));
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  await new Promise((r) => upstream.listen(0, '127.0.0.1', r));
  const sockets = [];
  t.after(() => {
    for (const s of sockets) s.destroy();
    upstream.close(); server.close(); participation.stop();
  });
  return { port: server.address().port, upstream, upstreamPort: upstream.address().port, participation, sockets };
}

async function call(port, method, p, { body, token, service = false } = {}) {
  const headers = { 'content-type': 'application/json' };
  if (token) headers.authorization = `Bearer ${token}`;
  if (service) headers['x-kvasir-service-token'] = 'svc-secret';
  const r = await fetch(`http://127.0.0.1:${port}${p}`, {
    method, headers, body: body ? JSON.stringify(body) : undefined,
  });
  return { status: r.status, body: await r.json().catch(() => null) };
}

async function mint(port, wallet) {
  const c = await call(port, 'POST', '/api/auth/challenge', { body: { wallet: wallet.address } });
  const m = await call(port, 'POST', '/api/auth/node-token', {
    body: { wallet: wallet.address, nonce: c.body.nonce, signature: wallet.sign(c.body.message) },
  });
  return m.body.node_token;
}

const upgradeRequest = (p) => `GET ${p} HTTP/1.1\r\nHost: t\r\nUpgrade: websocket\r\n`
  + `Connection: Upgrade\r\nSec-WebSocket-Key: ${crypto.randomBytes(16).toString('base64')}\r\n`
  + 'Sec-WebSocket-Version: 13\r\n\r\n';

/** Open a relay; resolves once spliced, with a promise of the close code. */
function openRelay(ctx, p) {
  return new Promise((resolve, reject) => {
    const reached = new Promise((r) => ctx.upstream.once('connection', r));
    const socket = net.connect(ctx.port, '127.0.0.1', () => socket.write(upgradeRequest(p)));
    ctx.sockets.push(socket);
    let seen = Buffer.alloc(0);
    let gotCode;
    const closeCode = new Promise((r) => { gotCode = r; });
    socket.on('data', (chunk) => {
      seen = Buffer.concat([seen, chunk]);
      const end = seen.indexOf('\r\n\r\n');
      const frame = end >= 0 ? seen.subarray(end + 4) : Buffer.alloc(0);
      if (frame.length >= 4 && (frame[0] & 0x0f) === 0x8) gotCode(frame.readUInt16BE(2));
    });
    socket.on('error', () => {});
    reached.then(() => resolve({ closeCode }));
    setTimeout(() => reject(new Error('relay never spliced')), 5000).unref?.();
  });
}

function closeCodeFor(port, p) {
  return new Promise((resolve, reject) => {
    const socket = net.connect(port, '127.0.0.1', () => socket.write(upgradeRequest(p)));
    let seen = Buffer.alloc(0);
    socket.on('data', (chunk) => {
      seen = Buffer.concat([seen, chunk]);
      const end = seen.indexOf('\r\n\r\n');
      const frame = end >= 0 ? seen.subarray(end + 4) : Buffer.alloc(0);
      if (frame.length >= 4 && (frame[0] & 0x0f) === 0x8) { resolve(frame.readUInt16BE(2)); socket.destroy(); }
    });
    socket.on('error', reject);
    setTimeout(() => reject(new Error('no close frame')), 5000).unref?.();
  });
}

const listed = () => JSON.parse(fs.readFileSync(process.env.KVR_PARTICIPATION_LIST_FILE, 'utf8'))
  .map((e) => e.wallet);

test('removal takes a wallet off the list and closes its open relays with 4403', async (t) => {
  const ctx = await start(t);
  const w = makeWallet();
  const other = makeWallet();

  // Only the service token may remove, as only it may admit.
  assert.equal((await call(ctx.port, 'POST', '/api/admissions/remove', { body: { wallet: w.address } })).status, 401);

  for (const wallet of [w, other]) {
    const added = await call(ctx.port, 'POST', '/api/admissions', { service: true, body: { wallet: wallet.address } });
    assert.equal(added.status, 200);
  }
  const token = await mint(ctx.port, w);
  const otherToken = await mint(ctx.port, other);
  assert.equal((await call(ctx.port, 'POST', '/api/expert-coverage', {
    token, body: { worker_id: 'w-1', model: 'm', segments: [[1, 0, 2]], url: 'relay:expert-w-1' } })).status, 200);
  assert.equal((await call(ctx.port, 'POST', '/api/expert-coverage', {
    token: otherToken, body: { worker_id: 'o-1', model: 'm', segments: [[1, 2, 4]], url: 'relay:expert-o-1' } })).status, 200);
  ctx.participation.relayTargets.get('expert-w-1').port = ctx.upstreamPort;
  ctx.participation.relayTargets.get('expert-o-1').port = ctx.upstreamPort;
  const mine = await openRelay(ctx, `/api/expert-relay?session=expert-w-1&token=${encodeURIComponent(token)}`);
  await openRelay(ctx, `/api/expert-relay?session=expert-o-1&token=${encodeURIComponent(otherToken)}`);
  assert.equal(ctx.participation.relays.size, 2);

  const removed = await call(ctx.port, 'POST', '/api/admissions/remove', { service: true, body: { wallet: w.address } });
  assert.equal(removed.status, 200);
  assert.equal(removed.body.removed, true);
  assert.equal(removed.body.still_admitted, false);
  assert.deepEqual(removed.body.revoked, { relays: 1, workers: 1, sessions: 1 });

  assert.equal(await mine.closeCode, 4403, 'the open relay is closed now, not at token expiry');
  assert.deepEqual(listed(), [other.address]);
  assert.equal(ctx.participation.relayTargets.has('expert-w-1'), false);
  assert.equal(ctx.participation.workers.has('w-1'), false);
  // The other wallet is untouched.
  assert.equal(ctx.participation.relays.size, 1);
  assert.equal(ctx.participation.relayTargets.has('expert-o-1'), true);

  // Its token still verifies, and still opens nothing.
  assert.equal((await call(ctx.port, 'GET', '/api/expert-demand', { token })).status, 403);

  // Removing again says it was not listed.
  const again = await call(ctx.port, 'POST', '/api/admissions/remove', { service: true, body: { wallet: w.address } });
  assert.equal(again.body.removed, false);
  assert.equal((await call(ctx.port, 'POST', '/api/admissions/remove', { service: true, body: {} })).status, 400);
});

test('a wallet taken off the file by hand loses its relays at the next recheck', async (t) => {
  const ctx = await start(t);
  const w = makeWallet();
  await call(ctx.port, 'POST', '/api/admissions', { service: true, body: { wallet: w.address } });
  const token = await mint(ctx.port, w);
  await call(ctx.port, 'POST', '/api/expert-coverage', {
    token, body: { worker_id: 'h-1', model: 'm', segments: [[1, 0, 2]], url: 'relay:expert-h-1' } });
  ctx.participation.relayTargets.get('expert-h-1').port = ctx.upstreamPort;
  const relay = await openRelay(ctx, `/api/expert-relay?session=expert-h-1&token=${encodeURIComponent(token)}`);

  // An operator edits the file directly; nothing tells the bridge.
  const file = process.env.KVR_PARTICIPATION_LIST_FILE;
  const kept = JSON.parse(fs.readFileSync(file, 'utf8')).filter((e) => e.wallet !== w.address);
  fs.writeFileSync(file, JSON.stringify(kept));
  const later = new Date(Date.now() + 5000);
  fs.utimesSync(file, later, later);   // a distinct mtime, so the cache re-reads

  assert.deepEqual(await ctx.participation.recheckAdmissions(), [w.address]);
  assert.equal(await relay.closeCode, 4403);
  assert.equal(ctx.participation.relayTargets.has('expert-h-1'), false);
});

test('server.js: a reset during a pending upgrade leaves the bridge serving', async (t) => {
  // Admission slow enough for the reset to land while it is pending.
  const ctx = await start(t, { eligible: () => new Promise((r) => setTimeout(() => r(true), 150)) });
  const token = await mint(ctx.port, makeWallet());
  const uncaught = [];
  const onUncaught = (error) => uncaught.push(error);
  process.on('uncaughtException', onUncaught);
  t.after(() => process.off('uncaughtException', onUncaught));

  for (let i = 0; i < 3; i += 1) {
    await new Promise((resolve) => {
      const socket = net.connect(ctx.port, '127.0.0.1', () => {
        socket.write(upgradeRequest(`/api/expert-relay?session=x&token=${encodeURIComponent(token)}`));
        setTimeout(() => { socket.resetAndDestroy(); resolve(); }, 30);
      });
      socket.on('error', () => {});
    });
  }
  await new Promise((r) => setTimeout(r, 400));
  assert.deepEqual(uncaught.map((e) => e.code ?? e.message), []);
  assert.equal((await call(ctx.port, 'GET', '/api/expert-demand', { token })).status, 200);
  assert.equal(await closeCodeFor(ctx.port, `/api/expert-relay?session=nobody&token=${encodeURIComponent(token)}`), 4404);
});

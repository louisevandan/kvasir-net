'use strict';
/**
 * The participation surface, end to end against a real server.
 *
 * These are the calls both phones make, in the order they make them, because
 * the order is load-bearing: the coverage POST is what creates the relay target
 * the worker then dials, and getting that backwards silently drops the node's
 * reward attribution.
 */
const test = require('node:test');
const assert = require('node:assert');
const crypto = require('node:crypto');
const http = require('node:http');
const net = require('node:net');

const { NodeAuth } = require('../nodeauth');
const { Participation } = require('../participation');
const wsrelay = require('../wsrelay');

const B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
function base58(buffer) {
  let n = BigInt('0x' + buffer.toString('hex'));
  let out = '';
  while (n > 0n) { out = B58[Number(n % 58n)] + out; n /= 58n; }
  for (const byte of buffer) { if (byte) break; out = `1${out}`; }
  return out;
}

/** A wallet, as the phone has one: a keypair whose address is its public key. */
function makeWallet() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const raw = publicKey.export({ format: 'der', type: 'spki' }).subarray(12);
  return {
    address: base58(raw),
    sign: (message) => crypto.sign(null, Buffer.from(message, 'utf8'), privateKey).toString('base64'),
  };
}

/** A wallet that has proved itself, reduced to the token the market wants. */
async function tokenFor(port) {
  const wallet = makeWallet();
  const challenge = await call(port, 'POST', '/api/auth/challenge', { body: { wallet: wallet.address } });
  const minted = await call(port, 'POST', '/api/auth/node-token', {
    body: {
      wallet: wallet.address,
      nonce: challenge.body.nonce,
      signature: wallet.sign(challenge.body.message),
    },
  });
  return minted.body.node_token;
}

// Layer 0 is dense on purpose: the real Step 3.7 has three leading dense
// blocks, and the market used to offer them as expert windows because it
// counted layers instead of reading which ones hold experts.
const MODEL = {
  id: 'step-3.7-flash', name: 'Step-3.7-Flash',
  nEmbd: 4096, nLayer: 4, nExpert: 8,
  expertLayers: [1, 2, 3],
};

function startBridge({ shard = null } = {}) {
  const credited = [];
  const participation = new Participation({
    auth: new NodeAuth({ secret: 'test-secret', serviceToken: 'svc-secret' }),
    credit: (nodeId, units, meta) => credited.push({ nodeId, units, ...meta }),
    models: () => [MODEL],
    shard,
  });
  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, 'http://test.local');
    let body = {};
    if (req.method === 'POST') {
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      body = JSON.parse(Buffer.concat(chunks).toString('utf8') || '{}');
    }
    if (await participation.handle(req, res, url.pathname, url.searchParams, body)) return;
    res.writeHead(404).end();
  });
  server.on('upgrade', (req, socket, head) => {
    const url = new URL(req.url, 'http://test.local');
    const target = participation.resolveUpgrade(url.pathname, url.searchParams);
    if (!target || target.code) {
      return wsrelay.refuse(req, socket, target?.code ?? 4404, target?.reason ?? 'unknown');
    }
    wsrelay.bridge(req, socket, head, target, {
      onBytes: (d, n) => participation.noteRelayBytes(target.session, d, n),
      onClose: () => participation.noteRelayClosed(target.session),
    });
  });
  return { server, participation, credited };
}

const listen = (server) => new Promise((resolve) => {
  server.listen(0, '127.0.0.1', () => resolve(server.address().port));
});

async function call(port, method, path, { body, token, raw = false } = {}) {
  const headers = { 'content-type': 'application/json' };
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fetch(`http://127.0.0.1:${port}${path}`, {
    method, headers, body: body ? JSON.stringify(body) : undefined,
  });
  const headersOut = Object.fromEntries(response.headers);
  // A shard is bytes, not JSON, and a test that parsed it would be asserting
  // about null rather than about the weights that came back.
  if (raw) {
    return { status: response.status, headers: headersOut,
      body: Buffer.from(await response.arrayBuffer()) };
  }
  return { status: response.status, headers: headersOut,
    body: await response.json().catch(() => null) };
}

test('a phone earns a node token from a wallet signature alone', async (t) => {
  const { server, participation } = startBridge();
  const port = await listen(server);
  t.after(() => { server.close(); participation.stop(); });
  const wallet = makeWallet();

  const challenge = await call(port, 'POST', '/api/auth/challenge', { body: { wallet: wallet.address } });
  assert.equal(challenge.status, 200);
  assert.match(challenge.body.nonce, /^[0-9a-f]{32}$/);
  assert.ok(challenge.body.message.includes(wallet.address));

  const token = await call(port, 'POST', '/api/auth/node-token', {
    body: {
      wallet: wallet.address,
      nonce: challenge.body.nonce,
      signature: wallet.sign(challenge.body.message),
    },
  });
  assert.equal(token.status, 200);
  assert.ok(token.body.node_token);
  assert.equal(token.body.wallet, wallet.address);
  assert.ok(token.body.expires_in > 0);
});

test('a challenge is single use and a foreign signature is refused', async (t) => {
  const { server, participation } = startBridge();
  const port = await listen(server);
  t.after(() => { server.close(); participation.stop(); });
  const wallet = makeWallet();
  const other = makeWallet();

  const challenge = await call(port, 'POST', '/api/auth/challenge', { body: { wallet: wallet.address } });
  const signature = wallet.sign(challenge.body.message);

  // Someone else's signature over the same message must not mint a token for
  // this wallet — otherwise anyone who can see a challenge can claim it.
  const impostor = await call(port, 'POST', '/api/auth/node-token', {
    body: { wallet: wallet.address, nonce: challenge.body.nonce, signature: other.sign(challenge.body.message) },
  });
  assert.equal(impostor.status, 401);
  assert.ok(impostor.body.error, 'the refusal must be in `error`, which is the only key the clients read');

  // And the nonce is spent even by that failed attempt, so the real signature
  // cannot be replayed behind it.
  const replay = await call(port, 'POST', '/api/auth/node-token', {
    body: { wallet: wallet.address, nonce: challenge.body.nonce, signature },
  });
  assert.equal(replay.status, 401);
});

test('a shard download is gated, relayed verbatim, and refused when there is no reader', async (t) => {
  // A stand-in for the reader beside the GGUF. It asserts what the bridge sends
  // it — the semantic parameters, and its own token, never the caller's.
  const seen = [];
  const reader = http.createServer((req, res) => {
    const url = new URL(req.url, 'http://reader.local');
    seen.push({
      path: url.pathname,
      query: Object.fromEntries(url.searchParams),
      token: req.headers['x-kvasir-service-token'],
    });
    if (url.searchParams.get('layer') === '0') {
      res.writeHead(400, { 'content-type': 'application/json' });
      return res.end(JSON.stringify({ error: 'layer 0 holds no routed experts' }));
    }
    const body = Buffer.from('EXPERTBYTES');
    res.writeHead(200, {
      'content-type': 'application/octet-stream',
      'content-length': String(body.length),
      'x-kvasir-shard-manifest-bytes': '7',
    });
    res.end(body);
  });
  const readerPort = await listen(reader);
  t.after(() => reader.close());

  const { server, participation } = startBridge({
    shard: { origin: `http://127.0.0.1:${readerPort}`, token: 'reader-secret' },
  });
  const port = await listen(server);
  t.after(() => { server.close(); participation.stop(); });
  const token = await tokenFor(port);
  const shard = `/api/proxy/models/step-3.7-flash/expert-shard?layer=3&expert_begin=0&expert_end=2`;

  // Open to the internet is exactly what this must not be.
  const anonymous = await call(port, 'GET', shard);
  assert.equal(anonymous.status, 401, 'a shard download without a node token');

  const ok = await call(port, 'GET', shard, { token, raw: true });
  assert.equal(ok.status, 200);
  assert.equal(ok.body.toString(), 'EXPERTBYTES', 'bytes must arrive unaltered');
  assert.equal(ok.headers['x-kvasir-shard-manifest-bytes'], '7', 'the manifest length must survive');
  assert.equal(seen.length, 1);
  assert.deepEqual(seen[0].query,
    { model: 'step-3.7-flash', layer: '3', expert_begin: '0', expert_end: '2' });
  assert.equal(seen[0].token, 'reader-secret', 'the bridge presents its own token, not the caller\'s');

  // The reader's refusal is more useful than anything the bridge could invent,
  // so it arrives with its status and its sentence intact.
  const dense = await call(port, 'GET',
    '/api/proxy/models/step-3.7-flash/expert-shard?layer=0&expert_begin=0&expert_end=2', { token });
  assert.equal(dense.status, 400);
  assert.match(dense.body.error, /no routed experts/);

  // A bridge with no model file beside it says so rather than hanging.
  const { server: bare, participation: bareP } = startBridge();
  const barePort = await listen(bare);
  t.after(() => { bare.close(); bareP.stop(); });
  const refused = await call(barePort, 'GET', shard, { token: await tokenFor(barePort) });
  assert.equal(refused.status, 503);
});

test('a layer with no experts is never offered, and an unread model offers nothing', async (t) => {
  // The bug this guards: nLayer was treated as "every layer has experts", so a
  // volunteer was handed layer 0 of a model whose first blocks are dense. The
  // window looked valid and had no tensors behind it; nothing would have caught
  // it until the device asked for a shard that does not exist.
  const { server, participation } = startBridge();
  const port = await listen(server);
  t.after(() => { server.close(); participation.stop(); });
  const token = await tokenFor(port);

  // Every window the market hands out, until coverage is satisfied, must be a
  // layer that actually holds experts.
  for (let i = 0; i < 12; i += 1) {
    const offer = await call(port, 'POST', '/api/expert-volunteer', { token, body: { max_experts: 8 } });
    assert.equal(offer.status, 200);
    if (!offer.body.assigned) break;
    assert.ok(MODEL.expertLayers.includes(offer.body.layer),
      `offered layer ${offer.body.layer}, which holds no experts`);
    await call(port, 'POST', '/api/expert-coverage', {
      token,
      body: {
        worker_id: `w-${i}`, model: MODEL.id, n_layer: MODEL.nLayer, n_expert: MODEL.nExpert,
        segments: [[offer.body.layer, offer.body.experts[0], offer.body.experts[1]]],
      },
    });
  }

  // A model whose topology was never read is offered nothing rather than
  // guessed at.
  const blind = new Participation({
    auth: new NodeAuth({ secret: 'test-secret', serviceToken: 'svc-secret' }),
    credit: () => {},
    models: () => [{ ...MODEL, expertLayers: undefined }],
  });
  assert.deepEqual(blind.coverage(MODEL.id), [], 'an unread model must expose no windows');
  blind.stop();
});

test('the market offers the scarcest window, and coverage shrinks it', async (t) => {
  const { server, participation } = startBridge();
  const port = await listen(server);
  t.after(() => { server.close(); participation.stop(); });
  const wallet = makeWallet();
  const challenge = await call(port, 'POST', '/api/auth/challenge', { body: { wallet: wallet.address } });
  const { body: { node_token: token } } = await call(port, 'POST', '/api/auth/node-token', {
    body: { wallet: wallet.address, nonce: challenge.body.nonce, signature: wallet.sign(challenge.body.message) },
  });

  const first = await call(port, 'POST', '/api/expert-volunteer', {
    token, body: { model: '', max_experts: 4 },
  });
  assert.equal(first.status, 200);
  assert.equal(first.body.assigned, true);
  assert.equal(first.body.n_embd, MODEL.nEmbd, 'a client aborts without n_embd');
  assert.equal(first.body.experts[1] - first.body.experts[0], 4, 'max_experts must clip the window');

  const claimed = await call(port, 'POST', '/api/expert-coverage', {
    token,
    body: {
      worker_id: 'phone-1',
      model: MODEL.id,
      n_layer: MODEL.nLayer,
      n_expert: MODEL.nExpert,
      segments: [[first.body.layer, first.body.experts[0], first.body.experts[1]]],
      url: 'relay:expert-phone-1',
    },
  });
  assert.equal(claimed.status, 200);
  assert.equal(claimed.body.wired, true, 'a relay: url must allocate a coordinator port');
  assert.equal(claimed.body.session, 'expert-phone-1');
  assert.ok(claimed.body.listen_port >= 52970);

  // The window this node took is now covered once, so the next volunteer must
  // be sent somewhere else rather than pile onto the same experts.
  const second = await call(port, 'POST', '/api/expert-volunteer', {
    token, body: { model: '', max_experts: 4 },
  });
  assert.equal(second.body.assigned, true);
  const overlaps = second.body.layer === first.body.layer
    && second.body.experts[0] < first.body.experts[1]
    && first.body.experts[0] < second.body.experts[1];
  assert.equal(overlaps, false, 'the market handed out the same window twice');
});

test('an unauthenticated market call is refused with a readable body', async (t) => {
  const { server, participation } = startBridge();
  const port = await listen(server);
  t.after(() => { server.close(); participation.stop(); });
  const refused = await call(port, 'POST', '/api/expert-volunteer', { body: {} });
  assert.equal(refused.status, 401);
  assert.equal(refused.body.error, 'authentication required');
});

test('the relay carries bytes both ways and credits the dialing wallet', async (t) => {
  const { server, participation, credited } = startBridge();
  const port = await listen(server);
  const wallet = makeWallet();
  const challenge = await call(port, 'POST', '/api/auth/challenge', { body: { wallet: wallet.address } });
  const { body: { node_token: token } } = await call(port, 'POST', '/api/auth/node-token', {
    body: { wallet: wallet.address, nonce: challenge.body.nonce, signature: wallet.sign(challenge.body.message) },
  });

  // Stand up the thing the relay dials — in production this is the coordinator
  // listening on the port the coverage POST allocated.
  const upstream = net.createServer((socket) => {
    socket.on('data', (chunk) => socket.write(Buffer.concat([Buffer.from('echo:'), chunk])));
  });
  const upstreamPort = await listen(upstream);
  t.after(() => { upstream.close(); server.close(); participation.stop(); });

  await call(port, 'POST', '/api/expert-coverage', {
    token,
    body: {
      worker_id: 'phone-2', model: MODEL.id, n_layer: MODEL.nLayer, n_expert: MODEL.nExpert,
      segments: [[0, 0, 2]], url: 'relay:expert-phone-2',
    },
  });
  // Point the session at our stand-in rather than the allocated port.
  participation.relayTargets.get('expert-phone-2').port = upstreamPort;

  const reply = await new Promise((resolve, reject) => {
    const key = crypto.randomBytes(16).toString('base64');
    const socket = net.connect(port, '127.0.0.1', () => {
      socket.write(
        `GET /api/expert-relay?session=expert-phone-2&token=${encodeURIComponent(token)} HTTP/1.1\r\n`
        + 'Host: test.local\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n'
        + `Sec-WebSocket-Key: ${key}\r\nSec-WebSocket-Version: 13\r\n\r\n`);
    });
    let handshake = '';
    let decoder = null;
    socket.on('data', (chunk) => {
      if (!decoder) {
        handshake += chunk.toString('latin1');
        const end = handshake.indexOf('\r\n\r\n');
        if (end < 0) return;
        assert.ok(handshake.startsWith('HTTP/1.1 101'), handshake.split('\r\n')[0]);
        decoder = new wsrelay.FrameDecoder({
          onData: (payload) => resolve(payload.toString('utf8')),
          onPing: () => {},
          onClose: () => reject(new Error('closed before any data')),
        });
        // The first byte of a ring dial is its role preamble; it must cross
        // untouched, so send one and expect it back through the echo.
        const masked = maskedFrame(Buffer.from('Phello'));
        socket.write(masked);
        const rest = Buffer.from(handshake.slice(end + 4), 'latin1');
        if (rest.length) decoder.push(rest);
        return;
      }
      decoder.push(chunk);
    });
    socket.on('error', reject);
    setTimeout(() => reject(new Error('relay timed out')), 5000).unref?.();
  });

  assert.equal(reply, 'echo:Phello', 'the role preamble must cross the relay untouched');

  participation.flushContributions();
  assert.ok(credited.length > 0, 'carrying bytes must credit someone');
  assert.equal(credited[0].owner, wallet.address, 'the dialing wallet is who gets paid');
});

test('a relay dial without a claimed session is refused, not dropped', async (t) => {
  const { server, participation } = startBridge();
  const port = await listen(server);
  t.after(() => { server.close(); participation.stop(); });
  const wallet = makeWallet();
  const challenge = await call(port, 'POST', '/api/auth/challenge', { body: { wallet: wallet.address } });
  const { body: { node_token: token } } = await call(port, 'POST', '/api/auth/node-token', {
    body: { wallet: wallet.address, nonce: challenge.body.nonce, signature: wallet.sign(challenge.body.message) },
  });

  const code = await closeCodeFor(port, `/api/expert-relay?session=nobody&token=${encodeURIComponent(token)}`);
  assert.equal(code, 4404, '4404 says "nothing claimed that session", which is diagnosed differently from 4401');

  const badToken = await closeCodeFor(port, '/api/expert-relay?session=nobody&token=rubbish');
  assert.equal(badToken, 4401);
});

/** Mask a client frame the way RFC 6455 requires of a client. */
function maskedFrame(payload) {
  const mask = crypto.randomBytes(4);
  const masked = Buffer.from(payload);
  for (let i = 0; i < masked.length; i += 1) masked[i] ^= mask[i & 3];
  const header = Buffer.alloc(2);
  header[0] = 0x82;                 // FIN + binary
  header[1] = 0x80 | masked.length; // MASK + length (test payloads stay small)
  return Buffer.concat([header, mask, masked]);
}

function closeCodeFor(port, path) {
  return new Promise((resolve, reject) => {
    const key = crypto.randomBytes(16).toString('base64');
    const socket = net.connect(port, '127.0.0.1', () => {
      socket.write(
        `GET ${path} HTTP/1.1\r\nHost: test.local\r\nUpgrade: websocket\r\n`
        + `Connection: Upgrade\r\nSec-WebSocket-Key: ${key}\r\nSec-WebSocket-Version: 13\r\n\r\n`);
    });
    let seen = Buffer.alloc(0);
    socket.on('data', (chunk) => {
      seen = Buffer.concat([seen, chunk]);
      const end = seen.indexOf('\r\n\r\n');
      if (end < 0) return;
      const frame = seen.subarray(end + 4);
      if (frame.length >= 4 && (frame[0] & 0x0f) === 0x8) {
        resolve(frame.readUInt16BE(2));
        socket.destroy();
      }
    });
    socket.on('error', reject);
    setTimeout(() => reject(new Error('no close frame')), 5000).unref?.();
  });
}

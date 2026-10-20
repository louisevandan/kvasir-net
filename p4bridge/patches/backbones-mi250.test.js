process.env.P4_BRIDGE_BACKBONES = JSON.stringify({"mi250-02": {model: "step-3.7-flash-mi250", port_base: 53970, port_span: 16, token_sha256: require("node:crypto").createHash("sha256").update("bb-secret").digest("hex")}});
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

function startBridge({ shard = null, eligible = null } = {}) {
  const credited = [];
  const participation = new Participation({
    auth: new NodeAuth({ secret: 'test-secret', serviceToken: 'svc-secret', eligible }),
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


async function list(port, token) {
  const r = await fetch(`http://127.0.0.1:${port}/api/expert-relay/sessions`, { headers: { 'x-kvasir-service-token': token } });
  return { status: r.status, body: await r.json().catch(() => null) };
}
test('a session claimed for a backbone gets its model, its port range, and only its token lists it', async (t) => {
  const { server, participation } = startBridge();
  const port = await listen(server);
  t.after(() => { server.close(); participation.stop(); });
  const token = await tokenFor(port);
  const claim = (worker, backbone) => call(port, 'POST', '/api/expert-coverage', { token, body: {
    worker_id: worker, model: MODEL.id, n_layer: MODEL.nLayer, n_expert: MODEL.nExpert,
    segments: [[1, 0, 4]], url: `relay:expert-${worker}`, ...(backbone ? { backbone } : {}) } });
  const a = await claim('gb', null);
  const b = await claim('mi', 'mi250-02');
  const c = await claim('xx', 'not-a-backbone');
  assert.ok(a.body.listen_port >= 52970 && a.body.listen_port < 53030);
  assert.ok(b.body.listen_port >= 53970 && b.body.listen_port < 53986);
  assert.ok(c.body.listen_port >= 52970 && c.body.listen_port < 53030, 'an unknown backbone is the default ring');
  const svc = await list(port, 'svc-secret');
  assert.equal(svc.status, 200);
  const byId = Object.fromEntries(svc.body.sessions.map((s) => [s.session, s]));
  assert.equal(byId['expert-gb'].model, 'step-3.7-flash');
  assert.equal(byId['expert-mi'].model, 'step-3.7-flash-mi250');
  assert.equal(byId['expert-mi'].backbone, 'mi250-02');
  const scoped = await list(port, 'bb-secret');
  assert.equal(scoped.status, 200);
  assert.deepEqual(scoped.body.sessions.map((s) => s.session), ['expert-mi']);
  assert.equal((await list(port, 'wrong')).status, 403);
  assert.equal((await list(port, token)).status, 403, 'a node token still cannot list');
  const back = await claim('mi', null);
  assert.ok(back.body.listen_port >= 52970 && back.body.listen_port < 53030, 'switching back moves the worker to the default range');
});

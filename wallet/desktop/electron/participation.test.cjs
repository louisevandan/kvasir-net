'use strict'
/**
 * Tests for the bridge participation client.
 *
 *   node electron/participation.test.cjs
 *
 * No framework: this project has none, and adding one to run a handful of
 * assertions would be a bigger change than the code under test. A real HTTP
 * server on loopback stands in for the bridge, so the transport, the headers
 * and the 401 retry are exercised for real rather than mocked away.
 */
const assert = require('node:assert')
const http = require('node:http')
const nacl = require('tweetnacl')
const { Participation, ParticipationError } = require('./participation.cjs')

let passed = 0
const results = []

async function test(name, fn) {
  try {
    await fn()
    passed++
    results.push(`  ok   ${name}`)
  } catch (e) {
    results.push(`  FAIL ${name}\n       ${e.message}`)
    process.exitCode = 1
  }
}

/** A stand-in bridge. `routes` maps "METHOD /path" to a handler. */
function fakeBridge(routes) {
  const seen = []
  const server = http.createServer((req, res) => {
    let body = ''
    req.on('data', (c) => { body += c })
    req.on('end', () => {
      const key = `${req.method} ${req.url}`
      seen.push({ key, headers: req.headers, body: body ? JSON.parse(body) : null })
      const handler = routes[key]
      if (!handler) { res.writeHead(404, { 'Content-Type': 'application/json' }); res.end('{"error":"no route"}'); return }
      const out = handler(body ? JSON.parse(body) : null, req)
      res.writeHead(out.status || 200, { 'Content-Type': 'application/json' })
      res.end(JSON.stringify(out.body == null ? {} : out.body))
    })
  })
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      resolve({
        base: `http://127.0.0.1:${server.address().port}`,
        seen,
        close: () => new Promise((r) => server.close(r)),
      })
    })
  })
}

const KEY = nacl.sign.keyPair()
const WALLET = 'CZpGzzYQcLA8iDacDcg3hf2z96iUWrdBRRxFkQq5XFGL'
const sign = (bytes) => nacl.sign.detached(Uint8Array.from(bytes), KEY.secretKey)

function client(base, extra = {}) {
  return new Participation({ baseUrl: base, wallet: () => WALLET, sign, ...extra })
}

;(async () => {
  await test('mints a token and signs the challenge message verbatim', async () => {
    const MESSAGE = 'Kvasir node login\nnonce: abc123\nissued: 2026-09-21'
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ body: { nonce: 'abc123', message: MESSAGE } }),
      'POST /api/auth/node-token': (b) => {
        // Verify exactly as the bridge would: the signature must check out
        // against the message bytes the bridge sent.
        const ok = nacl.sign.detached.verify(
          Uint8Array.from(Buffer.from(MESSAGE, 'utf8')),
          Uint8Array.from(Buffer.from(b.signature, 'base64')),
          KEY.publicKey,
        )
        assert.ok(ok, 'signature did not verify against the challenge message')
        assert.strictEqual(b.nonce, 'abc123')
        assert.strictEqual(b.wallet, WALLET)
        return { body: { node_token: 'nt_live', wallet: WALLET, expires_in: 2592000 } }
      },
    })
    try {
      const p = client(br.base)
      assert.strictEqual(await p.mintToken(), 'nt_live')
      assert.ok(p.tokenExpiresAt > Date.now(), 'expiry should be in the future')
    } finally { await br.close() }
  })

  await test('sends the token as a bearer header on market calls', async () => {
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ body: { nonce: 'n', message: 'm' } }),
      'POST /api/auth/node-token': () => ({ body: { node_token: 'nt_abc', expires_in: 100 } }),
      'POST /api/expert-volunteer': () => ({
        body: { assigned: true, model: 'step-3.7', layer: 4, experts: [0, 7], n_embd: 4096, n_layer: 61, n_expert: 256 },
      }),
    })
    try {
      const p = client(br.base)
      const a = await p.volunteer()
      assert.strictEqual(a.assigned, true)
      assert.strictEqual(a.n_embd, 4096)
      const call = br.seen.find((s) => s.key === 'POST /api/expert-volunteer')
      assert.strictEqual(call.headers.authorization, 'Bearer nt_abc')
    } finally { await br.close() }
  })

  await test('refuses an assignment with no n_embd instead of serving garbage', async () => {
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ body: { nonce: 'n', message: 'm' } }),
      'POST /api/auth/node-token': () => ({ body: { node_token: 't', expires_in: 100 } }),
      'POST /api/expert-volunteer': () => ({ body: { assigned: true, model: 'x', layer: 1, experts: [0, 1] } }),
    })
    try {
      const p = client(br.base)
      await assert.rejects(() => p.volunteer(), (e) => e.code === 'missing_n_embd')
      assert.strictEqual(p.assignment, null, 'a refused assignment must not be retained')
    } finally { await br.close() }
  })

  await test('reports "not assigned" without throwing', async () => {
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ body: { nonce: 'n', message: 'm' } }),
      'POST /api/auth/node-token': () => ({ body: { node_token: 't', expires_in: 100 } }),
      'POST /api/expert-volunteer': () => ({ body: { assigned: false, reason: 'expert coverage at target' } }),
    })
    try {
      const p = client(br.base)
      const r = await p.volunteer()
      assert.strictEqual(r.assigned, false)
      assert.match(r.reason, /coverage at target/)
    } finally { await br.close() }
  })

  await test('re-authenticates once when a stored token is rejected', async () => {
    let mints = 0
    let volunteerCalls = 0
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ body: { nonce: 'n', message: 'm' } }),
      'POST /api/auth/node-token': () => { mints++; return { body: { node_token: `fresh${mints}`, expires_in: 100 } } },
      'POST /api/expert-volunteer': (_b, req) => {
        volunteerCalls++
        // The stale token the store handed us is refused; the fresh one works.
        if (req.headers.authorization === 'Bearer stale') return { status: 401, body: { error: 'expired' } }
        return { body: { assigned: true, model: 'm', layer: 0, experts: [0, 1], n_embd: 512 } }
      },
    })
    try {
      const store = {
        saved: { token: 'stale', expiresAt: Date.now() + 3600_000 },
        load() { return this.saved },
        save(v) { this.saved = v },
      }
      const p = client(br.base, { store })
      const a = await p.volunteer()
      assert.strictEqual(a.assigned, true)
      assert.strictEqual(mints, 1, 'should mint exactly one replacement token')
      assert.strictEqual(volunteerCalls, 2, 'should retry the call once')
      assert.strictEqual(store.saved.token, 'fresh1', 'the new token must be persisted')
    } finally { await br.close() }
  })

  await test('does not retry forever on a persistent 401', async () => {
    let mints = 0
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ body: { nonce: 'n', message: 'm' } }),
      'POST /api/auth/node-token': () => { mints++; return { body: { node_token: 't', expires_in: 100 } } },
      'POST /api/expert-volunteer': () => ({ status: 401, body: { error: 'nope' } }),
    })
    try {
      const p = client(br.base)
      await assert.rejects(() => p.volunteer(), (e) => e.status === 401)
      assert.strictEqual(mints, 2, 'one initial mint plus one retry, then give up')
    } finally { await br.close() }
  })

  await test('reuses a cached token instead of re-signing every call', async () => {
    let mints = 0
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ body: { nonce: 'n', message: 'm' } }),
      'POST /api/auth/node-token': () => { mints++; return { body: { node_token: 't', expires_in: 3600 } } },
      'POST /api/expert-volunteer': () => ({ body: { assigned: false, reason: 'full' } }),
    })
    try {
      const p = client(br.base)
      await p.volunteer(); await p.volunteer(); await p.volunteer()
      assert.strictEqual(mints, 1, 'signing once should cover every later call')
    } finally { await br.close() }
  })

  await test('fails clearly when the wallet is locked', async () => {
    const br = await fakeBridge({})
    try {
      const p = new Participation({ baseUrl: br.base, wallet: () => '', sign })
      await assert.rejects(() => p.mintToken(), (e) => e.code === 'wallet_locked')
    } finally { await br.close() }
  })

  await test('rejects an over-long wallet before calling the bridge', async () => {
    const br = await fakeBridge({})
    try {
      const p = new Participation({ baseUrl: br.base, wallet: () => 'x'.repeat(45), sign })
      await assert.rejects(() => p.mintToken(), (e) => e.code === 'wallet_too_long')
      assert.strictEqual(br.seen.length, 0, 'must not reach the network')
    } finally { await br.close() }
  })

  await test('surfaces the bridge error text, not a bare status', async () => {
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ status: 401, body: { error: 'that challenge is unknown, expired, or already used' } }),
    })
    try {
      const p = client(br.base)
      await assert.rejects(() => p.mintToken(), (e) => /unknown, expired, or already used/.test(e.message))
    } finally { await br.close() }
  })

  await test('coverage segments describe the accepted assignment', async () => {
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ body: { nonce: 'n', message: 'm' } }),
      'POST /api/auth/node-token': () => ({ body: { node_token: 't', expires_in: 100 } }),
      'POST /api/expert-coverage': () => ({ body: { ok: true } }),
    })
    try {
      const p = client(br.base)
      const segs = p.segmentsFor({ model: 'step-3.7', layer: 4, experts: [8, 15] })
      assert.deepStrictEqual(segs, [{ model: 'step-3.7', layer: 4, expert_begin: 8, expert_end: 15 }])
      await p.reportCoverage(segs)
      const call = br.seen.find((s) => s.key === 'POST /api/expert-coverage')
      assert.deepStrictEqual(call.body.segments, segs)
      // Nothing held yet means nothing to report — and no pointless request.
      assert.deepStrictEqual(p.segmentsFor(null), [])
      assert.strictEqual(await p.reportCoverage([]), null)
    } finally { await br.close() }
  })

  console.log('\nbridge participation')
  console.log(results.join('\n'))
  console.log(`\n${passed}/${results.length} passed`)
})()

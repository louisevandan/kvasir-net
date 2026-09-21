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

  await test('coverage posts the bridge contract shape, not an object list', async () => {
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ body: { nonce: 'n', message: 'm' } }),
      'POST /api/auth/node-token': () => ({ body: { node_token: 't', expires_in: 100 } }),
      'POST /api/expert-coverage': (b) => {
        // Mirror p4bridge/participation.js:300-310 exactly: required top-level
        // fields, and segments kept only if they are 3-element arrays.
        if (!b.worker_id || !b.model) return { status: 400, body: { error: 'worker_id and model are required' } }
        const kept = (Array.isArray(b.segments) ? b.segments : [])
          .filter((x) => Array.isArray(x) && x.length === 3)
        return { body: { ok: true, kept: kept.length, workers: 1 } }
      },
    })
    try {
      const p = client(br.base)
      p.assignment = { model: 'step-3.7', layer: 4, experts: [8, 15], n_embd: 4096, n_layer: 61, n_expert: 256 }
      const r = await p.reportCoverage([[4, 8, 15]])
      assert.strictEqual(r.kept, 1, 'a [layer,begin,end] triple must survive the bridge filter')
      const call = br.seen.find((x) => x.key === 'POST /api/expert-coverage')
      assert.strictEqual(call.body.worker_id, 'desktop-CZpGzzYQ')
      assert.strictEqual(call.body.model, 'step-3.7')
      assert.strictEqual(call.body.n_layer, 61)
      assert.strictEqual(call.body.n_expert, 256)
      assert.strictEqual(call.body.owner, WALLET)
      // url is omitted until there is something to serve: relay wiring only
      // happens for a "relay:<session>" url, and a blank one must not wire.
      assert.ok(!('url' in call.body), 'url must be absent while nothing is served')
    } finally { await br.close() }
  })

  await test('holds nothing, so reports no segments but still joins the census', async () => {
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ body: { nonce: 'n', message: 'm' } }),
      'POST /api/auth/node-token': () => ({ body: { node_token: 't', expires_in: 100 } }),
      'POST /api/expert-coverage': () => ({ body: { ok: true, workers: 1 } }),
    })
    try {
      const p = client(br.base)
      // No shard download exists yet, so claiming a held segment would inflate
      // the bridge's replica count and stop it recruiting for a range nothing
      // can serve.
      assert.deepStrictEqual(p.heldSegments(), [])
      const r = await p.reportCoverage(p.heldSegments(), { model: 'step-3.7' })
      assert.strictEqual(r.workers, 1, 'an empty report still registers the worker')
      const call = br.seen.find((x) => x.key === 'POST /api/expert-coverage')
      assert.deepStrictEqual(call.body.segments, [])
    } finally { await br.close() }
  })

  await test('skips the post when it would be a guaranteed 400', async () => {
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ body: { nonce: 'n', message: 'm' } }),
      'POST /api/auth/node-token': () => ({ body: { node_token: 't', expires_in: 100 } }),
    })
    try {
      const p = client(br.base)
      // No assignment and no model: worker_id/model would be missing.
      assert.strictEqual(await p.reportCoverage([], {}), null)
      assert.ok(!br.seen.some((x) => x.key === 'POST /api/expert-coverage'))
    } finally { await br.close() }
  })

  await test('retries sooner after a failed tick than after a good one', async () => {
    // The bridge evicts a worker after 120s and reclaims its relay port with
    // it. Because the next tick is scheduled after the current one returns,
    // a poll interval close to that budget plus one slow tick drops the
    // machine out of the market — so a failure must not wait a full period.
    const br = await fakeBridge({
      'POST /api/auth/challenge': () => ({ status: 503, body: { error: 'bridge down' } }),
    })
    try {
      const p = client(br.base)
      const delays = []
      const realTimeout = global.setTimeout
      global.setTimeout = (fn, ms) => { delays.push(ms); return realTimeout(() => {}, 0) }
      try {
        p.running = true
        p.pollMs = 45_000
        await p.tick()
      } finally { global.setTimeout = realTimeout }
      p.running = false
      // The request's own abort timer also lands here; the reschedule is last.
      const next = delays[delays.length - 1]
      assert.ok(next < 45_000, `failed tick rescheduled in ${next}ms, expected sooner than the poll interval`)
    } finally { await br.close() }
  })

  await test('polls inside the bridge staleness budget', async () => {
    const WORKER_STALE_MS = 120_000 // p4bridge/participation.js:31
    const p = client('http://127.0.0.1:1')
    p.start({ pollMs: undefined })
    const used = p.pollMs
    p.stop()
    // Two consecutive intervals must still fit inside the eviction window,
    // leaving room for the tick's own duration on top.
    assert.ok(used * 2 < WORKER_STALE_MS,
      `poll ${used}ms: two in a row reach ${used * 2}ms against a ${WORKER_STALE_MS}ms budget`)
  })

  console.log('\nbridge participation')
  console.log(results.join('\n'))
  console.log(`\n${passed}/${results.length} passed`)
})()

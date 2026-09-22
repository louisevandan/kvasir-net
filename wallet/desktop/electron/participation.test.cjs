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

  // A host double: what expertHost.cjs exposes to the loop, and nothing else.
  function fakeHost({ canHost = true } = {}) {
    return {
      phase: 'idle', held: null, provisioned: [], released: [],
      canHost: () => canHost,
      busy() { return ['downloading', 'starting', 'serving'].includes(this.phase) },
      heldExperts() { return this.held ? this.held.end - this.held.begin : 0 },
      heldSegments() { return this.phase === 'serving' ? [[this.held.layer, this.held.begin, this.held.end]] : [] },
      coverageUrl() { return this.phase === 'serving' ? 'relay:expert-w' : '' },
      async provision(a) {
        this.provisioned.push(a)
        this.held = { model: a.model, layer: a.layer, begin: a.experts[0], end: a.experts[1] }
        this.phase = 'serving'
      },
      release(why) { this.released.push(why); this.phase = 'idle'; this.held = null },
      status() { return { phase: this.phase } },
    }
  }
  const marketRoutes = (volunteers) => ({
    'POST /api/auth/challenge': () => ({ body: { nonce: 'n', message: 'm' } }),
    'POST /api/auth/node-token': () => ({ body: { node_token: 'tok', expires_in: 2592000 } }),
    'POST /api/expert-volunteer': (b) => {
      volunteers.push(b)
      return { body: { assigned: true, model: 'step', layer: 5, experts: [0, 16], n_embd: 4096, n_layer: 45, n_expert: 288 } }
    },
    'POST /api/expert-coverage': () => ({ body: { ok: true, wired: true } }),
  })

  await test('once serving, reports the held segment with a relay url and stops volunteering', async () => {
    const volunteers = []
    const br = await fakeBridge(marketRoutes(volunteers))
    try {
      const host = fakeHost()
      const p = client(br.base, { host })
      p.running = true; p.maxExpertsFn = () => 64
      await p.tick()
      await new Promise((r) => setImmediate(r))
      assert.strictEqual(host.provisioned.length, 1)
      p.running = true
      await p.tick()
      p.stop()
      assert.strictEqual(volunteers.length, 1, 'volunteered again while holding a segment')
      const posts = br.seen.filter((x) => x.key === 'POST /api/expert-coverage').map((x) => x.body)
      const last = posts[posts.length - 1]
      assert.deepStrictEqual(last.segments, [[5, 0, 16]])
      assert.strictEqual(last.url, 'relay:expert-w')
      assert.deepStrictEqual(host.released, ['node stopped'])
    } finally { await br.close() }
  })

  await test('a lowered VRAM budget releases what no longer fits', async () => {
    const volunteers = []
    const br = await fakeBridge(marketRoutes(volunteers))
    try {
      const host = fakeHost()
      host.phase = 'serving'; host.held = { model: 'step', layer: 5, begin: 0, end: 16 }
      const p = client(br.base, { host })
      p.running = true; p.maxExpertsFn = () => 8
      await p.tick()
      p.running = false
      assert.deepStrictEqual(host.released, ['the VRAM budget no longer fits it'])
      // Whatever it posts afterwards, it no longer claims the range.
      for (const x of br.seen.filter((y) => y.key === 'POST /api/expert-coverage')) {
        assert.deepStrictEqual(x.body.segments, [])
      }
    } finally { await br.close() }
  })

  await test('a machine with no worker does not volunteer for work it cannot do', async () => {
    const volunteers = []
    const br = await fakeBridge(marketRoutes(volunteers))
    try {
      const p = client(br.base, { host: fakeHost({ canHost: false }) })
      p.running = true; p.maxExpertsFn = () => 64
      await p.tick()
      p.running = false
      assert.strictEqual(volunteers.length, 0)
      assert.strictEqual(p.assignment, null)
    } finally { await br.close() }
  })

  // A pool double with the ExpertPool surface the loop uses.
  function fakePool({ capacitySlots = 3, window = 64 } = {}) {
    const slots = []
    return {
      slots, provisioned: [], trimmed: 0,
      canHost: () => true,
      busySlots() { return slots.filter((s) => s.phase === 'serving') },
      heldExperts() { return slots.reduce((a, s) => a + (s.end - s.begin), 0) },
      nextWindow() { return slots.length < capacitySlots ? window : 0 },
      trim() { this.trimmed++ },
      async provision(a) {
        this.provisioned.push(a)
        slots.push({ phase: 'serving', layer: a.layer, begin: a.experts[0], end: a.experts[1] })
      },
      reports() {
        const out = [{ workerId: 'w', segments: [], url: '' }]
        slots.forEach((s, i) => {
          const r = { workerId: i === 0 ? 'w' : `w-${i + 1}`, segments: [[s.layer, s.begin, s.end]], url: `relay:expert-${i === 0 ? 'w' : `w-${i + 1}`}` }
          if (i === 0) out[0] = r
          else out.push(r)
        })
        return out
      },
      release() { slots.length = 0 },
      status() { return { phase: slots.length ? 'serving' : 'idle' } },
    }
  }

  await test('a pool grows one slot per tick and reports each slot under its own id', async () => {
    const volunteers = []
    const br = await fakeBridge(marketRoutes(volunteers))
    try {
      const pool = fakePool({ capacitySlots: 2, window: 40 })
      const p = client(br.base, { host: pool })
      p.pollMs = 45_000
      // Ticks run one at a time here; poke()'s early tick is the loop's own
      // business and would interleave with the ones this test drives.
      p.poke = () => {}
      for (let i = 0; i < 4; i++) {
        p.running = true
        await p.tick()
        await new Promise((r) => setImmediate(r))
        if (p.timer) { clearTimeout(p.timer); p.timer = null }
      }
      p.running = false
      // Capacity for two slots: two volunteers, each sized by nextWindow, then none.
      assert.strictEqual(volunteers.length, 2)
      assert.deepStrictEqual(volunteers.map((v) => v.max_experts), [40, 40])
      assert.strictEqual(pool.provisioned.length, 2)
      assert.ok(pool.trimmed >= 4, 'trim runs every tick')
      const last = br.seen.filter((x) => x.key === 'POST /api/expert-coverage').slice(-2).map((x) => x.body)
      assert.deepStrictEqual(last.map((b) => b.worker_id), ['w', 'w-2'])
      assert.deepStrictEqual(last.map((b) => b.url), ['relay:expert-w', 'relay:expert-w-2'])
      assert.ok(last.every((b) => b.owner === WALLET), 'every slot pays the same wallet')
      assert.ok(last.every((b) => b.model === 'step'))
    } finally { await br.close() }
  })

  await test('a poke during a tick runs after it, never alongside', async () => {
    const volunteers = []
    let release
    const gate = new Promise((r) => { release = r })
    const routes = marketRoutes(volunteers)
    const slow = routes['POST /api/expert-volunteer']
    let inFlight = 0
    let overlapped = false
    routes['POST /api/expert-volunteer'] = (b) => {
      inFlight++
      if (inFlight > 1) overlapped = true
      inFlight--
      return slow(b)
    }
    const br = await fakeBridge(routes)
    try {
      const p = client(br.base)
      p.running = true; p.maxExpertsFn = () => 64; p.pollMs = 45_000
      const first = p.tick()
      p.poke()              // lands while the first tick is still awaiting the bridge
      await first
      await new Promise((r) => setTimeout(r, 50))
      p.stop()
      assert.strictEqual(overlapped, false)
      assert.strictEqual(volunteers.length, 2, 'the poke still ran, after the first tick')
      release()
    } finally { await br.close() }
  })

  console.log('\nbridge participation')
  console.log(results.join('\n'))
  console.log(`\n${passed}/${results.length} passed`)
})()

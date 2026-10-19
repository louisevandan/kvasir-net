'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { liveRing, observeLiveRing, createLiveRingCache } = require('./liveRing.cjs')
const now = 100000
const agent = { running: true, snapshotAt: now, address: 'tcp://127.0.0.1:42031', nodes: [{ nodeId: 'stage-1' }] }
const row = { node_id: 'stage-1', agent: agent.address, perf_tps: 28.3, model: 'step-3.7-flash', last_work_at_ms: now - 1000 }
const run = (rows, patch = {}) => liveRing({ agent, now, contributions: rows, ...patch })
test('owned live stage exposes achieved request rate, without summing pipeline stages', () => {
  const r = run([row]); assert.equal(r.state, 'live'); assert.equal(r.stages[0].tps, 28.3)
  assert.equal(r.metric, 'achieved request throughput'); assert.equal(r.lastWorkAt, now - 1000)
})
test('source-shape legacy feed is unavailable, never mislabeled fresh', () => {
  const r = run([{ node_id: row.node_id, agent: row.agent, rows: 100, requests: 2, perf_tps: 28.3 }])
  assert.equal(r.state, 'unavailable'); assert.equal(r.stages[0].tps, null)
})
test('idle no-stage is first class; expert-only traffic is never tok/s', () => {
  const a = { ...agent, nodes: [] }
  assert.equal(run([], { agent: a }).state, 'idle')
  const r = run([], { agent: a, experts: { servingSlots: 1, slots: [{ relay: { bytes: 80000 } }] } })
  assert.equal(r.state, 'unavailable'); assert.equal(r.relayBytes, 80000); assert.deepEqual(r.stages, [])
})
test('other agent, other node, duplicate and missing matches fail closed', () => {
  for (const rows of [[{ ...row, agent: 'other' }], [{ ...row, node_id: 'other' }], [row, row], []]) {
    assert.equal(run(rows).stages[0].tps, null)
  }
})
test('stale work and stale agent inspection suppress rates', () => {
  assert.equal(run([{ ...row, last_work_at_ms: 1 }]).state, 'stale')
  assert.equal(run([row], { agent: { ...agent, snapshotAt: 1 } }).state, 'stale')
  assert.equal(run([row], { agent: { ...agent, lastError: 'inspect timeout' } }).state, 'stale')
})
test('invalid timestamps and rates cannot produce live throughput', () => {
  for (const at of [undefined, 0, NaN, now + 1]) assert.equal(run([{ ...row, last_work_at_ms: at }]).stages[0].tps, null)
  for (const rate of [0, -1, '28', Infinity]) assert.equal(run([{ ...row, perf_tps: rate }]).stages[0].tps, null)
})
test('no stages performs no HTTP request', async () => {
  const r = await observeLiveRing({ ...agent, snapshotAt: Date.now(), nodes: [] }, {}, 'https://example.com', () => { throw Error('must not request') })
  assert.equal(r.state, 'idle')
})
test('auth-only feed is unavailable and requests contain no credentials', async () => {
  const r = await observeLiveRing({ ...agent, snapshotAt: Date.now() }, {}, 'https://example.com', async (url, opts) => {
    assert.equal(url, 'https://example.com/api/contributions'); assert.equal(opts.credentials, 'omit')
    assert.equal(opts.headers, undefined); assert.equal(opts.redirect, 'error')
    return { ok: false, status: 401 }
  })
  assert.equal(r.state, 'unavailable'); assert.match(r.reason, /401/)
})
const flush = () => new Promise(resolve => setImmediate(resolve))
test('cache returns synchronously, coalesces pending polls and refreshes at most every 10s', async () => {
  let clock = now, calls = 0, resolve
  const cache = createLiveRingCache({ now: () => clock, observe: () => {
    calls++; return new Promise(done => { resolve = done })
  } })
  const poll = () => cache({ ...agent, snapshotAt: clock }, {}, 'https://example.com')
  const initial = poll()
  assert.equal(initial.then, undefined); assert.equal(initial.state, 'unavailable')
  await flush(); assert.equal(calls, 1)
  clock += 10000; poll(); await flush(); assert.equal(calls, 1)
  resolve(run([row])); await flush()
  assert.equal(poll().stages[0].tps, 28.3)
  await flush(); assert.equal(calls, 2)
  resolve(run([row])); await flush()
  clock += 9999; poll(); await flush(); assert.equal(calls, 2)
  clock++; poll(); await flush(); assert.equal(calls, 3)
  resolve(run([row])); await flush()
})
test('cache ages work without refreshing observedAt and clears stale agent data', async () => {
  let clock = now
  const cache = createLiveRingCache({ now: () => clock, observe: async () => run([row]) })
  const poll = a => cache(a || { ...agent, snapshotAt: clock }, { slots: [{ relay: { bytes: 123 } }] }, 'https://example.com')
  poll(); await flush()
  assert.equal(poll().state, 'live')
  clock += 60001
  const aged = poll()
  assert.equal(aged.state, 'stale'); assert.equal(aged.stages[0].tps, null)
  assert.equal(aged.observedAt, now); assert.equal(aged.relayBytes, 123)
  assert.equal(poll({ ...agent, lastError: 'failed' }).state, 'stale')
  await flush()
  assert.equal(poll().state, 'unavailable')
})
test('source, address, stage generation, and stop changes discard old in-flight observations', async () => {
  for (const change of ['source', 'address', 'generation', 'stop']) {
    let clock = now, resolve, calls = 0
    const cache = createLiveRingCache({ now: () => clock, observe: () => {
      calls++; return new Promise(done => { resolve = done })
    } })
    cache(agent, {}, 'https://example.com'); await flush()
    const nextAgent = { ...agent, ...(change === 'address' ? { address: 'other' } : {}),
      ...(change === 'generation' ? { nodes: [{ nodeId: 'stage-1', generation: 2 }] } : {}),
      ...(change === 'stop' ? { running: false } : {}) }
    const nextUrl = change === 'source' ? 'https://other.example.com' : 'https://example.com'
    assert.notEqual(cache(nextAgent, {}, nextUrl).state, 'live')
    resolve(run([row])); await flush()
    assert.notEqual(cache(nextAgent, {}, nextUrl).state, 'live')
    assert.equal(calls, 1)
    clock += 10000
    cache({ ...nextAgent, snapshotAt: clock }, {}, nextUrl); await flush()
    assert.equal(calls, change === 'stop' ? 1 : 2)
    resolve(run([row])); await flush()
  }
})
test('cache absorbs failed refreshes, throttles retries, and handles no-stage locally', async () => {
  let clock = now, calls = 0
  const cache = createLiveRingCache({ now: () => clock, observe: () => { calls++; throw Error('offline') } })
  const poll = a => cache(a || { ...agent, snapshotAt: clock }, {}, 'https://example.com')
  assert.equal(poll({ ...agent, nodes: [] }).state, 'idle')
  await flush(); assert.equal(calls, 0)
  poll(); await flush()
  assert.equal(poll().reasonCode, 'feedUnavailable'); assert.equal(calls, 1)
  clock += 9999; poll(); await flush(); assert.equal(calls, 1)
  clock++; poll(); await flush(); assert.equal(calls, 2)
})
test('reason codes retain diagnostic compatibility and expose HTTP status separately', async () => {
  assert.equal(run([], { agent: { ...agent, running: false } }).reasonCode, 'agentStale')
  assert.equal(run([], { agent: { ...agent, nodes: [] } }).reasonCode, 'noStage')
  assert.equal(run([], { agent: { ...agent, nodes: [] }, experts: { servingSlots: 1 } }).reasonCode, 'expertOnly')
  assert.equal(run(undefined).reasonCode, 'feedUnavailable')
  assert.equal(run([]).stages[0].reasonCode, 'noMatch')
  assert.equal(run([{ ...row, model: null }]).stages[0].reasonCode, 'missingMetadata')
  const r = await observeLiveRing({ ...agent, snapshotAt: Date.now() }, {}, 'https://example.com', async () => ({ ok: false, status: 403 }))
  assert.equal(r.reasonCode, 'feedHttp'); assert.equal(r.httpStatus, 403); assert.match(r.reason, /403/)
})

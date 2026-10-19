'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { liveRing, observeLiveRing } = require('./liveRing.cjs')
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

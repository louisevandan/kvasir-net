'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { ExpertPool } = require('./expertHost.cjs')
function poolOf(count = 3) {
  const pool = new ExpertPool({ workerId: () => 'fixture' })
  pool.slots = Array.from({ length: count }, (_, i) => ({ phase: 'serving', proc: {}, generation: 0, port: 8000 + i,
    held: { model: 'm', layer: 6 + i }, busy: () => true, heldExperts: () => 64,
    status() { return { phase: this.phase } },
    async benchmark(current) {
      assert.ok(current())
      return { ok: true, expertBenchmark: { rowsPerSecond: 100 + i, p95Ms: 10 + i, layer: 6 + i } }
    } }))
  return pool
}
test('measures every serving slot sequentially, publishes progress and sums rates / worst p95', async () => {
  const pool = poolOf(), order = []
  let active = 0
  for (const slot of pool.slots) {
    const bench = slot.benchmark
    slot.benchmark = async current => {
      assert.equal(++active, 1)
      order.push([pool.measurement.layer, pool.measurement.index, pool.measurement.count])
      await new Promise(resolve => setImmediate(resolve)); active--
      const result = await bench(current); slot.localBenchmark = result.expertBenchmark; return result
    }
  }
  pool.slots.push({ phase: 'downloading' })
  const pending = pool.benchmark()
  assert.match((await pool.benchmark()).error, /already running/)
  const result = await pending
  assert.equal(result.ok, true)
  assert.deepEqual(order, [[6, 1, 3], [7, 2, 3], [8, 3, 3]])
  assert.equal(result.expertBenchmarks.length, 3)
  assert.equal(result.expertBenchmarkTotal.rowsPerSecond, 303)
  assert.equal(result.expertBenchmarkTotal.p95Ms, 12)
  assert.equal(pool.measurement.running, false)
})
test('reassignment of any target aborts run and removes aggregate, errors release guard', async () => {
  const pool = poolOf()
  let calls = 0
  pool.slots[0].benchmark = async current => {
    calls++; pool.slots[2].generation++
    assert.equal(current(), false)
    return { ok: true, expertBenchmark: { rowsPerSecond: 1, p95Ms: 1 } }
  }
  assert.match((await pool.benchmark()).error, /assignment changed/)
  assert.equal(calls, 1); assert.equal(pool.measurement.running, false); assert.equal(pool.measurement.total, null)
  pool.slots[0].benchmark = async () => { throw Error('worker failed') }
  assert.match((await pool.benchmark()).error, /worker failed/)
  assert.equal(pool.measurement.running, false)
})
test('completed totals are invalidated when held ownership changes', async () => {
  const pool = poolOf()
  await pool.benchmark()
  assert.ok(pool.status().measurement.total)
  pool.slots[0].generation++
  assert.equal(pool.status().measurement.total, null)
})

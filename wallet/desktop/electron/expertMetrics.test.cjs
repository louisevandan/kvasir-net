'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const net = require('node:net')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { ExpertMetrics, ExpertFrames, benchmarkExpert } = require('./expertMetrics.cjs')
const { ExpertHost } = require('./expertHost.cjs')

function request(mode = 2, rows = 3, nEmbd = 4, pairs = 5) {
  const width = mode === -3 ? 2 : 4
  const size = (mode < 0 ? 12 : 8) + nEmbd * rows * width + (mode < 0 ? pairs * (8 + width) : mode * rows * 4)
  const data = Buffer.alloc(size)
  data.writeInt32LE(mode); data.writeInt32LE(rows, 4)
  if (mode < 0) data.writeInt32LE(pairs, 8)
  return data
}
for (const mode of [2, -2, -3]) test(`fragments/coalesces requests and responses, counts rows not pairs: ${mode}`, () => {
  let clock = 0
  const metrics = new ExpertMetrics({ now: () => clock })
  const frames = new ExpertFrames(4, metrics, () => clock)
  const data = request(mode)
  frames.request(data.subarray(0, 3)); clock = 10
  frames.request(Buffer.concat([data.subarray(3), data]))
  const out = Buffer.alloc(4 * (mode === -3 ? 2 : 4) * (mode < 0 ? 3 : 6))
  frames.response(out.subarray(0, 1)); assert.equal(metrics.status().calls, 0)
  clock = 20; frames.response(Buffer.concat([out.subarray(1), out]))
  const r = metrics.status()
  assert.equal(r.calls, 2); assert.equal(r.rows, 6); assert.equal(r.p50Ms, 10); assert.equal(r.p95Ms, 20)
  assert.equal(r.lastWorkAt, 20); assert.equal(r.rowsPerSecond, 300)
  clock = 60021; assert.equal(metrics.status().state, 'idle'); assert.equal(metrics.status().rowsPerSecond, 0)
})
test('unknown framing, incomplete responses and abandoned connections never count calls', () => {
  const metrics = new ExpertMetrics()
  for (const mode of [0, -1, 65]) {
    const frames = new ExpertFrames(4, metrics); const data = Buffer.alloc(8); data.writeInt32LE(mode); data.writeInt32LE(1, 4)
    frames.request(data); frames.response(Buffer.alloc(16)); assert.equal(frames.disabled, true)
  }
  const frames = new ExpertFrames(4, metrics)
  frames.request(request()); frames.response(Buffer.alloc(1))
  assert.equal(metrics.status().calls, 0)
  const fresh = new ExpertFrames(4, metrics); fresh.response(Buffer.alloc(95))
  assert.equal(metrics.status().calls, 0)
})
test('observer bounds pending metadata and rejects hostile row/pair lengths', () => {
  const metrics = new ExpertMetrics()
  const frames = new ExpertFrames(4, metrics)
  for (let i = 0; i < 129; i++) frames.request(request())
  assert.equal(frames.disabled, true); assert.equal(frames.pending.length, 0)
  for (const [rows, pairs] of [[0, 1], [8193, 1], [1, 65537], [1, -1]]) {
    const parser = new ExpertFrames(4, metrics); const head = Buffer.alloc(12)
    head.writeInt32LE(-2); head.writeInt32LE(rows, 4); head.writeInt32LE(pairs, 8)
    parser.request(head); assert.equal(parser.disabled, true)
  }
})
async function withWorker(handler, run) {
  const sockets = new Set()
  const server = net.createServer(socket => { sockets.add(socket); socket.on('close', () => sockets.delete(socket)); handler(socket) })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  try { return await run(server.address().port) }
  finally { for (const socket of sockets) socket.destroy(); await new Promise(resolve => server.close(resolve)) }
}
test('loopback synthetic benchmark sends correct hidden width and local IDs, excludes warmup', async () => {
  let calls = 0
  await withWorker(socket => {
    socket.on('data', data => {
      calls++
      assert.equal(data.readInt32LE(0), 3); assert.equal(data.readInt32LE(4), 1)
      assert.equal(data.length, 8 + 4 * 4 + 3 * 4)
      assert.deepEqual([0, 1, 2].map(i => data.readInt32LE(24 + i * 4)), [0, 1, 2])
      socket.write(Buffer.alloc(10)); socket.write(Buffer.alloc(38))
    })
  }, async port => {
    const r = await benchmarkExpert({ port, nEmbd: 4, experts: 3, isCurrent: () => true })
    assert.equal(r.kind, 'synthetic-expert-ffn'); assert.equal(calls, 31); assert.equal(r.calls, 30)
    assert.equal(r.rows, 30); assert.ok(r.rowsPerSecond > 0); assert.ok(r.p95Ms >= r.p50Ms)
  })
})
test('benchmark rejects a stalled worker and changed assignment', async () => {
  await withWorker(() => {}, async port => {
    await assert.rejects(benchmarkExpert({ port, nEmbd: 4, experts: 2, isCurrent: () => true, timeoutMs: 20 }), /timed out/)
  })
  let current = true
  await withWorker(socket => socket.on('data', () => { current = false; socket.write(Buffer.alloc(32)) }), async port => {
    await assert.rejects(benchmarkExpert({ port, nEmbd: 4, experts: 2, isCurrent: () => current }), /assignment changed/)
  })
})
test('default slot deadline is two seconds; reassignment cancels even a silent worker', async () => {
  await withWorker(() => {}, async port => {
    const start = Date.now()
    await assert.rejects(benchmarkExpert({ port, nEmbd: 4, experts: 2, isCurrent: () => true }), /timed out/)
    assert.ok(Date.now() - start < 3000)
    let current = true
    const pending = benchmarkExpert({ port, nEmbd: 4, experts: 2, isCurrent: () => current })
    setTimeout(() => { current = false }, 40)
    const cancelStart = Date.now()
    await assert.rejects(pending, /assignment changed/)
    assert.ok(Date.now() - cancelStart < 1000)
  })
})
test('host concurrency guard prevents a second benchmark and never adds synthetic work to live metrics', async () => {
  await withWorker(socket => socket.on('data', () => setImmediate(() => socket.write(Buffer.alloc(32)))), async port => {
    const host = new ExpertHost({ baseUrl: 'https://example.com', token: async () => '', workerBinary: () => null, shardDir: '/unused' })
    host.proc = {}; host.port = port; host.phase = 'serving'; host.held = { model: 'm', layer: 3, begin: 0, end: 2, nEmbd: 4 }
    const pending = host.benchmark()
    assert.match((await host.benchmark()).error, /already running/)
    const result = await pending
    assert.equal(result.ok, true); assert.equal(host.status().work.calls, 0)
    assert.equal(host.status().localBenchmark.model, 'm'); assert.equal(host.status().localBenchmark.layer, 3)
  })
})
test('benchmark rejects non-finite worker output even when the reply length matches', async () => {
  await withWorker(socket => socket.on('data', () => {
    const out = Buffer.alloc(32); out.writeFloatLE(NaN, 0)
    socket.write(out.subarray(0, 2)); socket.write(out.subarray(2))
  }), async port => {
    await assert.rejects(benchmarkExpert({ port, nEmbd: 4, experts: 2, isCurrent: () => true }), /non-finite/)
  })
})
test('late relay callbacks cannot change a new assignment or open its worker', async () => {
  const { EventEmitter } = require('node:events')
  let ws
  class FakeSocket extends EventEmitter { constructor() { super(); ws = this } terminate() { this.terminated = true } }
  const host = new ExpertHost({ baseUrl: 'https://example.com', token: async () => '', workerBinary: () => null, shardDir: '/unused', WebSocket: FakeSocket })
  host.phase = 'serving'; host.held = { nEmbd: 4 }; host.session = 'fixture'
  await host.dialRelay(host.generation)
  host.generation++
  host.relay = { ws: null, connected: false, bytes: 0, lastClose: null }
  ws.emit('open'); ws.emit('message', Buffer.alloc(8)); ws.emit('close', 1011, 'old')
  assert.equal(ws.terminated, true); assert.equal(host.relay.bytes, 0)
  assert.equal(host.relay.lastClose, null); assert.equal(host.relay.connected, false)
})
test('Measure selects serving experts before GGUF and falls back only when a GGUF exists', async () => {
  const source = fs.readFileSync(path.join(__dirname, 'main.cjs'), 'utf8')
  const start = source.indexOf('async function measureNode(')
  const context = vm.createContext({ expertPool: { slots: [] }, listModels: () => [], benchmark: async () => ({ ok: true, tps: 12 }) })
  vm.runInContext(source.slice(start, source.indexOf('\n}', start) + 2), context)
  assert.equal((await context.measureNode()).errorCode, 'noMeasurementTarget')
  context.listModels = () => [{ name: 'm.gguf' }]
  assert.equal((await context.measureNode()).tps, 12)
  context.expertPool.slots = [{ phase: 'serving', proc: {} }]
  context.expertPool.benchmark = async () => ({ ok: true, expertBenchmark: {} })
  assert.ok((await context.measureNode()).expertBenchmark)
})

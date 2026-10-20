'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { EventEmitter } = require('node:events')
const { LocalServer } = require('./localServer.cjs')

// Exercise the actual main-process functions without loading wallet credentials,
// starting Electron, or contacting the settlement service.
const source = fs.readFileSync(process.env.BENCHMARK_MAIN_SOURCE || path.join(__dirname, 'main.cjs'), 'utf8')
function fn(name) {
  const start = source.indexOf(`async function ${name}(`)
  assert.ok(start >= 0)
  return source.slice(start, source.indexOf('\n}', start) + 2)
}
function harness({ timings, httpOk = true, usage = 64 } = {}) {
  let now = 0
  const context = vm.createContext({
    path, fs: { existsSync: () => true }, process: { env: {} },
    llamaServerBin: () => '/runtime/llama-server', modelsDir: () => '/models',
    listModels: () => [{ name: 'small.gguf', sizeBytes: 10 }],
    measured: null, localProc: null,
    spawn: () => new EventEmitter(),
    Date: { now: () => now },
    setTimeout: (cb, ms) => { now += ms; cb() },
    fetch: async (url) => {
      if (url.endsWith('/health')) return { ok: true }
      if (url.endsWith('/props')) return { ok: true, json: async () => ({ model_path: path.join('/models', 'small.gguf') }) } // llama-server echoes -m, which main.cjs builds with path.join
      if (url.endsWith('/v1/models')) return { ok: true, json: async () => ({ data: [{ id: context.localServer.current.alias }] }) }
      now += 9000 // prompt/queue/decode, separate from the health wait
      return { ok: httpOk, status: httpOk ? 200 : 500, json: async () => ({
        choices: [{ message: { content: 'test' } }], usage: { completion_tokens: usage }, timings,
      }) }
    },
  })
  context.localServer = new LocalServer({ allocatePort: async () => 49152,
    spawnProcess: () => Object.assign(new EventEmitter(), { kill() {} }),
    sleep: async ms => { now += ms }, request: context.fetch })
  vm.runInContext(`${fn('localGenerate')}\n${fn('benchmark')}`, context)
  return context
}
const timing = { predicted_per_second: 80, predicted_n: 64, predicted_ms: 800, prompt_ms: 1200 }

test('benchmark uses server decode throughput, not health/prompt/wall time', async () => {
  const h = harness({ timings: timing })
  const r = await h.benchmark(null)
  assert.equal(r.ok, true)
  assert.equal(r.tps, 80) // old implementation reports 6.4
  assert.equal(r.tokens, 64)
  assert.equal(r.decodeMs, 800)
  assert.equal(r.elapsedMs, 10000) // preserve the existing wall-time field
  assert.equal(r.promptMs, 1200)
})
test('timings survive the completion response mapping', async () => {
  const r = await harness({ timings: timing }).localGenerate(null, 'small.gguf', 'test', 64)
  assert.deepEqual(r.timings, timing)
})
test('derive decode rate from predicted_n / predicted_ms if rate is absent', async () => {
  const r = await harness({ timings: { predicted_n: 40, predicted_ms: 500 }, usage: 999 }).benchmark(null)
  assert.equal(r.tps, 80)
  assert.equal(r.tokens, 40)
  assert.equal(r.promptMs, null)
})
for (const timings of [undefined, {}, { ...timing, predicted_n: 0 },
  { ...timing, predicted_ms: 0 }, { ...timing, predicted_ms: -1 },
  { ...timing, predicted_n: '64' }, { ...timing, predicted_ms: Infinity }]) {
  test(`invalid/missing timings fail closed: ${JSON.stringify(timings)}`, async () => {
    const h = harness({ timings })
    const r = await h.benchmark(null)
    assert.equal(r.ok, false)
    assert.match(r.error, /decode timings/)
    assert.equal(h.measured, null)
  })
}
for (const rate of [0, -1, NaN, Infinity, '80']) {
  test(`invalid rate ${rate} falls back to server decode count/time`, async () => {
    const r = await harness({ timings: { ...timing, predicted_per_second: rate } }).benchmark(null)
    assert.equal(r.tps, 80)
  })
}
test('HTTP failures are not successful measurements', async () => {
  const h = harness({ timings: timing, httpOk: false })
  const r = await h.benchmark(null)
  assert.equal(r.ok, false)
  assert.equal(h.measured, null)
})

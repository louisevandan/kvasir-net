'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { EventEmitter } = require('node:events')
const net = require('node:net')
const { LocalServer, freePort } = require('./localServer.cjs')

function harness({ wrongModel = false, wrongAlias = false, dead = false, unhealthy = false, hang = '' } = {}) {
  const children = [], urls = [], args = []
  const server = new LocalServer({
    allocatePort: async () => 49152 + children.length,
    spawnProcess: (_bin, argv) => {
      args.push(argv)
      const child = Object.assign(new EventEmitter(), { killed: false, kill() { this.killed = true } })
      children.push(child)
      return child
    },
    sleep: async () => { if (dead) children.at(-1).emit('exit', 1) },
    timeouts: { health: 10, props: 10, completion: 10 },
    request: async (url, { signal } = {}) => {
      urls.push(url)
      if (hang && url.endsWith(hang)) return new Promise((resolve, reject) => {
        signal.addEventListener('abort', () => reject(signal.reason), { once: true })
      })
      if (url.endsWith('/health')) return { ok: !unhealthy }
      if (url.endsWith('/props')) return { ok: true, json: async () => ({ model_path: wrongModel ? '/other.gguf' : args.at(-1)[1] }) }
      if (url.endsWith('/v1/models')) return { ok: true, json: async () => ({ data: [{
        id: wrongAlias ? 'other-launch' : args.at(-1)[args.at(-1).indexOf('--alias') + 1],
      }] }) }
      return { ok: true, json: async () => ({ timings: { predicted_n: 64, predicted_ms: 800 } }) }
    },
  })
  return { server, children, urls, args, run: model => server.generate('/bin/llama-server', model || '/models/qwen.gguf', 'test', 64) }
}
test('uses OS-selected loopback port instead of fixed listener', async () => {
  const port = await freePort()
  assert.ok(Number.isInteger(port) && port > 0)
  const h = harness()
  assert.equal((await h.run()).ok, true)
  assert.ok(h.urls.every(url => url.startsWith('http://127.0.0.1:49152/')))
  assert.equal(h.args[0][h.args[0].indexOf('--port') + 1], '49152')
})
test('a healthy different-model listener cannot supply benchmark timings', async () => {
  const h = harness({ wrongModel: true })
  assert.equal((await h.run()).ok, false)
  assert.ok(!h.urls.some(url => url.endsWith('/completions')))
  assert.equal(h.children[0].killed, true)
})
test('same-model foreign listener fails the per-launch alias check', async () => {
  const h = harness({ wrongAlias: true })
  assert.match((await h.run()).error, /launch ownership/)
  assert.ok(!h.urls.some(url => url.endsWith('/completions')))
  assert.equal(h.children[0].killed, true)
})
for (const endpoint of ['/health', '/props', '/v1/models', '/completions']) {
  test(`hung ${endpoint} aborts, kills owned child and releases concurrency guard`, async () => {
    // AbortSignal.timeout is unref'ed; keep the test alive while awaiting it.
    const keepAlive = setInterval(() => {}, 1000)
    try {
      const h = harness({ hang: endpoint })
      assert.equal((await h.run()).ok, false)
      assert.equal(h.children[0].killed, true)
      assert.equal(h.server.current, null)
      assert.equal(h.server.busy, false)
    } finally { clearInterval(keepAlive) }
  })
}
test('child exit fails before contacting a listener', async () => {
  const h = harness({ dead: true })
  assert.equal((await h.run()).ok, false)
  assert.deepEqual(h.urls, [])
})
test('health timeout never sends a completion', async () => {
  const h = harness({ unhealthy: true })
  assert.match((await h.run()).error, /timed out/)
  assert.equal(h.urls.length, 60)
  assert.equal(h.children[0].killed, true)
})
test('model changes replace owned child; late exit cannot clear new child', async () => {
  const h = harness()
  await h.run()
  await h.run('/models/second.gguf')
  assert.equal(h.children[0].killed, true)
  h.children[0].emit('exit', 0)
  assert.equal(h.server.current.child, h.children[1])
  assert.equal(h.server.current.model, '/models/second.gguf')
  h.server.stop()
  assert.equal(h.children[1].killed, true)
})
test('same-model calls reuse only the owned child', async () => {
  const h = harness()
  await h.run(); await h.run()
  assert.equal(h.children.length, 1)
})
test('concurrent generations fail closed without starting another child', async () => {
  const h = harness()
  const first = h.run()
  assert.match((await h.run()).error, /already in progress/)
  assert.equal((await first).ok, true)
  assert.equal(h.children.length, 1)
})
test('spawn error is handled and next call can retry', async () => {
  const h = harness()
  await h.run()
  h.children[0].emit('error', new Error('spawn failed'))
  assert.equal(h.server.current, null)
  assert.equal((await h.run()).ok, true)
})
test('port allocation excludes an occupied loopback port', async () => {
  const listener = net.createServer()
  await new Promise(resolve => listener.listen(0, '127.0.0.1', resolve))
  try { assert.notEqual(await freePort(), listener.address().port) }
  finally { await new Promise(resolve => listener.close(resolve)) }
})

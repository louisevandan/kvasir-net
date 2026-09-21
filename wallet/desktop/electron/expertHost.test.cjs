'use strict'
// node electron/expertHost.test.cjs
//
// A loopback gateway (shard download + /api/expert-relay), and a fake worker
// that serves a TCP port the way linkcpp-expert-worker --serve does. What is
// under test is the host's discipline: never report a segment it cannot
// compute, never keep a shard that did not check out, and carry relay bytes
// both ways.
const assert = require('node:assert/strict')
const fs = require('node:fs')
const http = require('node:http')
const os = require('node:os')
const path = require('node:path')
const { WebSocketServer } = require('ws')
const { ExpertHost, readGgufMetadata } = require('./expertHost.cjs')

const TOKEN = 'test-node-token'

/** A GGUF header with kvasir.expert_shard.* u32 keys and no tensors. */
function gguf({ layer, begin, end, pad = 0 }) {
  const parts = []
  const u32 = (v) => { const b = Buffer.alloc(4); b.writeUInt32LE(v); return b }
  const u64 = (v) => { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(v)); return b }
  const str = (s) => Buffer.concat([u64(Buffer.byteLength(s)), Buffer.from(s)])
  const kv = [
    ['general.architecture', 8, 'step35'],
    ['kvasir.expert_shard.model', 8, 'm'],
    ['kvasir.expert_shard.layer', 4, layer],
    ['kvasir.expert_shard.expert_begin', 4, begin],
    ['kvasir.expert_shard.expert_end', 4, end],
  ]
  parts.push(Buffer.from('GGUF'), u32(3), u64(0), u64(kv.length))
  for (const [k, t, v] of kv) parts.push(str(k), u32(t), t === 8 ? str(v) : u32(v))
  parts.push(Buffer.alloc(pad))
  return Buffer.concat(parts)
}

// Fake worker: `node fake-worker.js --model M --serve PORT ...`, replies to
// every chunk with "ack:" + chunk, dies when it receives "die".
const FAKE_WORKER = `
const net = require('net')
const a = process.argv
const port = Number(a[a.indexOf('--serve') + 1])
if (process.env.FAKE_WORKER_FAIL) { console.error('cannot open model'); process.exit(3) }
net.createServer((c) => c.on('data', (d) => {
  if (String(d) === 'die') process.exit(7)
  c.write(Buffer.concat([Buffer.from('ack:'), d]))
})).listen(port, '127.0.0.1', () => console.error('expert worker serving'))
`

async function withGateway(handlers, fn) {
  const hits = { shard: 0 }
  const server = http.createServer((req, res) => {
    const u = new URL(req.url, 'http://x')
    if (u.pathname.endsWith('/expert-shard')) {
      hits.shard++
      assert.equal(req.headers.authorization, `Bearer ${TOKEN}`)
      return handlers.shard(u, res, hits.shard)
    }
    res.writeHead(404).end()
  })
  const wss = new WebSocketServer({ noServer: true })
  server.on('upgrade', (req, socket, head) => {
    const u = new URL(req.url, 'http://x')
    if (u.pathname !== '/api/expert-relay') return socket.destroy()
    wss.handleUpgrade(req, socket, head, (ws) => handlers.relay(ws, u))
  })
  await new Promise((r) => server.listen(0, '127.0.0.1', r))
  const base = `http://127.0.0.1:${server.address().port}`
  try { return await fn(base, hits) } finally {
    for (const c of wss.clients) c.terminate()
    await new Promise((r) => server.close(r))
    server.closeAllConnections?.()
  }
}

function makeHost(base, dir, extra = {}) {
  const workerJs = path.join(dir, 'fake-worker.js')
  fs.writeFileSync(workerJs, FAKE_WORKER)
  const lines = []
  const host = new ExpertHost({
    baseUrl: base,
    token: async () => TOKEN,
    workerBinary: () => process.execPath,
    workerArgs: [workerJs],
    shardDir: path.join(dir, 'shards'),
    log: (l) => lines.push(l),
    ...extra,
  })
  host.lines = lines
  return host
}

const ASSIGN = { model: 'm', layer: 7, experts: [4, 6], n_embd: 4096 }
const serveShard = (u, res) => {
  const body = gguf({ layer: Number(u.searchParams.get('layer')), begin: Number(u.searchParams.get('expert_begin')),
    end: Number(u.searchParams.get('expert_end')), pad: 4096 })
  res.writeHead(200, { 'content-length': body.length }).end(body)
}
const until = async (cond, ms = 5000) => {
  const end = Date.now() + ms
  while (Date.now() < end) { if (cond()) return; await new Promise((r) => setTimeout(r, 20)) }
  throw new Error('timed out waiting')
}

const tests = []
const test = (name, fn) => tests.push([name, fn])

test('GGUF metadata reader reads u32 kvasir keys', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'eh-'))
  const f = path.join(dir, 'x.gguf')
  fs.writeFileSync(f, gguf({ layer: 3, begin: 5, end: 7 }))
  const m = readGgufMetadata(f)
  assert.equal(m['kvasir.expert_shard.layer'], 3)
  assert.equal(m['kvasir.expert_shard.expert_begin'], 5)
  assert.equal(m['kvasir.expert_shard.expert_end'], 7)
  fs.writeFileSync(f, Buffer.from('not a gguf at all, just bytes'))
  assert.throws(() => readGgufMetadata(f), /not a GGUF/)
})

test('reports the segment only once the worker serves it', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'eh-'))
  await withGateway({ shard: serveShard, relay: () => {} }, async (base) => {
    const host = makeHost(base, dir)
    assert.deepEqual(host.heldSegments(), [])
    const p = host.provision(ASSIGN, { workerId: 'desktop-abc' })
    assert.ok(host.busy(), 'busy while downloading')
    assert.deepEqual(host.heldSegments(), [], 'nothing reported while downloading')
    assert.equal(host.coverageUrl(), '')
    await p
    assert.equal(host.phase, 'serving')
    assert.deepEqual(host.heldSegments(), [[7, 4, 6]])
    assert.equal(host.coverageUrl(), 'relay:expert-desktop-abc')
    assert.ok(fs.existsSync(host.shardFile({ model: 'm', layer: 7, begin: 4, end: 6 })))
    host.release('test over')
    assert.deepEqual(host.heldSegments(), [])
    assert.equal(host.busy(), false)
  })
})

test('a truncated download is retried and never left on disk', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'eh-'))
  const shard = (u, res, n) => {
    if (n === 1) {
      // Promise the full length, send part of it, end the connection.
      const body = gguf({ layer: 7, begin: 4, end: 6, pad: 4096 })
      res.writeHead(200, { 'content-length': body.length })
      res.write(body.subarray(0, 100))
      return setTimeout(() => res.socket.destroy(), 20)
    }
    serveShard(u, res)
  }
  await withGateway({ shard, relay: () => {} }, async (base, hits) => {
    const host = makeHost(base, dir)
    await host.provision(ASSIGN, { workerId: 'w' })
    assert.equal(hits.shard, 2)
    assert.equal(host.phase, 'serving')
    const f = host.shardFile({ model: 'm', layer: 7, begin: 4, end: 6 })
    assert.equal(fs.existsSync(`${f}.part`), false)
    assert.ok(host.lines.some((l) => /attempt 1 failed/.test(l)))
    host.release()
  })
})

test('a shard for the wrong range is refused, and nothing is held', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'eh-'))
  const wrong = (u, res) => {
    const body = gguf({ layer: 7, begin: 0, end: 2 })
    res.writeHead(200, { 'content-length': body.length }).end(body)
  }
  await withGateway({ shard: wrong, relay: () => {} }, async (base) => {
    const host = makeHost(base, dir)
    await assert.rejects(host.provision(ASSIGN, { workerId: 'w' }), /download failed.*not layer 7 4-6/)
    assert.equal(host.phase, 'failed')
    assert.equal(host.busy(), false)
    assert.deepEqual(host.heldSegments(), [])
    assert.equal(fs.existsSync(host.shardFile({ model: 'm', layer: 7, begin: 4, end: 6 })), false)
  })
})

test('a worker that cannot start is reported, not served', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'eh-'))
  await withGateway({ shard: serveShard, relay: () => {} }, async (base) => {
    process.env.FAKE_WORKER_FAIL = '1'
    try {
      const host = makeHost(base, dir)
      await assert.rejects(host.provision(ASSIGN, { workerId: 'w' }), /exited before serving: cannot open model/)
      assert.deepEqual(host.heldSegments(), [])
    } finally { delete process.env.FAKE_WORKER_FAIL }
  })
})

test('relay bytes cross both ways, and a dead worker stops being reported', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'eh-'))
  let relayQuery = null
  let bridgeSide = null
  const replies = []
  const relay = (ws, u) => {
    relayQuery = u.searchParams
    bridgeSide = ws
    ws.on('message', (d) => replies.push(String(d)))
  }
  await withGateway({ shard: serveShard, relay }, async (base) => {
    const host = makeHost(base, dir)
    await host.provision(ASSIGN, { workerId: 'desktop-abc' })
    await until(() => bridgeSide)
    assert.equal(relayQuery.get('session'), 'expert-desktop-abc')
    assert.equal(relayQuery.get('token'), TOKEN)
    await until(() => host.relay.tcp)
    await new Promise((r) => setTimeout(r, 50))
    bridgeSide.send(Buffer.from('hello'))
    await until(() => replies.length > 0)
    assert.equal(replies[0], 'ack:hello')
    // The worker dies: the segment must disappear from the next report.
    bridgeSide.send(Buffer.from('die'))
    await until(() => host.phase === 'failed')
    assert.deepEqual(host.heldSegments(), [])
    assert.equal(host.coverageUrl(), '')
    assert.match(host.lastError, /worker exited/)
    host.release()
  })
})

test('the same assignment twice does not restart; a new one replaces it', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'eh-'))
  await withGateway({ shard: serveShard, relay: () => {} }, async (base, hits) => {
    const host = makeHost(base, dir)
    await host.provision(ASSIGN, { workerId: 'w' })
    const port = host.port
    await host.provision(ASSIGN, { workerId: 'w' })
    assert.equal(host.port, port)
    assert.equal(hits.shard, 1)
    await host.provision({ ...ASSIGN, experts: [8, 9] }, { workerId: 'w' })
    assert.deepEqual(host.heldSegments(), [[7, 8, 9]])
    assert.equal(hits.shard, 2)
    host.release()
    // Cached on disk: a restart does not download again.
    await host.provision(ASSIGN, { workerId: 'w' })
    assert.equal(hits.shard, 2)
    assert.ok(host.lines.some((l) => /using cached shard/.test(l)))
    host.release()
  })
})

;(async () => {
  let passed = 0
  for (const [name, fn] of tests) {
    try { await fn(); passed++; console.log('ok -', name) }
    catch (e) { console.log('FAIL -', name); console.log(e); process.exitCode = 1 }
  }
  console.log(`\n${passed}/${tests.length} passed`)
  process.exit()
})()

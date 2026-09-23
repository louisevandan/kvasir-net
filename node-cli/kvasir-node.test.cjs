'use strict'
// node kvasir-node.test.cjs
//
// The CLI is glue, so the test that matters drives the whole node — key
// file, token, volunteer, shard download, worker, per-slot coverage — against
// a loopback bridge and a fake worker. The rest pins the pieces a server
// operator touches: the key file, the token store, the budget.
const assert = require('node:assert/strict')
const fs = require('node:fs')
const http = require('node:http')
const os = require('node:os')
const path = require('node:path')
const nacl = require('tweetnacl')
const { WebSocketServer } = require('ws')
const cli = require('./kvasir-node.cjs')

const tests = []
const test = (name, fn) => tests.push([name, fn])
const tmp = () => fs.mkdtempSync(path.join(os.tmpdir(), 'kn-'))
const MIB = 1024 * 1024
const until = async (cond, ms = 8000) => {
  const end = Date.now() + ms
  while (Date.now() < end) { if (cond()) return; await new Promise((r) => setTimeout(r, 20)) }
  throw new Error('timed out waiting')
}

test('base58 matches Solana for the all-zero key', () => {
  assert.equal(cli.base58(new Uint8Array(32)), '11111111111111111111111111111111')
})

test('keygen writes a 0600 keypair that loads to the same address', () => {
  const f = path.join(tmp(), 'k', 'node-key.json')
  const address = cli.keygen(f)
  assert.equal(cli.loadKey(f).address, address)
  if (process.platform !== 'win32') assert.equal(fs.statSync(f).mode & 0o777, 0o600)
  assert.throws(() => cli.keygen(f), /refusing to overwrite/)
})

test('a key file readable by others is refused', () => {
  if (process.platform === 'win32') return   // no POSIX modes to check
  const f = path.join(tmp(), 'key.json')
  fs.writeFileSync(f, JSON.stringify(Array.from(nacl.sign.keyPair().secretKey)), { mode: 0o644 })
  assert.throws(() => cli.loadKey(f), /readable by other users/)
})

test('the token store drops a token minted for another wallet', () => {
  const f = path.join(tmp(), 'node-token.json')
  let who = 'WalletA'
  const store = cli.tokenStore(f, () => who)
  store.save({ token: 't', expiresAt: 1, wallet: 'WalletA' })
  assert.equal(store.load().token, 't')
  who = 'WalletB'
  assert.equal(store.load(), null)
  store.save(null)
  assert.equal(fs.existsSync(f), false)
})

test('budget: explicit wins, else half the card, else refuse on unified memory', async () => {
  assert.equal(await cli.resolveBudget({ budget: '2' }), 2 * 1024 ** 3)
  await assert.rejects(cli.resolveBudget({ budget: '0' }), /positive/)
  assert.equal(await cli.resolveBudget({}, async () => ({ gpus: [{ totalBytes: 8 * 1024 ** 3 }] })), 4 * 1024 ** 3)
  await assert.rejects(cli.resolveBudget({}, async () => ({ gpus: [{ totalBytes: NaN }] })), /--budget/)
  await assert.rejects(cli.resolveBudget({}, async () => ({ vendor: null })), /--budget/)
})

// ---- the whole node --------------------------------------------------------------

function gguf({ layer, begin, end }) {
  const u32 = (v) => { const b = Buffer.alloc(4); b.writeUInt32LE(v); return b }
  const u64 = (v) => { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(v)); return b }
  const str = (s) => Buffer.concat([u64(Buffer.byteLength(s)), Buffer.from(s)])
  const kv = [['kvasir.expert_shard.layer', layer], ['kvasir.expert_shard.expert_begin', begin], ['kvasir.expert_shard.expert_end', end]]
  const parts = [Buffer.from('GGUF'), u32(3), u64(0), u64(kv.length)]
  for (const [k, v] of kv) parts.push(str(k), u32(4), u32(v))
  parts.push(Buffer.alloc(1024))
  return Buffer.concat(parts)
}

const FAKE_WORKER = `
const net = require('net'); const a = process.argv
const port = Number(a[a.indexOf('--serve') + 1])
net.createServer((c) => c.on('data', (d) => c.write(d))).listen(port, '127.0.0.1')
`

test('a node signs in, takes two slots, and reports each under its own id', async () => {
  const dir = tmp()
  const keyFile = path.join(dir, 'key.json')
  const address = cli.keygen(keyFile)
  const workerJs = path.join(dir, 'worker.js')
  fs.writeFileSync(workerJs, FAKE_WORKER)
  const coverage = []
  let assigned = 0
  const server = http.createServer((req, res) => {
    let body = ''
    req.on('data', (c) => { body += c })
    req.on('end', () => {
      const u = new URL(req.url, 'http://x')
      const json = (o) => { res.writeHead(200, { 'content-type': 'application/json' }); res.end(JSON.stringify(o)) }
      const b = body ? JSON.parse(body) : null
      if (u.pathname === '/api/auth/challenge') return json({ nonce: 'n', message: 'sign me' })
      if (u.pathname === '/api/auth/node-token') {
        const ok = nacl.sign.detached.verify(Buffer.from('sign me'), Buffer.from(b.signature, 'base64'), nacl.sign.keyPair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(keyFile, 'utf8')))).publicKey)
        assert.ok(ok, 'the node signed with the key file')
        assert.equal(b.wallet, address)
        return json({ node_token: 'tok', expires_in: 2592000 })
      }
      if (u.pathname === '/api/expert-volunteer') {
        // Like the real bridge, the scarce segment bounds the window too: 8 wide here.
        const layer = 3 + assigned++
        return json({ assigned: true, model: 'm', layer, experts: [0, Math.min(8, b.max_experts)], n_embd: 4096, n_layer: 45, n_expert: 288 })
      }
      if (u.pathname === '/api/expert-coverage') { coverage.push(b); return json({ ok: true, wired: true }) }
      if (u.pathname.endsWith('/expert-shard')) {
        const buf = gguf({ layer: Number(u.searchParams.get('layer')), begin: Number(u.searchParams.get('expert_begin')), end: Number(u.searchParams.get('expert_end')) })
        res.writeHead(200, { 'content-length': buf.length }); return res.end(buf)
      }
      res.writeHead(404).end()
    })
  })
  const wss = new WebSocketServer({ noServer: true })
  server.on('upgrade', (req, socket, head) => wss.handleUpgrade(req, socket, head, () => {}))
  await new Promise((r) => server.listen(0, '127.0.0.1', r))
  const lines = []
  // A tiny memory model so two 8-expert slots fill the budget exactly.
  const model = { residentBytesPerExpert: 1 * MIB, fixedBytes: 4 * MIB, scratchBytes: 4 * MIB, headroomBytes: 8 * MIB }
  const node = cli.createNode({
    key: keyFile, data: path.join(dir, 'data'), gateway: `http://127.0.0.1:${server.address().port}`,
    name: 'Test Box', worker: process.execPath, workerArgs: [workerJs],
    budgetBytes: 8 * MIB + 2 * (8 * MIB + 8 * MIB), memoryModel: model, log: (l) => lines.push(l),
  })
  try {
    assert.match(node.workerId, /^server-[1-9A-HJ-NP-Za-km-z]{8}-test-box$/)
    node.participation.start({ pollMs: 50 })
    await until(() => {
      const ids = new Set(coverage.filter((c) => c.segments.length).map((c) => c.worker_id))
      return ids.size === 2
    })
    const held = coverage.filter((c) => c.segments.length)
    assert.deepEqual([...new Set(held.map((c) => c.worker_id))].sort(), [node.workerId, `${node.workerId}-2`])
    assert.ok(held.every((c) => c.owner === address), 'rewards go to the key file wallet')
    assert.ok(held.every((c) => c.url === `relay:expert-${c.worker_id}`))
    // Recorded as what it is, not left for the bridge to guess (it assumed "phone").
    assert.ok(held.every((c) => c.device_kind === 'server' && c.accelerator === 'gpu' && c.os && c.backend))
    // Two 8-expert slots use the budget up (16 MiB each + 8 MiB headroom); no third ask.
    await new Promise((r) => setTimeout(r, 300))
    assert.equal(assigned, 2, 'the budget fits two slots; it stops asking')
    assert.ok(fs.existsSync(path.join(dir, 'data', 'node-token.json')))
  } finally {
    node.participation.stop()
    for (const c of wss.clients) c.terminate()
    server.closeAllConnections?.()
    await new Promise((r) => server.close(r))
  }
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

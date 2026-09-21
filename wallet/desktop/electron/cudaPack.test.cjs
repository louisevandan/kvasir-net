'use strict'
// node electron/cudaPack.test.cjs
//
// A loopback server stands in for R2 (with Range support), and the zip is
// built here so each failure can be staged: a wrong hash, an entry that
// escapes the install dir, a download cut off halfway.
const assert = require('node:assert/strict')
const crypto = require('node:crypto')
const fs = require('node:fs')
const http = require('node:http')
const os = require('node:os')
const path = require('node:path')
const zlib = require('node:zlib')
const { CudaPack } = require('./cudaPack.cjs')

/** Minimal zip writer: deflate entries, no zip64. */
function makeZip(files) {
  const locals = []
  const centrals = []
  let offset = 0
  for (const [name, content] of Object.entries(files)) {
    const data = Buffer.from(content)
    const packed = zlib.deflateRawSync(data)
    const nameBuf = Buffer.from(name)
    const crc = zlib.crc32 ? zlib.crc32(data) : 0
    const local = Buffer.alloc(30)
    local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(20, 4); local.writeUInt16LE(8, 8)
    local.writeUInt32LE(crc, 14); local.writeUInt32LE(packed.length, 18); local.writeUInt32LE(data.length, 22)
    local.writeUInt16LE(nameBuf.length, 26)
    const central = Buffer.alloc(46)
    central.writeUInt32LE(0x02014b50, 0); central.writeUInt16LE(20, 4); central.writeUInt16LE(20, 6)
    central.writeUInt16LE(8, 10); central.writeUInt32LE(crc, 16); central.writeUInt32LE(packed.length, 20)
    central.writeUInt32LE(data.length, 24); central.writeUInt16LE(nameBuf.length, 28); central.writeUInt32LE(offset, 42)
    locals.push(local, nameBuf, packed)
    centrals.push(central, nameBuf)
    offset += 30 + nameBuf.length + packed.length
  }
  const cd = Buffer.concat(centrals)
  const eocd = Buffer.alloc(22)
  eocd.writeUInt32LE(0x06054b50, 0)
  eocd.writeUInt16LE(Object.keys(files).length, 8); eocd.writeUInt16LE(Object.keys(files).length, 10)
  eocd.writeUInt32LE(cd.length, 12); eocd.writeUInt32LE(offset, 16)
  return Buffer.concat([...locals, cd, eocd])
}

async function serve(body, { cutAt = null } = {}) {
  let requests = 0
  const ranges = []
  const server = http.createServer((req, res) => {
    requests++
    const m = /bytes=(\d+)-/.exec(req.headers.range || '')
    const start = m ? Number(m[1]) : 0
    ranges.push(start)
    const slice = body.subarray(start)
    res.writeHead(m ? 206 : 200, { 'content-length': slice.length })
    if (cutAt != null && requests === 1) {
      res.write(slice.subarray(0, cutAt))
      return setTimeout(() => res.socket.destroy(), 20)
    }
    res.end(slice)
  })
  await new Promise((r) => server.listen(0, '127.0.0.1', r))
  return {
    url: `http://127.0.0.1:${server.address().port}/pack.zip`,
    ranges,
    close: () => new Promise((r) => { server.closeAllConnections?.(); server.close(r) }),
  }
}

const ok = async () => ({ ok: true })
const WORKER = 'linkcpp-expert-worker.exe'
function pack(url, body, extra = {}) {
  return { version: 'v1', url, sha256: crypto.createHash('sha256').update(body).digest('hex'),
    bytes: body.length, worker: WORKER, minComputeCapability: 5, minCudaMajor: 12, ...extra }
}

const tests = []
const test = (name, fn) => tests.push([name, fn])

test('installs, verifies and exposes the worker', async () => {
  const body = makeZip({ [WORKER]: 'MZ fake worker', 'cublas64_12.dll': 'x'.repeat(5000), 'README.txt': 'hi' })
  const srv = await serve(body)
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cp-'))
  try {
    const cp = new CudaPack({ dir, pack: pack(srv.url, body), check: ok })
    assert.equal(cp.workerPath(), null)
    const w = await cp.install()
    assert.equal(w, path.join(dir, 'v1', WORKER))
    assert.equal(fs.readFileSync(w, 'utf8'), 'MZ fake worker')
    assert.equal(fs.readFileSync(path.join(dir, 'v1', 'cublas64_12.dll'), 'utf8').length, 5000)
    assert.equal(cp.status().phase, 'installed')
    assert.equal(fs.existsSync(path.join(dir, 'v1.zip.part')), false)
    assert.equal(fs.existsSync(path.join(dir, 'v1.tmp')), false)
  } finally { await srv.close() }
})

test('a pack that does not match the pinned hash is refused and discarded', async () => {
  const body = makeZip({ [WORKER]: 'tampered' })
  const srv = await serve(body)
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cp-'))
  try {
    const cp = new CudaPack({ dir, pack: pack(srv.url, body, { sha256: '0'.repeat(64) }), check: ok })
    await assert.rejects(cp.install(), /does not match its expected hash/)
    assert.equal(cp.workerPath(), null)
    assert.equal(fs.existsSync(path.join(dir, 'v1.zip.part')), false, 'a bad archive must not be resumed from')
  } finally { await srv.close() }
})

test('an entry that escapes the install dir aborts the install', async () => {
  const body = makeZip({ [WORKER]: 'w', '../../evil.dll': 'nope' })
  const srv = await serve(body)
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cp-'))
  try {
    const cp = new CudaPack({ dir, pack: pack(srv.url, body), check: ok })
    await assert.rejects(cp.install(), /escapes the install dir/)
    assert.equal(cp.workerPath(), null)
    assert.equal(fs.existsSync(path.join(dir, '..', 'evil.dll')), false)
  } finally { await srv.close() }
})

test('a download cut off halfway resumes from where it stopped', async () => {
  const body = makeZip({ [WORKER]: 'w'.repeat(20000), 'cublas64_12.dll': crypto.randomBytes(40000) })
  const srv = await serve(body, { cutAt: 10000 })
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cp-'))
  try {
    const cp = new CudaPack({ dir, pack: pack(srv.url, body), check: ok })
    await assert.rejects(cp.install(), /ended at 10000 of|terminated|socket/)
    assert.equal(fs.statSync(path.join(dir, 'v1.zip.part')).size, 10000)
    await cp.install()
    assert.deepEqual(srv.ranges, [0, 10000])
    assert.ok(cp.workerPath())
  } finally { await srv.close() }
})

test('an ineligible GPU downloads nothing', async () => {
  const body = makeZip({ [WORKER]: 'w' })
  const srv = await serve(body)
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cp-'))
  try {
    const cp = new CudaPack({ dir, pack: pack(srv.url, body), check: async () => ({ ok: false, reason: 'compute capability 3.5 is below 5' }) })
    await assert.rejects(cp.install(), /below 5/)
    assert.deepEqual(srv.ranges, [])
  } finally { await srv.close() }
})

test('an unpublished pack says so', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cp-'))
  const cp = new CudaPack({ dir, pack: pack(null, Buffer.from('x')), check: ok })
  assert.equal(cp.status().available, false)
  await assert.rejects(cp.install(), /not been published/)
})

;(async () => {
  let passed = 0
  for (const [name, fn] of tests) {
    try { await fn(); passed++; console.log('ok -', name) }
    catch (e) { console.log('FAIL -', name); console.log(e); process.exitCode = 1 }
  }
  console.log(`\n${passed}/${tests.length} passed`)
})()

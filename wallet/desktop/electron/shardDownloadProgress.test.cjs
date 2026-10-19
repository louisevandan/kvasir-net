'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const { ExpertHost, ExpertPool } = require('./expertHost.cjs')

test('fetchTo publishes byte progress while streaming and clears it afterwards', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'shard-progress-'))
  const seen = []
  let host
  const body = (async function* () {
    for (let i = 0; i < 4; i++) {
      yield Buffer.alloc(256)
      await new Promise(r => setImmediate(r))
      seen.push({ ...host.downloadProgress })
    }
  })()
  host = new ExpertHost({ baseUrl: 'http://gate.test', token: async () => 'tok', workerBinary: () => null, shardDir: dir,
    WebSocket: function () {}, fetch: async () => ({ ok: true, headers: { get: () => '1024' }, body }) })
  assert.equal(host.downloadProgress ?? null, null)
  await host.fetchTo('/x', path.join(dir, 'f.part'))
  assert.deepEqual(seen.map(p => p.bytes), [256, 512, 768, 1024])
  assert.ok(seen.every(p => p.totalBytes === 1024 && Number.isFinite(p.startedAt)))
  assert.equal(host.downloadProgress, null)
  assert.equal(typeof host.download, 'function') // the download() method is not shadowed
  assert.equal(host.status().download, null)
  fs.rmSync(dir, { recursive: true, force: true })
})

test('pool downloadStatus reports per-slot progress copies without touching hardware', () => {
  const pool = new ExpertPool({ workerId: () => 'fixture' })
  pool.slots = [{ phase: 'downloading', downloadProgress: { bytes: 5, totalBytes: 10, startedAt: 1 } }, { phase: 'serving', downloadProgress: null }]
  const out = pool.downloadStatus()
  assert.equal(out.length, 2)
  assert.deepEqual(out[0].download, { bytes: 5, totalBytes: 10, startedAt: 1 })
  assert.equal(out[1].download, null)
  out[0].download.bytes = 99
  assert.equal(pool.slots[0].downloadProgress.bytes, 5)
})

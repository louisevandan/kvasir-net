'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { EventEmitter } = require('node:events')
const { DirectRelay, directRelaySettings } = require('./directRelay.cjs')
const { ExpertHost } = require('./expertHost.cjs')

const cfg = { directRelay: { host: 'h', user: 'u', keyPath: '/k', allowAddresses: ['ME'] } }
test('settings apply only to a listed wallet with a complete config', () => {
  assert.deepEqual(directRelaySettings(cfg, 'ME'), { host: 'h', port: 22, user: 'u', keyPath: '/k', bridgePort: 19000 })
  assert.equal(directRelaySettings(cfg, 'SOMEONE'), null)
  assert.equal(directRelaySettings(cfg, null), null)
  assert.equal(directRelaySettings({}, 'ME'), null)
  assert.equal(directRelaySettings({ directRelay: { ...cfg.directRelay, keyPath: '' } }, 'ME'), null)
})
test('forward comes up with a restricted ssh -L and falls back (null base) when ssh exits', async () => {
  const procs = []
  const relay = new DirectRelay({ settings: directRelaySettings(cfg, 'ME'), exists: () => true, allocatePort: async () => 40001, wait: async () => {}, retryMs: 60000,
    spawnProcess: (cmd, args) => { const p = new EventEmitter(); p.cmd = cmd; p.args = args; p.kill = () => p.emit('exit', null); procs.push(p); return p } })
  assert.equal(relay.base(), null)
  await relay.start()
  assert.equal(relay.base(), 'ws://127.0.0.1:40001'); assert.equal(relay.status().state, 'up')
  const a = procs[0].args
  assert.equal(procs[0].cmd, 'ssh'); assert.ok(a.includes('-N')); assert.ok(a.includes('BatchMode=yes')); assert.ok(a.includes('ExitOnForwardFailure=yes'))
  assert.ok(a.includes('127.0.0.1:40001:127.0.0.1:19000')); assert.equal(a.at(-1), 'u@h')
  procs[0].emit('exit', 255)
  assert.equal(relay.base(), null); assert.equal(relay.status().state, 'failed'); assert.match(relay.status().error, /255/)
  relay.stop(); assert.equal(relay.status().state, 'off')
})
test('missing key fails without spawning; no settings is a no-op', async () => {
  let spawned = 0
  const r = new DirectRelay({ settings: directRelaySettings(cfg, 'ME'), exists: () => false, spawnProcess: () => { spawned++ } })
  assert.equal((await r.start()).state, 'failed'); assert.equal(spawned, 0)
  assert.equal((await new DirectRelay({ settings: null }).start()).state, 'off')
})
test('expert host dials the direct base when it is up, the gateway otherwise', () => {
  let base = null
  const host = new ExpertHost({ baseUrl: 'https://gate.example', token: async () => 't', workerBinary: () => null, shardDir: '/tmp', WebSocket: function () {}, relayBase: () => base })
  host.session = 's'
  assert.match(host.relayUrl('tok'), /^wss:\/\/gate\.example\/api\/expert-relay\?/)
  base = 'ws://127.0.0.1:40001'
  assert.match(host.relayUrl('tok'), /^ws:\/\/127\.0\.0\.1:40001\/api\/expert-relay\?session=s&token=tok$/)
})

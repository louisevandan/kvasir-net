'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { createLiveRingCache } = require('./liveRing.cjs')

// Execute the real status handler without starting Electron or opening a wallet.
test('nodeStatus returns while the live-ring request is still pending', async () => {
  const source = fs.readFileSync(process.env.NODE_STATUS_MAIN_SOURCE || path.join(__dirname, 'main.cjs'), 'utf8')
  const start = source.indexOf('async function nodeStatus(')
  assert.ok(start >= 0)
  const agent = { running: true, snapshotAt: Date.now(), address: 'agent', nodes: [{ nodeId: 'stage' }] }
  let requests = 0
  let finish
  const observe = () => { requests++; return new Promise(resolve => { finish = resolve }) }
  const context = vm.createContext({
    p4node: { status: () => agent, inspect: async () => {} },
    expertPool: { status: () => ({}), heldExperts: () => 0 },
    observeLiveRing: observe,
    cachedLiveRing: createLiveRingCache ? createLiveRingCache({ observe }) : undefined,
    capability: async () => ({}), lastCapability: null, measured: null,
    readConfig: () => ({}), C: { stakingServiceUrl: 'https://example.com' },
    relay: { status: () => ({}) }, participation: { status: () => ({}) }, directRelay: { status: () => ({ state: 'off' }) },
    DEBUG_WALLET: false, refreshGpuTotal: async () => ({}),
    vramBudgetBytes: () => 0, maxExpertsForBudget: () => 0,
    knowsWhatASlotCosts: () => false, memoryTopology: () => 'unknown',
    memoryTopologyReason: () => '', expertMemoryModel: () => null,
    SYSTEM_VRAM_RESERVE: 0, ownWorkerGpuBytes: () => 0, cudaPack: { status: () => ({}) },
  })
  vm.runInContext(source.slice(start, source.indexOf('\n}', start) + 2), context)
  const pending = context.nodeStatus({ inspect: true })
  const result = await Promise.race([pending, new Promise(resolve => setImmediate(() => resolve('blocked')))])
  // Release the mock even on the pre-fix path; no hanging test or real network.
  finish?.({ state: 'unavailable', stages: [] })
  assert.notEqual(result, 'blocked', 'contribution fetch must not delay node status')
  assert.equal(requests, 1)
  assert.equal(result.running, true)
  assert.equal(result.liveRing.state, 'unavailable')
})

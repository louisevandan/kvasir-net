'use strict'
// node electron/executors.test.cjs
const assert = require('node:assert/strict')
const { expertsForBudget, capacityForBudget, nextSlotWindow, slotsThatFit, expertExecutor, KNOWN, MAX_SLOTS } = require('./executors.cjs')

const MIB = 1024 * 1024
const GIB = 1024 * MIB
const cuda = KNOWN.find((k) => k.id === 'linkcpp-expert-worker').memoryModel
let passed = 0
const test = (name, fn) => { fn(); passed++; console.log('ok -', name) }

test('no budget or no model caps at one shard, never unlimited', () => {
  assert.equal(expertsForBudget(null, cuda, null), 64)
  assert.equal(expertsForBudget(4 * GIB, null, null), 64)
})

test('fixed, scratch and headroom come off the top', () => {
  // 320 MiB is exactly F + S + H: nothing is left for weights.
  assert.equal(expertsForBudget(320 * MIB, cuda, null), 0)
  assert.equal(expertsForBudget(0, cuda, null), 0)
  // One expert more than the overheads.
  assert.equal(expertsForBudget(320 * MIB + cuda.residentBytesPerExpert, cuda, null), 1)
})

test('a small budget is not over-asked the way a flat x1.25 did', () => {
  // 512 MiB: the old formula said floor(512 MiB / (9,502,720 * 1.25)) = 45.
  const n = expertsForBudget(512 * MIB, cuda, null)
  assert.equal(n, 21)
  assert.ok(n * cuda.residentBytesPerExpert + 320 * MIB <= 512 * MIB)
})

test('clamped to what one shard carries', () => {
  assert.equal(expertsForBudget(8 * GIB, cuda, null), 64)
})

test('free VRAM caps the budget when another program holds the card', () => {
  // 4 GiB lent, but only 600 MiB actually free right now.
  assert.equal(expertsForBudget(4 * GIB, cuda, 600 * MIB), 30)
  assert.equal(expertsForBudget(4 * GIB, cuda, 0), 0)
})

test('an expanding executor reports its own size and gets fewer experts', () => {
  const fp16 = { ...cuda, residentBytesPerExpert: 31_457_280 }
  // 512 MiB keeps both under the 64 clamp: 21 quantized vs 6 at fp16 size.
  assert.equal(expertsForBudget(512 * MIB, cuda, null), 21)
  assert.equal(expertsForBudget(512 * MIB, fp16, null), 6)
})

test('capacity counts slots, not one shard: a big budget holds several', () => {
  // 4 GiB: five 64-expert slots, each paying F + S, headroom once.
  assert.deepEqual(capacityForBudget(4 * GIB, cuda, null), { experts: 320, slots: 5 })
  // Small budgets are unchanged from the single-slot formula.
  assert.deepEqual(capacityForBudget(512 * MIB, cuda, null), { experts: 21, slots: 1 })
  assert.deepEqual(capacityForBudget(0, cuda, null), { experts: 0, slots: 0 })
  // The slot cap bounds a huge budget (the bridge has 60 relay ports for the fleet).
  assert.deepEqual(capacityForBudget(64 * GIB, cuda, null), { experts: 64 * MAX_SLOTS, slots: MAX_SLOTS })
  // No budget or model: one shard, never unlimited.
  assert.deepEqual(capacityForBudget(null, cuda, null), { experts: 64, slots: 1 })
})

test('the next window is what fits after the slots already held', () => {
  assert.equal(nextSlotWindow(4 * GIB, cuda, null, []), 64)
  assert.equal(nextSlotWindow(4 * GIB, cuda, null, [64, 64, 64, 64]), 64)
  // After five full slots, what is left does not fit a sixth of 64.
  const sixth = nextSlotWindow(4 * GIB, cuda, null, [64, 64, 64, 64, 64])
  assert.ok(sixth < 64, `sixth window ${sixth}`)
  assert.equal(nextSlotWindow(64 * GIB, cuda, null, new Array(MAX_SLOTS).fill(64)), 0)
  // Unknown budget: one slot only.
  assert.equal(nextSlotWindow(null, cuda, null, []), 64)
  assert.equal(nextSlotWindow(null, cuda, null, [64]), 0)
})

test('a lowered budget keeps the oldest slots that still fit', () => {
  assert.equal(slotsThatFit(4 * GIB, cuda, null, [64, 64, 64, 64, 64, 64]), 5)
  assert.equal(slotsThatFit(1 * GIB, cuda, null, [64, 64, 64]), 1)
  assert.equal(slotsThatFit(256 * MIB, cuda, null, [64]), 0)
  // Free VRAM caps it too: another program took the card.
  assert.equal(slotsThatFit(4 * GIB, cuda, 700 * MIB, [64, 64]), 0)
})

test('the expert executor is the one for this platform', () => {
  const { entry } = expertExecutor()
  assert.ok(entry, 'no expert executor for this platform')
  assert.ok(!entry.platforms || entry.platforms.includes(process.platform))
  assert.ok(entry.memoryModel && entry.memoryModel.residentBytesPerExpert > 0)
})

console.log(`\n${passed} passed`)

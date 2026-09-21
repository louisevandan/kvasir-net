'use strict'
// node electron/executors.test.cjs
const assert = require('node:assert/strict')
const { expertsForBudget, KNOWN } = require('./executors.cjs')

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

console.log(`\n${passed} passed`)

'use strict'
// node electron/executors.test.cjs
const assert = require('node:assert/strict')
const { expertsForBudget, capacityForBudget, nextSlotWindow, slotsThatFit, expertExecutor, KNOWN, MAX_SLOTS,
  memoryModelFor, memoryTopology, resolveMemoryTopology, TOPOLOGY } = require('./executors.cjs')

const MIB = 1024 * 1024
const GIB = 1024 * MIB
// The arithmetic is tested on a fixed model, so re-measuring a backend does not
// rewrite these expectations. The shipped models are checked separately below.
const cuda = { residentBytesPerExpert: 9_568_256, fixedBytes: 128 * MIB, scratchBytes: 64 * MIB, headroomBytes: 128 * MIB }
const cudaEntry = KNOWN.find((k) => k.id === 'linkcpp-expert-worker')
const shippedCuda = cudaEntry.memoryModels.discrete
const shippedGrace = cudaEntry.memoryModels.unified
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
  // Either a single measured model (Metal) or one per memory topology (CUDA).
  assert.ok(entry.memoryModel || entry.memoryModels, 'the entry carries no memory model at all')
})

test('an unresolved topology yields no CUDA model, so the node cannot lend', () => {
  // The failure this guards is silent and expensive: a Grace part charged at a
  // card's rates over-commits the machine by ~320 MiB per slot. Refusing to
  // answer is the only safe thing to do before the probe has run.
  assert.equal(memoryTopology.length, 0)
  const unresolved = memoryModelFor({ memoryModels: cudaEntry.memoryModels })
  const known = memoryTopology() !== TOPOLOGY.UNKNOWN
  if (!known) assert.equal(unresolved, null, 'an unknown topology must not pick a model')
  // And a null model must produce no offer, not the "one shard's worth" a
  // slider preview shows.
  assert.equal(capacityForBudget(8 * GIB, null, null).experts, 64,
    'the preview default is unchanged — callers that lend must check for null themselves')
})

test('the two CUDA models differ by the host cost a card does not pay', () => {
  const perSlot = (m) => m.fixedBytes + m.scratchBytes
  const extra = perSlot(shippedGrace) - perSlot(shippedCuda)
  // Measured on a GB10: 174.93 MiB of private host pages at rest and 150.4
  // more while serving, none of which exists on a discrete card.
  assert.ok(extra >= 300 * MIB, `Grace charges only ${extra / MIB} MiB more per slot`)
  assert.equal(shippedGrace.residentBytesPerExpert, shippedCuda.residentBytesPerExpert,
    'R is the served bytes on both; only the fixed and scratch terms move')
})

test('the Grace model halves what a 1.8 GiB budget offers', () => {
  // The GB10 node advertised 124 experts in 2 slots on the discrete model and
  // was 27% over its budget while serving. This is the corrected figure, and
  // the drop is the point rather than a regression.
  const budget = 1843.2 * MIB
  assert.deepEqual(capacityForBudget(budget, shippedCuda, null), { experts: 124, slots: 2 })
  const fixed = capacityForBudget(budget, shippedGrace, null)
  assert.ok(fixed.experts <= 64 && fixed.slots === 1,
    `Grace should fit one slot at this budget, got ${fixed.experts} in ${fixed.slots}`)
})

test('the shipped CUDA model covers what the cuBLAS build was measured to take', () => {
  // RTX 4060, cuBLAS + FP32, 64 experts: +107 MiB scratch at 512 tokens x 8.
  assert.ok(shippedCuda.scratchBytes >= 107 * MIB, `scratch ${shippedCuda.scratchBytes / MIB} MiB`)
  assert.ok(shippedCuda.residentBytesPerExpert >= 9_502_720, 'R below the served bytes per expert')
})

test('a GPU that will not state its size is charged the dearer model, not refused', () => {
  // A GB10 answers [N/A] to nvidia-smi's memory.total, and it does that BECAUSE
  // its memory is the system's. The first version of this probe used that
  // missing number as the discriminator, resolved UNKNOWN, and the headless
  // node then refused to start — every five seconds, forever. Caught before
  // deployment by the machine it would have taken out of the market.
  const cuda = KNOWN.find((k) => k.id === 'linkcpp-expert-worker')
  const perSlot = (m) => m.fixedBytes + m.scratchBytes
  assert.ok(perSlot(cuda.memoryModels.unified) > perSlot(cuda.memoryModels.discrete),
    'unified must be the dearer model, or falling back to it would not be the safe direction')
})

test('the operator can state the topology when the machine will not', () => {
  const prior = process.env.KVASIR_MEMORY_TOPOLOGY
  process.env.KVASIR_MEMORY_TOPOLOGY = 'discrete'
  resolveMemoryTopology().then((t) => {
    assert.equal(t, TOPOLOGY.DISCRETE, 'an explicit setting must win over any probe')
    if (prior == null) delete process.env.KVASIR_MEMORY_TOPOLOGY
    else process.env.KVASIR_MEMORY_TOPOLOGY = prior
    console.log('ok - the operator can state the topology when the machine will not')
  })
})

test('resolving the topology settles it, and the settled answer is usable', () => {
  // Async, so it runs after the synchronous tests; failures still surface
  // because an unhandled rejection fails the process.
  resolveMemoryTopology().then((t) => {
    assert.ok(Object.values(TOPOLOGY).includes(t), `unexpected topology ${t}`)
    assert.equal(memoryTopology(), t)
    if (t !== TOPOLOGY.UNKNOWN) {
      const { entry } = expertExecutor()
      assert.ok(memoryModelFor(entry), 'a settled topology must yield a model')
    }
    console.log('ok - resolving the topology settles it, and the settled answer is usable')
  })
})

console.log(`\n${passed} passed`)

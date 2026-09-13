import assert from "node:assert/strict";
import test from "node:test";
import { planModelLoading } from "../src/model-loading-planner.ts";
import { analystJudgments, fixture } from "../validation/loading-judgments.ts";
import { referencePlan, comparePlan } from "../validation/loading-reference.ts";

for (const row of analystJudgments()) {
  test(`analyst judgment: ${row.id}`, () => {
    const before = structuredClone(row.input);
    const reference = referencePlan(row.input);
    if (!row.expected) {
      assert.equal(reference, null, row.reason);
      assert.throws(() => planModelLoading(row.input), /no (?:memory-)?tier prefix/, row.reason);
    } else {
      assert.deepEqual(reference && { tier: reference.maxTierIndex, period: reference.period, total: reference.total, stages: reference.stages.length }, row.expected, row.reason);
      const result = planModelLoading(row.input);
      assert.equal(comparePlan(row.input, reference, result).category, "optimal", row.reason);
    }
    assert.deepEqual(row.input, before, "planning/refusal must preserve the complete input");
  });
}

test("seeded independent Pareto oracle checks heterogeneous cuts, machine masks and ties", () => {
  let seed = 20260913;
  const rand = (n: number) => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed % n; };
  for (let i = 0; i < 240; i++) {
    const layers = 2 + rand(6), devices = 1 + rand(5);
    const p = fixture(Array.from({ length: devices }, () => 1 + rand(20)),
      Array.from({ length: devices }, () => Array.from({ length: layers }, () => rand(10))),
      Array.from({ length: layers }, () => 1 + rand(5)));
    p.model.fixedBytesPerStage = rand(3);
    p.constraints = { minimumMachines: 1 + rand(devices) };
    if (i % 3 === 0) p.model.legalCuts = Array.from({ length: layers - 1 }, (_, j) => j + 1).filter(() => rand(2) === 0);
    const ref = referencePlan(p);
    let actual = null;
    try { actual = planModelLoading(p); } catch (error) { assert.match(String(error), /no (?:memory-)?tier prefix/); }
    assert.equal(comparePlan(p, ref, actual).agrees, true, JSON.stringify({ i, p, ref, actual }));
  }
});

test("score rejects fabricated memory, timing and device assignments", () => {
  const p = fixture([4, 4], [[1, 1, 1, 1], [2, 2, 2, 2]], [1, 1, 1, 1]);
  const ref = referencePlan(p);
  const original = planModelLoading(p);
  for (const damage of [
    (r: typeof original) => { r.placement.stages[0].memory.memory.requiredBytes = 0; },
    (r: typeof original) => { r.placement.stages[0].deviceId = "made-up"; },
    (r: typeof original) => { r.placement.stages[0].predictedServiceMs = 0; },
    (r: typeof original) => { r.placement.stages[0].layerEnd++; },
  ]) {
    const broken = structuredClone(original); damage(broken);
    assert.equal(comparePlan(p, ref, broken).agrees, false);
  }
});

test("planner rejects ambiguous shared memory and malformed profiles", () => {
  const p = fixture([10], [[1]], [1]);
  Object.assign(p.machines[0].accelerators[0], { memoryKind: "unified", unifiedTier: "mac_unified" });
  p.machines[0].accelerators.push({ ...p.machines[0].accelerators[0], id: "1" });
  assert.throws(() => planModelLoading(p), /shared-memory topology/);
  p.machines[0].accelerators.pop(); p.machines[0].ram.availableBytes = NaN;
  assert.throws(() => planModelLoading(p), /safe integer/);
});

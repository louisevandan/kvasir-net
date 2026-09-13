import type { MachineSpecification, ModelLoadingPlannerInput } from "../src/model-loading-planner.ts";

export function fixture(capacities: number[], times: number[][], weights: number[]): ModelLoadingPlannerInput {
  const machines: MachineSpecification[] = capacities.map((capacity, i) => ({ id: `h${i}`,
    cpu: { architecture: "fixture", logicalCores: 8 }, ram: { totalBytes: 100, reserveBytes: 0, allowOffload: false },
    accelerators: [{ id: "0", vendor: "fixture", backend: "fixture", memoryKind: "dedicated",
      memoryTotalBytes: capacity, reserveBytes: 0, enabled: true, order: i }] }));
  return { schema: "p4-model-loading-planner-v1", machines,
    model: { id: "literal", fingerprint: "analyst-fixture-v1", architecture: "fixture", fixedBytesPerStage: 0,
      layers: weights.map((weightBytes, index) => ({ index, weightBytes, kvBytesPerTokenPerSequence: 0, runtimeBytes: 0 })) },
    workload: { contextTokens: 1, concurrentSequences: 1 },
    calibrations: times.map((layerServiceMs, i) => ({ poolId: `h${i}:gpu:0`, layerServiceMs, fixedServiceMs: 0, hopServiceMs: 0 })) };
}

/** Literal judgments written before policy comparison; no production result supplies an expectation. */
export function analystJudgments() {
  const cases: Array<{ id: string; reason: string; input: ModelLoadingPlannerInput;
    expected: null | { tier: number; period: number; total: number; stages: number } }> = [];
  const add = (id: string, reason: string, input: ModelLoadingPlannerInput, expected: typeof cases[number]["expected"]) => cases.push({ id, reason, input, expected });
  let p = fixture([2, 1, 1], [[3, 3, 100], [100, 5, 100], [100, 100, 10]], [1, 1, 1]);
  add("later-bottleneck", "A owns layers 0-1 in 6ms; C owns layer 2 in 10ms. Splitting A/B lowers an irrelevant prefix maximum but raises total to 18ms.", p, { tier: 0, period: 10, total: 16, stages: 2 });
  p = fixture([40, 40], [[1, 1, 1, 1], [2, 2, 2, 2]], [10, 10, 10, 10]);
  p.machines[0].accelerators[0].memoryAvailableBytes = 0;
  add("busy-fast-device", "The occupied fast GPU has no capacity; all four layers belong on the slow GPU.", p, { tier: 0, period: 8, total: 8, stages: 1 });
  p = fixture([40], [[1, 1, 1, 1]], [10, 10, 10, 10]);
  p.machines[0].accelerators[0].memoryAvailableBytes = 39;
  add("one-byte-short", "40 bytes cannot fit into 39 available bytes.", p, null);
  p = fixture([40], [[1, 1, 1, 1]], [10, 10, 10, 10]);
  Object.assign(p.machines[0].accelerators[0], { memoryKind: "unified", unifiedTier: "mac_unified" });
  p.machines[0].ram.availableBytes = 39;
  add("shared-ram-pressure", "Apple GPU and host RAM describe one allocation domain; the smaller host availability rules out the plan.", p, null);
  p = fixture([2, 2], [[1, 1, 1], [1, 1, 1]], [1, 1, 1]);
  p.model.legalCuts = [];
  add("indivisible-storage", "No boundary is legal and neither device holds all three layers.", p, null);
  p = fixture([2, 2], [[1, 1, 1], [1, 1, 1]], [1, 1, 1]);
  p.model.fixedBytesPerStage = 1;
  add("repeated-fixed-cost", "Each GPU loses one byte to fixed state, leaving capacity for only two of three layers in total.", p, null);
  p = fixture([10, 10], [[1, 1], [1, 1]], [1, 1]);
  p.machines[0].accelerators.push({ ...p.machines[1].accelerators[0], id: "1" });
  p.machines.pop(); p.calibrations[1].poolId = "h0:gpu:1"; p.constraints = { minimumMachines: 2 };
  add("two-gpus-one-host", "Two devices on one host cannot satisfy two physical machines.", p, null);
  p = fixture([2, 10], [[1, 1], [0, 0]], [1, 1]);
  Object.assign(p.machines[1].accelerators[0], { memoryKind: "unified", unifiedTier: "mac_unified" });
  add("tier-before-speed", "GDDR fits completely; the faster hypothetical Mac cannot override the hard tier preference.", p, { tier: 0, period: 2, total: 2, stages: 1 });
  p = fixture([40, 40], [[1, 1, 1, 1], [2, 2, 2, 2]], [10, 10, 10, 10]);
  p.model.layers.forEach((layer) => layer.kvBytesPerTokenPerSequence = 1);
  p.workload = { contextTokens: 4, concurrentSequences: 8 };
  add("context-times-sessions", "Each indivisible layer needs 10 + 4*8 = 42 bytes, exceeding either 40-byte device despite 80 aggregate bytes.", p, null);
  p = fixture([4, 4], [[1, 1, 1, 1], [2, 2, 2, 2]], [1, 1, 1, 1]);
  p.machines[0].accelerators[0].enabled = false;
  add("operator-disabled", "Disabled hardware remains inventory only; the slow enabled GPU owns all four layers.", p, { tier: 0, period: 8, total: 8, stages: 1 });
  return cases;
}

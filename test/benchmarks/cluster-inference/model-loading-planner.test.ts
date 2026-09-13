import assert from "node:assert/strict";
import test from "node:test";
import { planModelLoading, type MachineSpecification, type ModelLoadingPlannerInput } from "../../../tools/cluster-inference/model-loading-planner.ts";

const machine = (id: string, memory: number, service: number): [MachineSpecification, { poolId: string; layerServiceMs: number[]; fixedServiceMs: number; hopServiceMs: number }] => [{
  id, cpu: { architecture: "test", logicalCores: 8 },
  ram: { totalBytes: 1000, reserveBytes: 100, allowOffload: false },
  accelerators: [{ id: "0", vendor: "test", backend: "test", memoryKind: "dedicated", memoryTotalBytes: memory,
    reserveBytes: 0, enabled: true, order: id === "fast" ? 0 : 1 }],
}, { poolId: `${id}:gpu:0`, layerServiceMs: [service, service, service, service], fixedServiceMs: 0, hopServiceMs: 0 }];

test("the general planner derives KV per layer and returns every decision stage", () => {
  const [fast, fastCalibration] = machine("fast", 40, 1);
  const [slow, slowCalibration] = machine("slow", 40, 2);
  const input: ModelLoadingPlannerInput = {
    schema: "p4-model-loading-planner-v1", machines: [fast, slow],
    model: { id: "model", fingerprint: "sha256", architecture: "fixture", fixedBytesPerStage: 0,
      layers: Array.from({ length: 4 }, (_, index) => ({ index, weightBytes: 10, kvBytesPerTokenPerSequence: 1, runtimeBytes: 0 })) },
    workload: { contextTokens: 2, concurrentSequences: 1 }, calibrations: [fastCalibration, slowCalibration],
    constraints: { minimumMachines: 2 },
  };
  const result = planModelLoading(input);
  assert.deepEqual(result.stages.map((stage) => stage.name), [
    "normalize-hardware", "derive-layer-demand", "coarse-admit-memory-tier", "optimize-contiguous-cuts",
  ]);
  assert.equal(result.admission.requiredBytes, 48);
  assert.deepEqual(result.placement.stages.map((stage) => [stage.deviceId, stage.layerBegin, stage.layerEnd]), [
    ["fast:gpu:0", 0, 3], ["slow:gpu:0", 3, 4],
  ]);
  assert.equal(result.stages[1].detail.kvBytes, 8);
});

test("the planner refuses to guess performance for a machine without calibration", () => {
  const [host] = machine("host", 100, 1);
  assert.throws(() => planModelLoading({
    schema: "p4-model-loading-planner-v1", machines: [host],
    model: { id: "model", fingerprint: "hash", architecture: "fixture", fixedBytesPerStage: 0,
      layers: [{ index: 0, weightBytes: 1, kvBytesPerTokenPerSequence: 1, runtimeBytes: 0 }] },
    workload: { contextTokens: 1, concurrentSequences: 1 }, calibrations: [],
  }), /missing calibrated service profile/);
});

import assert from "node:assert/strict";
import test from "node:test";
import {
  planPlacement,
  type MemoryTier,
  type PlacementDevice,
  type PlacementRequest,
} from "../src/placement-policy.ts";

function device(id: string, tier: MemoryTier, capacity: number, service: number,
  order: number, machineId = id, reserve = 0): PlacementDevice {
  return {
    id,
    machineId,
    tier,
    order,
    pools: { device: { capacityBytes: capacity, reserveBytes: reserve, fixedBytes: 0 } },
    layerMemoryBytes: Array.from({ length: 6 }, () => ({ device: 1 })),
    layerServiceMs: Array.from({ length: 6 }, () => service),
    fixedServiceMs: 0,
    hopServiceMs: 0,
  };
}

function request(devices: PlacementDevice[], minimumMachines = 1): PlacementRequest {
  return { schema: "p4-placement-request-v1", modelId: "fixture", layerCount: 6, minimumMachines, devices };
}

test("measured service time produces an unequal layer split", () => {
  const plan = planPlacement(request([
    device("fast", "gddr", 4, 1, 0),
    device("slow", "gddr", 4, 2, 1),
  ], 2));
  assert.deepEqual(plan.stages.map((stage) => [stage.deviceId, stage.layerBegin, stage.layerEnd]), [
    ["fast", 0, 4],
    ["slow", 4, 6],
  ]);
  assert.equal(plan.predictedPipelinePeriodMs, 4);
});

test("KV and runtime reserve is removed before weights are placed", () => {
  const plan = planPlacement(request([
    device("gddr", "gddr", 4, 1, 0, "pc", 2),
    device("mac", "mac_unified", 4, 1, 1, "mac"),
  ], 2));
  assert.equal(plan.maxTier, "mac_unified");
  assert.deepEqual(plan.stages.map((stage) => stage.layerEnd - stage.layerBegin), [2, 4]);
  assert.equal(plan.stages[0].memory.device.usableBytes, 2);
});

test("a slower tier is excluded when the preceding tier prefix fits", () => {
  const plan = planPlacement(request([
    device("gddr-a", "gddr", 3, 1, 0, "a"),
    device("gddr-b", "gddr", 3, 1, 1, "b"),
    device("mac", "mac_unified", 6, 0.5, 2, "mac"),
    device("gb10", "gb10_unified", 6, 0.25, 3, "spark"),
  ], 2));
  assert.equal(plan.maxTier, "gddr");
  assert.deepEqual(plan.stages.map((stage) => stage.deviceId), ["gddr-a", "gddr-b"]);
  assert.match(plan.excludedDevices.find((entry) => entry.id === "mac")!.reason, /tier not required/);
});

test("Mac then GB10 then DDR are admitted only when the prior prefix cannot fit", () => {
  const plan = planPlacement(request([
    device("gddr", "gddr", 2, 1, 0),
    device("mac", "mac_unified", 1, 1, 1),
    device("spark", "gb10_unified", 1, 1, 2),
    device("host", "ddr_offload", 2, 10, 3),
  ]));
  assert.equal(plan.maxTier, "ddr_offload");
  assert.deepEqual(plan.stages.map((stage) => stage.deviceId), ["gddr", "mac", "spark", "host"]);
});

test("a slow device is omitted when faster devices already meet capacity and machine count", () => {
  const plan = planPlacement(request([
    device("fast-a", "gddr", 3, 1, 0, "a"),
    device("slow", "gddr", 1, 100, 1, "slow"),
    device("fast-b", "gddr", 3, 1, 2, "b"),
  ], 2));
  assert.deepEqual(plan.stages.map((stage) => stage.deviceId), ["fast-a", "fast-b"]);
  assert.match(plan.excludedDevices.find((entry) => entry.id === "slow")!.reason, /bottleneck/);
});

test("missing per-layer measurements are rejected instead of guessed from a GPU name", () => {
  const broken = device("named-only", "gddr", 6, 1, 0);
  broken.layerServiceMs = [];
  assert.throws(() => planPlacement(request([broken])), /one memory and service record per layer/);
});

test("the full 26-accelerator inventory has no exhaustive-subset ceiling", () => {
  const devices = Array.from({ length: 26 }, (_, index) => device(
    `gpu-${index}`, "gddr", 10, index + 1, index, `machine-${Math.floor(index / 3)}`,
  ));
  const plan = planPlacement(request(devices));
  assert.equal(plan.maxTier, "gddr");
  assert.deepEqual(plan.stages.map((stage) => [stage.deviceId, stage.layerBegin, stage.layerEnd]), [
    ["gpu-0", 0, 4],
    ["gpu-1", 4, 6],
  ]);
});

import assert from "node:assert/strict";
import test from "node:test";
import { validateNativeDeployment, type NativeDeploymentInput, type NativeMemoryPlan } from "../index.ts";

const hash = (character: string, length = 64) =>
  (character + "0123456789abcdef").repeat(Math.ceil(length / 17)).slice(0, length);
const plan = (begin: number, end: number, device: string, description: string, required = 300): NativeMemoryPlan => ({
  schema: 2,
  memory_topology: { mode: "host-shared", host_shared_devices: [0] },
  execution_shape: { n_ctx: 819200, n_ctx_seq: 819200, n_batch: 128, n_ubatch: 64,
    n_seq_max: 8, kv_unified: true },
  complete: true,
  fits_current_free: true,
  layer_device_query_supported: true,
  layer_device_expectations_checked: false,
  layer_default_devices: Array.from({ length: end - begin }, (_, index) => ({ layer: begin + index, device })),
  entries: [
    { scope: "device", index: 0, name: device, description, free: 900, total: 1000,
      model: required - 30, context: 20, compute: 10, required },
    { scope: "host", index: -1, name: "host", description: "host memory", free: 900, total: 1000,
      model: 0, context: 0, compute: 10, required: 10 },
  ],
});

function fixture(): NativeDeploymentInput {
  const hosts = ["b", "c", "d"].map((id) => ({ id: hash(id), inspectedUnixMs: 1789400000000,
    pools: [{ id: "shared", availableBytes: 1000, reserveBytes: 100 }] }));
  const cuts = [0, 2, 4, 6];
  return {
    schema: "p4-native-deployment-v1",
    model: { id: "qwen", fingerprint: hash("a"), layerCount: 6, legalCuts: [2, 4] },
    profile: { totalContext: 819200, resident: 8, nBatch: 128, nUbatch: 64, kvUnified: true },
    minimumMachines: 2,
    requireActual: false,
    hosts,
    stages: cuts.slice(1).map((end, index) => {
      const p = plan(cuts[index], end, index === 0 ? "CUDA0" : "MTL0", index === 0 ? "NVIDIA GB10" : "Apple M4 Pro");
      return {
        id: `stage-${index}`,
        hostId: hosts[index].id,
        layerBegin: cuts[index],
        layerEnd: end,
        expectedDeviceDescription: index === 0 ? "NVIDIA GB10" : "Apple M4 Pro",
        expectedLayerDevice: index === 0 ? "CUDA0" : "MTL0",
        entryPools: { "device:0": "shared", "host:-1": "shared" },
        runtime: { binarySha256: hash(String(index + 1)), upstreamCommit: hash("e", 40),
          patchSet: hash("f"), backendInventory: index === 0 ? "CPU[CPU]|CUDA[CUDA0]" : "CPU[CPU]|Metal[MTL0]" },
        plan: p,
      };
    }),
  };
}

test("native PLAN authorizes one contiguous, source-bound, shared-pool deployment", () => {
  const input = fixture();
  const result = validateNativeDeployment(input);
  assert.deepEqual(result.cuts, [0, 2, 4, 6]);
  assert.equal(result.machineCount, 3);
  assert.equal(result.loadAuthorized, true);
  assert.equal(result.actualAllocationConformant, false);
  assert.deepEqual(result.pools.map((pool) => pool.requiredBytes), [310, 310, 310]);
  assert.deepEqual(result.pools.map((pool) => pool.headroomBytes), [590, 590, 590]);
});

test("post-LOAD evidence must reproduce PLAN allocation and placement", () => {
  const input = fixture();
  input.requireActual = true;
  for (const stage of input.stages) stage.actual = structuredClone(stage.plan);
  assert.equal(validateNativeDeployment(input).actualAllocationConformant, true);
  input.stages[1].actual!.entries[0].compute++;
  input.stages[1].actual!.entries[0].required++;
  assert.throws(() => validateNativeDeployment(input), /MEMORY_ACTUAL differs from PLAN/);
});

test("preflight fails closed on every authority and ownership mismatch", () => {
  const mutations: Array<(input: NativeDeploymentInput) => void> = [
    (input) => { input.hosts[0].pools[0].availableBytes = 409; },
    (input) => { input.stages[1].runtime.patchSet = hash("9"); },
    (input) => { input.model.legalCuts = [2]; },
    (input) => { input.stages[1].layerBegin = 3; },
    (input) => { input.stages[1].plan.layer_default_devices[0].device = "CPU"; },
    (input) => { input.stages[1].plan.entries[0].required--; },
    (input) => { delete input.stages[1].entryPools["host:-1"]; },
    (input) => { input.stages[1].entryPools["host:-1"] = "missing"; },
    (input) => { input.minimumMachines = 4; },
  ];
  for (const mutate of mutations) {
    const input = fixture();
    mutate(input);
    assert.throws(() => validateNativeDeployment(input));
  }
});

test("requiring actual evidence never turns a PLAN-only result into runtime acceptance", () => {
  const input = fixture();
  input.requireActual = true;
  assert.throws(() => validateNativeDeployment(input), /post-LOAD allocation evidence is required/);
});

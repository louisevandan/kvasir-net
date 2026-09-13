import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { catalogGgufModels, loadingPoolsFromFleetSnapshot, recommendModelLoad, type LoadingPool } from "../../../tools/cluster-inference/model-loading-policy.ts";

const GIB = 1024 ** 3;
const pool = (id: string, tier: LoadingPool["tier"], usableGiB: number, order: number): LoadingPool => ({
  id, machineId: id, tier, capacityBytes: usableGiB * GIB, reserveBytes: 0, order,
});

test("KV takes the fastest memory before weights and tiers advance only for capacity", () => {
  const result = recommendModelLoad(
    { modelId: "mixed", kvBytes: 6 * GIB, runtimeBytes: 2 * GIB, weightBytes: 10 * GIB },
    [pool("gddr", "gddr", 8, 0), pool("mac", "mac_unified", 16, 1)],
  );
  assert.equal(result.maxTier, "mac_unified");
  assert.deepEqual(result.allocations[0], {
    poolId: "gddr", machineId: "gddr", tier: "gddr", kvBytes: 6 * GIB,
    runtimeBytes: 2 * GIB, weightBytes: 0,
  });
  assert.equal(result.allocations[1].weightBytes, 10 * GIB);
});

test("operator-disabled memory and current occupancy cannot silently approve a load", () => {
  const result = recommendModelLoad(
    { modelId: "safe", kvBytes: 2 * GIB, runtimeBytes: 0, weightBytes: 8 * GIB },
    [
      { ...pool("allowed", "gddr", 12, 0), availableBytes: 9 * GIB },
      { ...pool("display", "gddr", 16, 1), enabled: false },
    ],
  );
  assert.equal(result.maxTier, "gddr");
  assert.equal(result.currentlyAdmissible, false);
  assert.deepEqual(result.allocations.map((entry) => entry.poolId), ["allowed"]);
});

test("the known consumer fleet boundaries reproduce independent model judgments", () => {
  const pools = [
    pool("consumer-gddr", "gddr", 78, 0),
    pool("two-macs", "mac_unified", 108, 1),
    pool("gb10", "gb10_unified", 101, 2),
    pool("host-ddr", "ddr_offload", 900, 3),
  ];
  const cases = [
    ["Laguna 77", 77, "gddr"],
    ["Mistral 80", 80, "mac_unified"],
    ["DeepSeek 151", 151, "mac_unified"],
    ["Hy3 192", 192, "gb10_unified"],
    ["Nex 255", 255, "gb10_unified"],
    ["MiniMax M3 275", 275, "gb10_unified"],
    ["MiniMax M3 278 plus runtime", 278 + 12, "ddr_offload"],
    ["Nemotron 550B", 365, "ddr_offload"],
  ] as const;
  for (const [modelId, demandGiB, expected] of cases) {
    assert.equal(recommendModelLoad({ modelId, weightBytes: demandGiB * GIB, kvBytes: 0, runtimeBytes: 0 }, pools).maxTier, expected);
  }
});

test("all 26 accelerator pools can be considered without an exhaustive-subset ceiling", () => {
  const pools = Array.from({ length: 26 }, (_, index) => pool(`gpu-${index}`, "gddr", 40, index));
  const result = recommendModelLoad(
    { modelId: "large", weightBytes: 900 * GIB, kvBytes: 20 * GIB, runtimeBytes: 0 }, pools,
  );
  assert.equal(result.maxTier, "gddr");
  assert.equal(result.allocations.length, 23);
});

test("catalog groups split GGUFs, marks missing parts, and excludes mmproj", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "p4-model-catalog-"));
  try {
    fs.writeFileSync(path.join(root, "model-00001-of-00002.gguf"), Buffer.alloc(3));
    fs.writeFileSync(path.join(root, "model-00002-of-00002.gguf"), Buffer.alloc(5));
    fs.writeFileSync(path.join(root, "broken-00002-of-00003.gguf"), Buffer.alloc(7));
    fs.writeFileSync(path.join(root, "mmproj-F32.gguf"), Buffer.alloc(11));
    const models = catalogGgufModels(root);
    assert.deepEqual(models.map(({ modelId, weightBytes, complete }) => ({ modelId, weightBytes, complete })), [
      { modelId: "model.gguf", weightBytes: 8, complete: true },
      { modelId: "broken.gguf", weightBytes: 7, complete: false },
    ]);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("fleet inventory classifies dedicated, Apple and GB10 memory and does not double-count unified RAM", () => {
  const machine = (vendor: string, name: string, memoryKind: string, memoryBytes: number) => ({
    machine: {
      capability: { gpus: [{ index: 0, uuid: name, vendor, name, memory_kind: memoryKind, memory_total_bytes: memoryBytes }], memory: { total_bytes: memoryBytes } },
      occupancy: { gpus: [{ uuid: name, memory_free_bytes: memoryBytes - 1 }], memory: { available_bytes: memoryBytes - 2 } },
    },
  });
  const pools = loadingPoolsFromFleetSnapshot({ machines: {
    pc: machine("NVIDIA", "RTX 3090", "dedicated", 24 * GIB),
    mac: machine("Apple", "Apple M4 Pro", "unified", 64 * GIB),
    spark: machine("NVIDIA", "NVIDIA GB10", "unified", 120 * GIB),
  } }, { excludedDeviceIds: ["pc:gpu:0"] });
  assert.deepEqual(pools.map(({ id, tier, enabled }) => [id, tier, enabled]), [
    ["mac:gpu:0", "mac_unified", true],
    ["pc:gpu:0", "gddr", false],
    ["pc:ram", "ddr_offload", undefined],
    ["spark:gpu:0", "gb10_unified", true],
  ]);
});

test("the canonical fleet inventory array is accepted by the general loading function", () => {
  const snapshot = { machines: [{
    machineId: "host",
    capability: { gpus: [{ index: 0, uuid: "gpu", vendor: "AMD", name: "MI250", memory_kind: "dedicated", memory_total_bytes: 64 * GIB }], memory: { total_bytes: 128 * GIB } },
    occupancy: { gpus: [{ uuid: "gpu", memory_free_bytes: 60 * GIB }], memory: { available_bytes: 100 * GIB } },
  }] };
  const pools = loadingPoolsFromFleetSnapshot(snapshot, { reserveBytes: { gddr: 2 * GIB } });
  assert.equal(recommendModelLoad(
    { modelId: "profile", weightBytes: 50 * GIB, kvBytes: 8 * GIB, runtimeBytes: 2 * GIB }, pools,
  ).maxTier, "gddr");
  assert.deepEqual(pools.map((entry) => entry.id), ["host:gpu:0", "host:ram"]);
});

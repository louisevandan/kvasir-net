import fs from "node:fs";
import path from "node:path";

export type LoadingTier = "gddr" | "mac_unified" | "gb10_unified" | "ddr_offload";

export type LoadingPool = {
  id: string;
  machineId: string;
  tier: LoadingTier;
  capacityBytes: number;
  reserveBytes: number;
  availableBytes?: number;
  order: number;
  enabled?: boolean;
};

export type ModelLoadDemand = {
  modelId: string;
  weightBytes: number;
  kvBytes: number;
  runtimeBytes: number;
};

export type PoolAllocation = {
  poolId: string;
  machineId: string;
  tier: LoadingTier;
  kvBytes: number;
  weightBytes: number;
  runtimeBytes: number;
};

export type ModelLoadRecommendation = {
  modelId: string;
  maxTier: LoadingTier;
  requiredBytes: number;
  usableBytes: number;
  currentlyAdmissible: boolean;
  allocations: PoolAllocation[];
};

export type CatalogModel = {
  modelId: string;
  weightBytes: number;
  files: string[];
  complete: boolean;
};

export type FleetPoolOptions = {
  excludedDeviceIds?: string[];
  excludedMachineIds?: string[];
  reserveBytes?: Partial<Record<LoadingTier, number>>;
  includeHostDdr?: boolean;
};

export const LOADING_TIERS: LoadingTier[] = ["gddr", "mac_unified", "gb10_unified", "ddr_offload"];

function nonNegative(value: number, label: string): void {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error(`${label} must be a non-negative safe integer`);
}

function usable(pool: LoadingPool, current: boolean): number {
  const capacity = current && pool.availableBytes !== undefined
    ? Math.min(pool.capacityBytes, pool.availableBytes)
    : pool.capacityBytes;
  return Math.max(0, capacity - pool.reserveBytes);
}

function allocate(demand: ModelLoadDemand, pools: LoadingPool[], current: boolean): PoolAllocation[] | null {
  const remaining = { kvBytes: demand.kvBytes, runtimeBytes: demand.runtimeBytes, weightBytes: demand.weightBytes };
  const allocations = pools.map((pool) => ({
    poolId: pool.id,
    machineId: pool.machineId,
    tier: pool.tier,
    kvBytes: 0,
    runtimeBytes: 0,
    weightBytes: 0,
    free: usable(pool, current),
  }));
  // KV has first claim on the fastest memory, followed by non-weight runtime
  // state. Weights consume only the residual capacity.
  for (const kind of ["kvBytes", "runtimeBytes", "weightBytes"] as const) {
    for (const allocation of allocations) {
      const bytes = Math.min(allocation.free, remaining[kind]);
      allocation[kind] += bytes;
      allocation.free -= bytes;
      remaining[kind] -= bytes;
      if (remaining[kind] === 0) break;
    }
    if (remaining[kind] !== 0) return null;
  }
  return allocations
    .filter((allocation) => allocation.kvBytes + allocation.runtimeBytes + allocation.weightBytes > 0)
    .map(({ free: _free, ...allocation }) => allocation);
}

export function recommendModelLoad(
  demand: ModelLoadDemand,
  pools: LoadingPool[],
  tierOrder: LoadingTier[] = LOADING_TIERS,
): ModelLoadRecommendation {
  nonNegative(demand.weightBytes, "weightBytes");
  nonNegative(demand.kvBytes, "kvBytes");
  nonNegative(demand.runtimeBytes, "runtimeBytes");
  if (!demand.modelId) throw new Error("modelId is required");
  if (new Set(tierOrder).size !== tierOrder.length || tierOrder.some((tier) => !LOADING_TIERS.includes(tier))) {
    throw new Error("tierOrder must contain unique known tiers");
  }
  const ids = new Set<string>();
  for (const pool of pools) {
    if (!pool.id || ids.has(pool.id)) throw new Error(`pool id is empty or duplicated: ${pool.id}`);
    ids.add(pool.id);
    if (!tierOrder.includes(pool.tier)) throw new Error(`pool ${pool.id} has an unknown tier`);
    nonNegative(pool.capacityBytes, `${pool.id}.capacityBytes`);
    nonNegative(pool.reserveBytes, `${pool.id}.reserveBytes`);
    if (pool.availableBytes !== undefined) nonNegative(pool.availableBytes, `${pool.id}.availableBytes`);
    if (pool.reserveBytes > pool.capacityBytes) throw new Error(`${pool.id} reserve exceeds capacity`);
  }
  const enabled = pools.filter((pool) => pool.enabled !== false);
  const requiredBytes = demand.weightBytes + demand.kvBytes + demand.runtimeBytes;
  for (let tierIndex = 0; tierIndex < tierOrder.length; tierIndex += 1) {
    const allowed = new Set(tierOrder.slice(0, tierIndex + 1));
    const candidates = enabled
      .filter((pool) => allowed.has(pool.tier))
      .sort((left, right) => tierOrder.indexOf(left.tier) - tierOrder.indexOf(right.tier)
        || left.order - right.order || left.id.localeCompare(right.id));
    const allocations = allocate(demand, candidates, false);
    if (!allocations) continue;
    return {
      modelId: demand.modelId,
      maxTier: tierOrder[tierIndex],
      requiredBytes,
      usableBytes: candidates.reduce((sum, pool) => sum + usable(pool, false), 0),
      currentlyAdmissible: allocate(demand, candidates, true) !== null,
      allocations,
    };
  }
  throw new Error(`no memory-tier prefix can hold ${demand.modelId} (${requiredBytes} bytes)`);
}

export function catalogGgufModels(root: string): CatalogModel[] {
  const groups = new Map<string, { files: string[]; bytes: number; expected: number | null; parts: Set<number> }>();
  const visit = (directory: string): void => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const full = path.join(directory, entry.name);
      if (entry.isDirectory()) visit(full);
      else if (entry.isFile() && entry.name.toLowerCase().endsWith(".gguf") && !entry.name.toLowerCase().startsWith("mmproj")) {
        const match = /^(.*)-(\d{5})-of-(\d{5})\.gguf$/i.exec(entry.name);
        const logicalName = match ? `${match[1]}.gguf` : entry.name;
        const modelId = path.relative(root, path.join(directory, logicalName)).replaceAll("\\", "/");
        const group = groups.get(modelId) ?? { files: [], bytes: 0, expected: match ? Number(match[3]) : 1, parts: new Set<number>() };
        if ((match ? Number(match[3]) : 1) !== group.expected) throw new Error(`${modelId} has inconsistent split counts`);
        group.files.push(full);
        group.bytes += fs.statSync(full).size;
        group.parts.add(match ? Number(match[2]) : 1);
        groups.set(modelId, group);
      }
    }
  };
  visit(root);
  return [...groups.entries()].map(([modelId, group]) => ({
    modelId,
    weightBytes: group.bytes,
    files: group.files.sort(),
    complete: group.expected === group.parts.size
      && [...group.parts].every((part, index) => part === index + 1),
  })).sort((left, right) => right.weightBytes - left.weightBytes || left.modelId.localeCompare(right.modelId));
}

type JsonRecord = Record<string, unknown>;

function record(value: unknown, label: string): JsonRecord {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label} must be an object`);
  return value as JsonRecord;
}

function integer(value: unknown, label: string): number {
  if (typeof value !== "number") throw new Error(`${label} is missing`);
  nonNegative(value, label);
  return value;
}

/** Convert a persisted p4-agent fleet snapshot into coarse load-admission pools. */
export function loadingPoolsFromFleetSnapshot(snapshot: unknown, options: FleetPoolOptions = {}): LoadingPool[] {
  const root = record(snapshot, "snapshot");
  const rawMachines = root.machines;
  const machines: Array<[string, JsonRecord]> = Array.isArray(rawMachines)
    ? rawMachines.map((value, index) => {
      const item = record(value, `snapshot.machines[${index}]`);
      if (typeof item.machineId !== "string" || !item.machineId) throw new Error(`snapshot.machines[${index}].machineId is required`);
      return [item.machineId, { machine: { capability: item.capability, occupancy: item.occupancy } }];
    })
    : Object.entries(record(rawMachines, "snapshot.machines")).map(([id, value]) => [id, record(value, `snapshot.machines.${id}`)]);
  const excludedDevices = new Set(options.excludedDeviceIds ?? []);
  const excludedMachines = new Set(options.excludedMachineIds ?? []);
  const reserves = options.reserveBytes ?? {};
  const pools: LoadingPool[] = [];
  let order = 0;
  for (const [machineId, rawMachine] of machines.sort(([left], [right]) => left.localeCompare(right))) {
    if (excludedMachines.has(machineId)) continue;
    const machine = record(rawMachine, `machines.${machineId}`);
    const machineReport = record(machine.machine, `machines.${machineId}.machine`);
    const capability = record(machineReport.capability, `machines.${machineId}.machine.capability`);
    const occupancy = record(machineReport.occupancy, `machines.${machineId}.machine.occupancy`);
    const rawGpus = capability.gpus;
    if (!Array.isArray(rawGpus)) throw new Error(`machines.${machineId}.capability.gpus must be an array`);
    const occupancyGpus = Array.isArray(occupancy.gpus) ? occupancy.gpus.map((value, index) => record(value, `${machineId}.occupancy.gpus[${index}]`)) : [];
    let hasUnified = false;
    for (const [gpuOffset, rawGpu] of rawGpus.entries()) {
      const gpu = record(rawGpu, `${machineId}.gpus[${gpuOffset}]`);
      const index = integer(gpu.index, `${machineId}.gpus[${gpuOffset}].index`);
      const id = `${machineId}:gpu:${index}`;
      const memoryKind = String(gpu.memory_kind ?? "").toLowerCase();
      const vendor = String(gpu.vendor ?? "").toLowerCase();
      const name = String(gpu.name ?? "").toLowerCase();
      let tier: LoadingTier;
      if (memoryKind === "dedicated") tier = "gddr";
      else if (memoryKind === "unified" && vendor.includes("apple")) tier = "mac_unified";
      else if (memoryKind === "unified" && (vendor.includes("nvidia") || name.includes("gb10"))) tier = "gb10_unified";
      else throw new Error(`${id} has unclassified memory kind/vendor: ${memoryKind}/${vendor}`);
      hasUnified ||= memoryKind === "unified";
      const capacityBytes = integer(gpu.memory_total_bytes, `${id}.memory_total_bytes`);
      const uuid = String(gpu.uuid ?? "");
      const current = occupancyGpus.find((entry) => String(entry.uuid ?? "") === uuid) ?? occupancyGpus[gpuOffset];
      const availableValue = current?.memory_free_bytes ?? current?.vram_free_bytes;
      pools.push({
        id, machineId, tier, capacityBytes,
        reserveBytes: reserves[tier] ?? 0,
        availableBytes: availableValue === undefined ? undefined : integer(availableValue, `${id}.availableBytes`),
        enabled: !excludedDevices.has(id), order: order++,
      });
    }
    if (options.includeHostDdr !== false && !hasUnified) {
      const memory = record(capability.memory, `${machineId}.capability.memory`);
      const occupancyMemory = record(occupancy.memory, `${machineId}.occupancy.memory`);
      pools.push({
        id: `${machineId}:ram`, machineId, tier: "ddr_offload",
        capacityBytes: integer(memory.total_bytes, `${machineId}.memory.total_bytes`),
        reserveBytes: reserves.ddr_offload ?? 0,
        availableBytes: integer(occupancyMemory.available_bytes, `${machineId}.memory.available_bytes`),
        order: order++,
      });
    }
  }
  return pools;
}

/**
 * OUTER policy for placing a staged model on a heterogeneous fleet.
 *
 * The policy consumes measured bytes and measured service time. It never
 * infers either from a GPU name. Memory tiers are a hard lexicographic gate;
 * layer cuts are then chosen by minimising the slowest predicted stage.
 */

export type MemoryTier = "gddr" | "mac_unified" | "gb10_unified" | "ddr_offload";

export type MemoryPool = {
  capacityBytes: number;
  reserveBytes: number;
  fixedBytes: number;
};

export type PlacementDevice = {
  id: string;
  machineId: string;
  order: number;
  tier: MemoryTier;
  pools: Record<string, MemoryPool>;
  /** Exact PLAN/actual bytes for each model layer and pool. */
  layerMemoryBytes: Array<Record<string, number>>;
  /** Calibrated stage service contribution for the target workload mix. */
  layerServiceMs: number[];
  fixedServiceMs: number;
  hopServiceMs: number;
};

export type PlacementRequest = {
  schema: "p4-placement-request-v1";
  modelId: string;
  layerCount: number;
  minimumMachines?: number;
  devices: PlacementDevice[];
  tierOrder?: MemoryTier[];
  /** Allowed interior boundaries from the adapter's memory topology. Absent means all. */
  legalCuts?: number[];
};

export type StagePlacement = {
  deviceId: string;
  machineId: string;
  tier: MemoryTier;
  layerBegin: number;
  layerEnd: number;
  predictedServiceMs: number;
  memory: Record<string, { requiredBytes: number; usableBytes: number }>;
};

export type PlacementPlan = {
  schema: "p4-placement-plan-v1";
  modelId: string;
  maxTier: MemoryTier;
  maxTierIndex: number;
  predictedPipelinePeriodMs: number;
  stages: StagePlacement[];
  excludedDevices: Array<{ id: string; reason: string }>;
};

const DEFAULT_TIERS: MemoryTier[] = ["gddr", "mac_unified", "gb10_unified", "ddr_offload"];
const EPSILON = 1e-9;

export class PlacementInfeasibleError extends Error {}

type Candidate = { stages: StagePlacement[]; bottleneck: number; total: number };

function finiteNonNegative(value: number, label: string): void {
  if (!Number.isFinite(value) || value < 0) throw new Error(`${label} must be finite and non-negative`);
}

function validate(request: PlacementRequest): MemoryTier[] {
  if (request.schema !== "p4-placement-request-v1") throw new Error("unsupported placement request schema");
  if (!Number.isSafeInteger(request.layerCount) || request.layerCount < 1) throw new Error("layerCount must be positive");
  const tiers = request.tierOrder ?? DEFAULT_TIERS;
  if (new Set(tiers).size !== tiers.length || tiers.some((tier) => !DEFAULT_TIERS.includes(tier))) {
    throw new Error("tierOrder must contain unique known tiers");
  }
  const ids = new Set<string>();
  for (const cut of request.legalCuts ?? []) {
    if (!Number.isSafeInteger(cut) || cut < 1 || cut >= request.layerCount) throw new Error("invalid legal cut");
  }
  for (const device of request.devices) {
    if (!device.id || ids.has(device.id)) throw new Error(`device id is empty or duplicated: ${device.id}`);
    ids.add(device.id);
    if (!device.machineId || !Number.isSafeInteger(device.order)) throw new Error("device machineId and integer order are required");
    if (!Object.keys(device.pools).length) throw new Error("device requires at least one memory pool");
    if (!tiers.includes(device.tier)) throw new Error(`device ${device.id} uses a tier absent from tierOrder`);
    if (device.layerMemoryBytes.length !== request.layerCount || device.layerServiceMs.length !== request.layerCount) {
      throw new Error(`device ${device.id} does not have one memory and service record per layer`);
    }
    finiteNonNegative(device.fixedServiceMs, `${device.id}.fixedServiceMs`);
    finiteNonNegative(device.hopServiceMs, `${device.id}.hopServiceMs`);
    for (const [poolName, pool] of Object.entries(device.pools)) {
      finiteNonNegative(pool.capacityBytes, `${device.id}.${poolName}.capacityBytes`);
      finiteNonNegative(pool.reserveBytes, `${device.id}.${poolName}.reserveBytes`);
      finiteNonNegative(pool.fixedBytes, `${device.id}.${poolName}.fixedBytes`);
    }
    for (let layer = 0; layer < request.layerCount; layer += 1) {
      finiteNonNegative(device.layerServiceMs[layer], `${device.id}.layerServiceMs[${layer}]`);
      for (const [poolName, bytes] of Object.entries(device.layerMemoryBytes[layer])) {
        if (!(poolName in device.pools)) throw new Error(`device ${device.id} layer ${layer} names unknown pool ${poolName}`);
        finiteNonNegative(bytes, `${device.id}.layerMemoryBytes[${layer}].${poolName}`);
      }
    }
  }
  return tiers;
}

function better(left: Candidate | null, right: Candidate): Candidate {
  if (!left) return right;
  if (right.bottleneck < left.bottleneck - EPSILON) return right;
  if (right.bottleneck > left.bottleneck + EPSILON) return left;
  if (right.total < left.total - EPSILON) return right;
  if (right.total > left.total + EPSILON) return left;
  return right.stages.length < left.stages.length ? right : left;
}

/** Exact ordered-subset and contiguous-cut search without a 2^device fleet ceiling. */
function partitionOptional(devices: PlacementDevice[], layerCount: number, minimumMachines: number,
  legalCuts?: number[], ceiling?: number): Candidate | null {
  const machines = [...new Set(devices.map((device) => device.machineId))];
  const machineBits = new Map(machines.map((machine, index) => [machine, 1n << BigInt(index)]));
  const satisfiedMask = -1n;
  if (machines.length < minimumMachines || layerCount < minimumMachines) return null;
  const boundaries = new Set(legalCuts ?? Array.from({ length: layerCount - 1 }, (_, i) => i + 1));
  const addMachine = (mask: bigint, bit: bigint): bigint => {
    if (mask === satisfiedMask) return mask;
    const next = mask | bit;
    let value = next;
    let count = 0;
    while (value !== 0n && count < minimumMachines) {
      value &= value - 1n;
      count += 1;
    }
    return count >= minimumMachines ? satisfiedMask : next;
  };
  type State = { end: number; machineMask: bigint; candidate: Candidate };
  let states = new Map<string, State>();
  states.set("0:0", { end: 0, machineMask: 0n, candidate: { stages: [], bottleneck: 0, total: 0 } });
  for (const device of devices) {
    const memoryPrefix = Object.fromEntries(Object.keys(device.pools).map((name) => {
      const values = [0];
      for (const layer of device.layerMemoryBytes) values.push(values.at(-1)! + (layer[name] ?? 0));
      return [name, values];
    }));
    const servicePrefix = [0];
    for (const value of device.layerServiceMs) servicePrefix.push(servicePrefix.at(-1)! + value);
    const next = new Map(states);
    for (const state of states.values()) {
      for (let end = state.end + 1; end <= layerCount; end += 1) {
        const memory: StagePlacement["memory"] = {};
        let fits = true;
        for (const [name, pool] of Object.entries(device.pools)) {
          const requiredBytes = pool.fixedBytes + memoryPrefix[name][end] - memoryPrefix[name][state.end];
          const usableBytes = Math.max(0, pool.capacityBytes - pool.reserveBytes);
          if (requiredBytes > usableBytes) { fits = false; break; }
          memory[name] = { requiredBytes, usableBytes };
        }
        if (!fits) break;
        if (end !== layerCount && !boundaries.has(end)) continue;
        const predictedServiceMs = device.fixedServiceMs + device.hopServiceMs + servicePrefix[end] - servicePrefix[state.end];
        if (ceiling !== undefined && predictedServiceMs > ceiling + EPSILON) continue;
        const nextStage: StagePlacement = { deviceId: device.id, machineId: device.machineId, tier: device.tier,
          layerBegin: state.end, layerEnd: end, predictedServiceMs, memory };
        const machineMask = addMachine(state.machineMask, machineBits.get(device.machineId)!);
        const candidate = {
          stages: [...state.candidate.stages, nextStage],
          bottleneck: Math.max(state.candidate.bottleneck, nextStage.predictedServiceMs),
          total: state.candidate.total + nextStage.predictedServiceMs,
        };
        const key = `${end}:${machineMask.toString(16)}`;
        const previous = next.get(key);
        // Pass one preserves the minimum maximum; pass two fixes that maximum
        // and minimises the additive total. A single lexicographic prefix is unsafe.
        const chosen = ceiling === undefined ? better(previous?.candidate ?? null, candidate)
          : !previous || candidate.total < previous.candidate.total - EPSILON
            || (Math.abs(candidate.total - previous.candidate.total) <= EPSILON && candidate.stages.length < previous.candidate.stages.length)
            ? candidate : previous.candidate;
        next.set(key, { end, machineMask, candidate: chosen });
      }
    }
    states = next;
  }
  let result: Candidate | null = null;
  for (const state of states.values()) {
    if (state.end !== layerCount) continue;
    if (state.machineMask !== satisfiedMask) continue;
    result = better(result, state.candidate);
  }
  return result;
}

export function planPlacement(request: PlacementRequest): PlacementPlan {
  const tiers = validate(request);
  const minimumMachines = request.minimumMachines ?? 1;
  if (!Number.isSafeInteger(minimumMachines) || minimumMachines < 1) throw new Error("minimumMachines must be positive");
  for (let maxTierIndex = 0; maxTierIndex < tiers.length; maxTierIndex += 1) {
    const permittedTiers = new Set(tiers.slice(0, maxTierIndex + 1));
    const permitted = request.devices
      .filter((device) => permittedTiers.has(device.tier))
      .sort((left, right) => left.order - right.order || left.id.localeCompare(right.id));
    const primary = partitionOptional(permitted, request.layerCount, minimumMachines, request.legalCuts);
    if (!primary) continue;
    const best = partitionOptional(permitted, request.layerCount, minimumMachines, request.legalCuts, primary.bottleneck)!;
    const selectedIds = new Set(best.stages.map((entry) => entry.deviceId));
    return {
      schema: "p4-placement-plan-v1",
      modelId: request.modelId,
      maxTier: tiers[maxTierIndex],
      maxTierIndex,
      predictedPipelinePeriodMs: best.bottleneck,
      stages: best.stages,
      excludedDevices: request.devices
        .filter((device) => !selectedIds.has(device.id))
        .map((device) => ({
          id: device.id,
          reason: tiers.indexOf(device.tier) > maxTierIndex ? "slower memory tier not required" : "not selected by measured bottleneck objective",
        })),
    };
  }
  throw new PlacementInfeasibleError("no tier prefix can place every layer within the supplied memory limits and legal cuts");
}

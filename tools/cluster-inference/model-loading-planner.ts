import { recommendModelLoad, type LoadingPool, type LoadingTier, type ModelLoadRecommendation } from "./model-loading-policy.ts";
import { planPlacement, type PlacementPlan, type PlacementRequest } from "./placement-policy.ts";

export type AcceleratorSpecification = {
  id: string;
  vendor: string;
  backend: "cuda" | "rocm" | "metal" | string;
  architecture?: string;
  computeUnits?: number;
  peakComputeFlops?: number;
  memoryBandwidthBytesPerSecond?: number;
  memoryKind: "dedicated" | "unified";
  unifiedTier?: "mac_unified" | "gb10_unified";
  memoryTotalBytes: number;
  memoryAvailableBytes?: number;
  reserveBytes: number;
  enabled: boolean;
  order: number;
};

export type MachineSpecification = {
  id: string;
  cpu: {
    architecture: string;
    logicalCores: number;
    physicalCores?: number;
    memoryBandwidthBytesPerSecond?: number;
  };
  ram: {
    totalBytes: number;
    availableBytes?: number;
    reserveBytes: number;
    allowOffload: boolean;
  };
  accelerators: AcceleratorSpecification[];
  links?: Array<{ toMachineId: string; bandwidthBytesPerSecond: number; latencyMs: number }>;
};

export type ModelLayerDefinition = {
  index: number;
  kind?: "attention" | "sparse_attention" | "recurrent" | "expert" | "hybrid" | string;
  weightBytes: number;
  /** Exact KV bytes contributed by this layer for one token in one sequence. */
  kvBytesPerTokenPerSequence: number;
  /** Recurrent, hybrid, graph and other layer-local state at the target workload. */
  runtimeBytes: number;
};

export type ModelLoadingDefinition = {
  id: string;
  fingerprint: string;
  architecture: string;
  features?: { attention?: string; expertCount?: number; activeExpertCount?: number; recurrent?: boolean };
  layers: ModelLayerDefinition[];
  /** PLAN-measured non-layer bytes required by every selected stage. */
  fixedBytesPerStage: number;
};

export type DeviceCalibration = {
  poolId: string;
  layerServiceMs: number[];
  fixedServiceMs: number;
  hopServiceMs: number;
};

export type ModelLoadingPlannerInput = {
  schema: "p4-model-loading-planner-v1";
  machines: MachineSpecification[];
  model: ModelLoadingDefinition;
  workload: { contextTokens: number; concurrentSequences: number };
  calibrations: DeviceCalibration[];
  constraints?: { minimumMachines?: number; tierOrder?: LoadingTier[] };
};

export type ModelLoadingPlannerResult = {
  schema: "p4-model-loading-plan-v1";
  modelId: string;
  stages: Array<{ name: string; detail: Record<string, unknown> }>;
  admission: ModelLoadRecommendation;
  placement: PlacementPlan;
};

function safe(value: number, label: string, positive = false): void {
  if (!Number.isSafeInteger(value) || value < (positive ? 1 : 0)) throw new Error(`${label} must be a ${positive ? "positive" : "non-negative"} safe integer`);
}

function tier(accelerator: AcceleratorSpecification): LoadingTier {
  if (accelerator.memoryKind === "dedicated") return "gddr";
  if (accelerator.unifiedTier) return accelerator.unifiedTier;
  throw new Error(`${accelerator.id} unified memory needs unifiedTier`);
}

/**
 * General staged planner. Hardware names are evidence only: memory comes from
 * specifications, layer demand comes from the model profile and workload, and
 * performance comes from calibration for the same model/workload family.
 */
export function planModelLoading(input: ModelLoadingPlannerInput): ModelLoadingPlannerResult {
  if (input.schema !== "p4-model-loading-planner-v1") throw new Error("unsupported model loading planner schema");
  safe(input.workload.contextTokens, "contextTokens", true);
  safe(input.workload.concurrentSequences, "concurrentSequences", true);
  safe(input.model.fixedBytesPerStage, "fixedBytesPerStage");
  if (!input.model.id || !input.model.fingerprint || !input.model.layers.length) throw new Error("model identity, fingerprint and layers are required");
  input.model.layers.forEach((layer, index) => {
    if (layer.index !== index) throw new Error("model layers must be contiguous and zero based");
    safe(layer.weightBytes, `layers[${index}].weightBytes`);
    safe(layer.kvBytesPerTokenPerSequence, `layers[${index}].kvBytesPerTokenPerSequence`);
    safe(layer.runtimeBytes, `layers[${index}].runtimeBytes`);
  });

  const poolIds = new Set<string>();
  const machineIds = new Set<string>();
  const pools: LoadingPool[] = [];
  for (const machine of input.machines) {
    if (!machine.id || machineIds.has(machine.id)) throw new Error(`machine id is empty or duplicated: ${machine.id}`);
    machineIds.add(machine.id);
    safe(machine.cpu.logicalCores, `${machine.id}.cpu.logicalCores`, true);
    safe(machine.ram.totalBytes, `${machine.id}.ram.totalBytes`, true);
    safe(machine.ram.reserveBytes, `${machine.id}.ram.reserveBytes`);
    for (const link of machine.links ?? []) {
      if (!link.toMachineId) throw new Error(`${machine.id} link target is required`);
      safe(link.bandwidthBytesPerSecond, `${machine.id}.link.bandwidthBytesPerSecond`, true);
      if (!Number.isFinite(link.latencyMs) || link.latencyMs < 0) throw new Error(`${machine.id}.link.latencyMs must be finite and non-negative`);
    }
    const hasUnified = machine.accelerators.some((accelerator) => accelerator.memoryKind === "unified");
    for (const accelerator of machine.accelerators) {
      const id = `${machine.id}:gpu:${accelerator.id}`;
      if (poolIds.has(id)) throw new Error(`duplicate pool id: ${id}`);
      poolIds.add(id);
      safe(accelerator.memoryTotalBytes, `${id}.memoryTotalBytes`, true);
      safe(accelerator.reserveBytes, `${id}.reserveBytes`);
      pools.push({ id, machineId: machine.id, tier: tier(accelerator), capacityBytes: accelerator.memoryTotalBytes,
        availableBytes: accelerator.memoryAvailableBytes, reserveBytes: accelerator.reserveBytes,
        enabled: accelerator.enabled, order: accelerator.order });
    }
    if (machine.ram.allowOffload && !hasUnified) {
      const id = `${machine.id}:ram`;
      pools.push({ id, machineId: machine.id, tier: "ddr_offload", capacityBytes: machine.ram.totalBytes,
        availableBytes: machine.ram.availableBytes, reserveBytes: machine.ram.reserveBytes, order: Number.MAX_SAFE_INTEGER });
      poolIds.add(id);
    }
  }

  const multiplier = input.workload.contextTokens * input.workload.concurrentSequences;
  safe(multiplier, "contextTokens times concurrentSequences", true);
  const layerDemand = input.model.layers.map((layer) => ({
    weightBytes: layer.weightBytes,
    kvBytes: layer.kvBytesPerTokenPerSequence * multiplier,
    runtimeBytes: layer.runtimeBytes,
  }));
  layerDemand.forEach((layer, index) => {
    safe(layer.kvBytes, `computed layers[${index}].kvBytes`);
  });
  const weightBytes = layerDemand.reduce((sum, layer) => sum + layer.weightBytes, 0);
  const kvBytes = layerDemand.reduce((sum, layer) => sum + layer.kvBytes, 0);
  const runtimeBytes = layerDemand.reduce((sum, layer) => sum + layer.runtimeBytes, 0) + input.model.fixedBytesPerStage;
  const tierOrder = input.constraints?.tierOrder;
  const admission = recommendModelLoad({ modelId: input.model.id, weightBytes, kvBytes, runtimeBytes }, pools, tierOrder);

  const calibrationByPool = new Map(input.calibrations.map((calibration) => [calibration.poolId, calibration]));
  if (calibrationByPool.size !== input.calibrations.length) throw new Error("duplicate device calibration");
  const missing = pools.filter((pool) => pool.enabled !== false && !calibrationByPool.has(pool.id)).map((pool) => pool.id);
  if (missing.length) throw new Error(`missing calibrated service profile for: ${missing.join(", ")}`);
  const devices = pools.filter((pool) => pool.enabled !== false).map((pool) => {
    const calibration = calibrationByPool.get(pool.id)!;
    if (calibration.layerServiceMs.length !== layerDemand.length) throw new Error(`${pool.id} calibration layer count differs from model`);
    return {
      id: pool.id, machineId: pool.machineId, order: pool.order, tier: pool.tier,
      pools: { memory: { capacityBytes: pool.capacityBytes, reserveBytes: pool.reserveBytes, fixedBytes: input.model.fixedBytesPerStage } },
      layerMemoryBytes: layerDemand.map((layer) => ({ memory: layer.weightBytes + layer.kvBytes + layer.runtimeBytes })),
      layerServiceMs: calibration.layerServiceMs,
      fixedServiceMs: calibration.fixedServiceMs,
      hopServiceMs: calibration.hopServiceMs,
    };
  });
  const placementRequest: PlacementRequest = {
    schema: "p4-placement-request-v1", modelId: input.model.id, layerCount: input.model.layers.length,
    minimumMachines: input.constraints?.minimumMachines, devices, tierOrder,
  };
  const placement = planPlacement(placementRequest);
  return {
    schema: "p4-model-loading-plan-v1", modelId: input.model.id,
    stages: [
      { name: "normalize-hardware", detail: { machines: input.machines.length, memoryPools: pools.length } },
      { name: "derive-layer-demand", detail: { layers: layerDemand.length, weightBytes, kvBytes, runtimeBytes } },
      { name: "coarse-admit-memory-tier", detail: { maxTier: admission.maxTier, currentlyAdmissible: admission.currentlyAdmissible,
        note: "Exact repeated per-stage fixed memory is enforced by placement." } },
      { name: "optimize-contiguous-cuts", detail: { selectedStages: placement.stages.length, predictedPipelinePeriodMs: placement.predictedPipelinePeriodMs } },
    ], admission, placement,
  };
}

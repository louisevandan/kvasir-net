import type { MachineSpecification, ModelLoadingDefinition, ModelLoadingPlannerInput } from "../src/model-loading-planner.ts";

const GIB = 1024 ** 3;
// Deliberately hypothetical capacity classes, not claims about measured fleet speed.
const CLASSES = {
  laptop12: { gpu: 12, ram: 32, cards: 1, tier: "gddr", cost: 4 },
  gpu24: { gpu: 24, ram: 128, cards: 1, tier: "gddr", cost: 2 },
  dual24: { gpu: 24, ram: 256, cards: 2, tier: "gddr", cost: 2 },
  hbm64x4: { gpu: 64, ram: 512, cards: 4, tier: "gddr", cost: 1 },
  mac64: { gpu: 64, ram: 64, cards: 1, tier: "mac_unified", cost: 3 },
  gb10128: { gpu: 128, ram: 128, cards: 1, tier: "gb10_unified", cost: 5 },
  cpu512: { gpu: 0, ram: 512, cards: 0, tier: "ddr_offload", cost: 12 },
} as const;
type Class = keyof typeof CLASSES;
export type LoadingScenario = { id: string; classes: Class[]; minimumMachines: number; availableFraction: number; offload: boolean; disableFirst: boolean };

/** Fixed cartesian study design; policy outcomes never select or discard cases. */
export function loadingScenarios(): LoadingScenario[] {
  const rows: LoadingScenario[] = [];
  for (const name of Object.keys(CLASSES) as Class[]) for (const count of [1, 2, 4, 8]) {
    rows.push({ id: `${name}-${count}`, classes: Array(count).fill(name), minimumMachines: Math.min(count, 2), availableFraction: 1, offload: false, disableFirst: false });
  }
  const mixes: Class[][] = [
    ["gpu24", "mac64"], ["dual24", "gb10128"], ["laptop12", "gpu24", "mac64"],
    ["dual24", "dual24", "mac64", "gb10128"], ["hbm64x4", "gpu24", "mac64", "gb10128"],
    ["laptop12", "gpu24", "dual24", "mac64", "mac64", "gb10128", "cpu512", "cpu512"],
  ];
  mixes.forEach((classes, i) => {
    for (const mode of ["nominal", "busy", "offload", "disabled"] as const) rows.push({
      id: `mixed-${i}-${mode}`, classes, minimumMachines: Math.min(classes.length, 3),
      availableFraction: mode === "busy" ? 0.35 : 1, offload: mode === "offload", disableFirst: mode === "disabled",
    });
  });
  rows.push({ id: "26-devices-9-hosts", classes: ["hbm64x4", "hbm64x4", "hbm64x4", "hbm64x4", "dual24", "dual24", "dual24", "dual24", "dual24"], minimumMachines: 2, availableFraction: 1, offload: false, disableFirst: false });
  return rows;
}

export const STUDY_WORKLOADS = [
  { id: "stored-weights", contextTokens: 1, concurrentSequences: 1, kv: 0, runtime: 0 },
  { id: "assumed-4k-1", contextTokens: 4096, concurrentSequences: 1, kv: 512, runtime: 4 * 1024 ** 2 },
  { id: "assumed-32k-8", contextTokens: 32768, concurrentSequences: 8, kv: 512, runtime: 4 * 1024 ** 2 },
];

/** Construct test assumptions explicitly; these timing arrays must never be deployed as measurements. */
export function scenarioInput(model: ModelLoadingDefinition, scenario: LoadingScenario, workload: typeof STUDY_WORKLOADS[number]): ModelLoadingPlannerInput {
  const calibrations: ModelLoadingPlannerInput["calibrations"] = [];
  let order = 0;
  const machines: MachineSpecification[] = scenario.classes.map((name, i) => {
    const spec = CLASSES[name], id = `host-${i}`, shared = spec.tier === "mac_unified" || spec.tier === "gb10_unified";
    const host: MachineSpecification = { id, cpu: { architecture: shared ? "arm64" : "x86_64", logicalCores: 16 },
      ram: { totalBytes: spec.ram * GIB, availableBytes: Math.floor(spec.ram * GIB * scenario.availableFraction), reserveBytes: 8 * GIB,
        allowOffload: scenario.offload || name === "cpu512" }, accelerators: [] };
    for (let card = 0; card < spec.cards; card++) {
      host.accelerators.push({ id: `${card}`, vendor: "hypothetical", backend: "study-only", memoryKind: shared ? "unified" : "dedicated",
        ...(shared ? { unifiedTier: spec.tier as "mac_unified" | "gb10_unified" } : {}), memoryTotalBytes: spec.gpu * GIB,
        memoryAvailableBytes: Math.floor(spec.gpu * GIB * scenario.availableFraction), reserveBytes: (shared ? 8 : 2) * GIB,
        enabled: !(scenario.disableFirst && i === 0 && card === 0), order: order++ });
      calibrations.push({ poolId: `${id}:gpu:${card}`, fixedServiceMs: 2, hopServiceMs: i ? 2 : 0,
        layerServiceMs: model.layers.map((layer) => Math.max(1, Math.ceil(layer.weightBytes / (256 * 1024 ** 2))) * spec.cost) });
    }
    if (host.ram.allowOffload && !shared) calibrations.push({ poolId: `${id}:ram`, fixedServiceMs: 4, hopServiceMs: 2,
      layerServiceMs: model.layers.map((layer) => Math.max(1, Math.ceil(layer.weightBytes / (256 * 1024 ** 2))) * 12) });
    return host;
  });
  return { schema: "p4-model-loading-planner-v1", machines, model: { ...model,
    layers: model.layers.map((layer) => ({ ...layer, kvBytesPerTokenPerSequence: workload.kv, runtimeBytes: workload.runtime })) },
    workload: { contextTokens: workload.contextTokens, concurrentSequences: workload.concurrentSequences },
    calibrations, constraints: { minimumMachines: scenario.minimumMachines } };
}

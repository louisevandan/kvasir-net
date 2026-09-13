import type { ModelLoadingPlannerInput } from "../../../tools/cluster-inference/model-loading-planner.ts";

// Independent analyst specification. No production normaliser, admission or solver is called.
const TIERS = ["gddr", "mac_unified", "gb10_unified", "ddr_offload"];
export type ReferenceStage = { deviceId: string; machineId: string; layerBegin: number; layerEnd: number; service: number };
export type ReferencePlan = { maxTierIndex: number; period: number; total: number; stages: ReferenceStage[] };
type Label = { period: number; total: number; stages: ReferenceStage[]; machines: string[] };

/** Keep every non-dominated prefix: a later bottleneck can erase an earlier advantage. */
function retain(labels: Label[], added: Label): Label[] {
  const dominates = (a: Label, b: Label) => a.period <= b.period && a.total <= b.total
    && (a.total < b.total || a.stages.length <= b.stages.length);
  if (labels.some((label) => dominates(label, added))) return labels;
  return [...labels.filter((label) => !dominates(added, label)), added];
}

/** Exact Pareto enumeration in the declared ordered-device, additive-cost model. */
export function referencePlan(input: ModelLoadingPlannerInput): ReferencePlan | null {
  const tiers = input.constraints?.tierOrder ?? TIERS;
  const count = input.model.layers.length;
  const minimum = input.constraints?.minimumMachines ?? 1;
  const devices: Array<{ id: string; machine: string; tier: string; order: number; free: number }> = [];
  for (const host of input.machines) {
    for (const gpu of host.accelerators) {
      if (!gpu.enabled) continue;
      const shared = gpu.memoryKind === "unified";
      const capacity = Math.min(gpu.memoryTotalBytes, gpu.memoryAvailableBytes ?? Infinity,
        shared ? host.ram.totalBytes : Infinity, shared ? host.ram.availableBytes ?? Infinity : Infinity);
      devices.push({ id: `${host.id}:gpu:${gpu.id}`, machine: host.id,
        tier: shared ? gpu.unifiedTier! : "gddr", order: gpu.order,
        free: Math.max(0, capacity - Math.max(gpu.reserveBytes, shared ? host.ram.reserveBytes : 0)) });
    }
    if (host.ram.allowOffload && !host.accelerators.some((gpu) => gpu.memoryKind === "unified")) {
      devices.push({ id: `${host.id}:ram`, machine: host.id, tier: "ddr_offload", order: Number.MAX_SAFE_INTEGER,
        free: Math.max(0, Math.min(host.ram.totalBytes, host.ram.availableBytes ?? Infinity) - host.ram.reserveBytes) });
    }
  }
  devices.sort((a, b) => a.order - b.order || a.id.localeCompare(b.id));
  const layerBytes = input.model.layers.map((layer) => layer.weightBytes + layer.runtimeBytes
    + layer.kvBytesPerTokenPerSequence * input.workload.contextTokens * input.workload.concurrentSequences);
  for (let tier = 0; tier < tiers.length; tier++) {
    const permitted = devices.filter((d) => tiers.indexOf(d.tier as never) <= tier);
    if (new Set(permitted.map((d) => d.machine)).size < minimum || count < minimum) continue;
    let states = new Map<string, { end: number; labels: Label[] }>();
    states.set("0:[]", { end: 0, labels: [{ period: 0, total: 0, stages: [], machines: [] }] });
    for (const device of permitted) {
      const next = new Map([...states].map(([key, state]) => [key, { end: state.end, labels: [...state.labels] }]));
      const timing = input.calibrations.find((entry) => entry.poolId === device.id)!;
      if (!timing) throw new Error(`reference missing timing: ${device.id}`);
      for (const state of states.values()) {
        let bytes = input.model.fixedBytesPerStage;
        let service = timing.fixedServiceMs + timing.hopServiceMs;
        for (let end = state.end + 1; end <= count; end++) {
          bytes += layerBytes[end - 1];
          service += timing.layerServiceMs[end - 1];
          if (bytes > device.free) break;
          if (end < count && input.model.legalCuts && !input.model.legalCuts.includes(end)) continue;
          for (const label of state.labels) {
            const machines = [...new Set([...label.machines, device.machine])].sort();
            const key = `${end}:${machines.length >= minimum ? "satisfied" : JSON.stringify(machines)}`;
            const added = { period: Math.max(label.period, service), total: label.total + service, machines,
              stages: [...label.stages, { deviceId: device.id, machineId: device.machine,
                layerBegin: state.end, layerEnd: end, service }] };
            const existing = next.get(key);
            next.set(key, { end, labels: retain(existing?.labels ?? [], added) });
          }
        }
      }
      states = next;
    }
    const finals = [...states.values()].filter((state) => state.end === count)
      .flatMap((state) => state.labels).filter((label) => label.machines.length >= minimum);
    finals.sort((a, b) => a.period - b.period || a.total - b.total || a.stages.length - b.stages.length);
    if (finals.length) return { maxTierIndex: tier, period: finals[0].period, total: finals[0].total, stages: finals[0].stages };
  }
  return null;
}

/** Score semantics and objectives, allowing several equally good device/cut selections. */
export function comparePlan(input: ModelLoadingPlannerInput, reference: ReferencePlan | null,
  actual: { placement: { maxTierIndex: number; predictedPipelinePeriodMs: number; stages: Array<{
    deviceId: string; machineId: string; layerBegin: number; layerEnd: number; predictedServiceMs: number;
    memory: Record<string, { requiredBytes: number; usableBytes: number }>;
  }> } } | null) {
  if (!reference || !actual) return { agrees: !reference && !actual, category: reference ? "false_refusal" : actual ? "false_admission" : "both_infeasible", periodRegret: null };
  const p = actual.placement;
  let end = 0;
  const used = new Set<string>();
  let invalid = false;
  let previousOrder = -Infinity, previousId = "", computedPeriod = 0, computedTotal = 0, computedTier = 0;
  for (const stage of p.stages) {
    if (stage.layerBegin !== end || stage.layerEnd <= end || used.has(stage.deviceId)) invalid = true;
    const host = input.machines.find((machine) => machine.id === stage.machineId);
    const gpu = host?.accelerators.find((accelerator) => `${host.id}:gpu:${accelerator.id}` === stage.deviceId);
    const ram = host && stage.deviceId === `${host.id}:ram` && host.ram.allowOffload
      && !host.accelerators.some((device) => device.memoryKind === "unified");
    const shared = gpu?.memoryKind === "unified";
    const timing = input.calibrations.find((entry) => entry.poolId === stage.deviceId);
    if (!host || (!gpu && !ram) || gpu?.enabled === false || !timing) { invalid = true; continue; }
    const order = gpu?.order ?? Number.MAX_SAFE_INTEGER;
    if (order < previousOrder || (order === previousOrder && stage.deviceId.localeCompare(previousId) <= 0)) invalid = true;
    previousOrder = order; previousId = stage.deviceId;
    const capacity = gpu ? Math.min(gpu.memoryTotalBytes, gpu.memoryAvailableBytes ?? Infinity,
      shared ? host.ram.totalBytes : Infinity, shared ? host.ram.availableBytes ?? Infinity : Infinity)
      : Math.min(host.ram.totalBytes, host.ram.availableBytes ?? Infinity);
    const reserve = gpu ? Math.max(gpu.reserveBytes, shared ? host.ram.reserveBytes : 0) : host.ram.reserveBytes;
    const free = Math.max(0, capacity - reserve);
    const bytes = input.model.fixedBytesPerStage + input.model.layers.slice(stage.layerBegin, stage.layerEnd).reduce((sum, layer) =>
      sum + layer.weightBytes + layer.runtimeBytes + layer.kvBytesPerTokenPerSequence * input.workload.contextTokens * input.workload.concurrentSequences, 0);
    const service = timing.fixedServiceMs + timing.hopServiceMs + timing.layerServiceMs.slice(stage.layerBegin, stage.layerEnd).reduce((a, b) => a + b, 0);
    if (bytes > free || stage.memory.memory?.requiredBytes !== bytes || stage.memory.memory?.usableBytes !== free
      || Math.abs(stage.predictedServiceMs - service) > 1e-7) invalid = true;
    if (stage.layerEnd < input.model.layers.length && input.model.legalCuts && !input.model.legalCuts.includes(stage.layerEnd)) invalid = true;
    const tier = gpu ? shared ? gpu.unifiedTier! : "gddr" : "ddr_offload";
    computedTier = Math.max(computedTier, (input.constraints?.tierOrder ?? TIERS).indexOf(tier as never));
    computedPeriod = Math.max(computedPeriod, service); computedTotal += service;
    end = stage.layerEnd;
    used.add(stage.deviceId);
    if (Object.values(stage.memory).some((pool) => pool.requiredBytes > pool.usableBytes)) invalid = true;
  }
  if (end !== input.model.layers.length || new Set(p.stages.map((s) => s.machineId)).size < (input.constraints?.minimumMachines ?? 1)) invalid = true;
  const total = computedTotal;
  if (computedTier !== p.maxTierIndex || Math.abs(computedPeriod - p.predictedPipelinePeriodMs) > 1e-7) invalid = true;
  const close = (a: number, b: number) => Math.abs(a - b) <= 1e-7 * Math.max(1, Math.abs(a), Math.abs(b));
  const category = invalid ? "invalid_plan" : p.maxTierIndex !== reference.maxTierIndex ? "tier_mismatch"
    : !close(p.predictedPipelinePeriodMs, reference.period) ? "period_regret"
    : !close(total, reference.total) ? "total_regret" : p.stages.length !== reference.stages.length ? "stage_count_regret" : "optimal";
  return { agrees: category === "optimal", category,
    periodRegret: reference.period === 0 ? (p.predictedPipelinePeriodMs === 0 ? 0 : null) : p.predictedPipelinePeriodMs / reference.period - 1 };
}

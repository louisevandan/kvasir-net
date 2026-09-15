import { isDeepStrictEqual } from "node:util";

export type NativeMemoryEntry = {
  scope: "device" | "host";
  index: number;
  name: string;
  description: string;
  free: number;
  total: number;
  model: number;
  context: number;
  compute: number;
  required: number;
};

export type NativeMemoryPlan = {
  schema: 2;
  memory_topology: { mode: "discrete" | "host-shared"; host_shared_devices: number[] };
  execution_shape: {
    n_ctx: number;
    n_ctx_seq: number;
    n_batch: number;
    n_ubatch: number;
    n_seq_max: number;
    kv_unified: boolean;
  };
  complete: boolean;
  fits_current_free: boolean;
  layer_device_query_supported: boolean;
  layer_device_expectations_checked: boolean;
  layer_default_devices: Array<{ layer: number; device: string }>;
  entries: NativeMemoryEntry[];
};

export type NativeDeploymentInput = {
  schema: "p4-native-deployment-v1";
  model: { id: string; fingerprint: string; layerCount: number; legalCuts: number[] };
  profile: {
    totalContext: number;
    resident: number;
    nBatch: number;
    nUbatch: number;
    kvUnified: boolean;
  };
  minimumMachines: number;
  requireActual: boolean;
  hosts: Array<{
    id: string;
    inspectedUnixMs: number;
    pools: Array<{ id: string; availableBytes: number; reserveBytes: number }>;
  }>;
  stages: Array<{
    id: string;
    hostId: string;
    layerBegin: number;
    layerEnd: number;
    expectedDeviceDescription: string;
    expectedLayerDevice: string;
    entryPools: Record<string, string>;
    runtime: {
      binarySha256: string;
      upstreamCommit: string;
      patchSet: string;
      backendInventory: string;
    };
    plan: NativeMemoryPlan;
    actual?: NativeMemoryPlan;
  }>;
};

export type NativeDeploymentResult = {
  schema: "p4-native-deployment-result-v1";
  modelId: string;
  cuts: number[];
  machineCount: number;
  stages: Array<{
    id: string;
    hostId: string;
    layerBegin: number;
    layerEnd: number;
    plannedBytes: number;
    actualConformant: boolean;
  }>;
  pools: Array<{
    hostId: string;
    poolId: string;
    availableBytes: number;
    reserveBytes: number;
    usableBytes: number;
    requiredBytes: number;
    headroomBytes: number;
  }>;
  loadAuthorized: true;
  actualAllocationConformant: boolean;
};

const fail = (condition: unknown, message: string): asserts condition => {
  if (!condition) throw new Error(message);
};
const integer = (value: number, name: string, minimum = 0): void => {
  fail(Number.isSafeInteger(value) && value >= minimum, `${name} must be a safe integer >= ${minimum}`);
};
const sha = (value: string, length: 40 | 64): boolean =>
  typeof value === "string" && new RegExp(`^[a-f0-9]{${length}}$`).test(value) && !/^([a-f0-9])\1+$/.test(value);
const entryKey = (entry: NativeMemoryEntry): string => `${entry.scope}:${entry.index}`;

function validatePlan(
  input: NativeDeploymentInput,
  stage: NativeDeploymentInput["stages"][number],
  plan: NativeMemoryPlan,
  label: string,
  requireCurrentFree: boolean,
): Map<string, NativeMemoryEntry> {
  fail(plan?.schema === 2 && plan.complete === true,
    `${stage.id} ${label} is incomplete`);
  if (requireCurrentFree) {
    fail(plan.fits_current_free === true,
      `${stage.id} ${label} does not fit current free memory`);
  }
  fail(plan.layer_device_query_supported === true, `${stage.id} ${label} cannot report layer devices`);
  const shape = plan.execution_shape;
  fail(shape.n_ctx === input.profile.totalContext && shape.n_ctx_seq === input.profile.totalContext &&
    shape.n_batch === input.profile.nBatch && shape.n_ubatch === input.profile.nUbatch &&
    shape.n_seq_max === input.profile.resident && shape.kv_unified === input.profile.kvUnified,
    `${stage.id} ${label} execution shape differs from the sealed profile`);
  const expectedLayers = Array.from({ length: stage.layerEnd - stage.layerBegin }, (_, index) => ({
    layer: stage.layerBegin + index,
    device: stage.expectedLayerDevice,
  }));
  fail(isDeepStrictEqual(plan.layer_default_devices, expectedLayers),
    `${stage.id} ${label} layer placement differs from the sealed cut/device`);
  fail(Array.isArray(plan.entries) && plan.entries.length > 0, `${stage.id} ${label} has no memory entries`);
  const entries = new Map<string, NativeMemoryEntry>();
  for (const entry of plan.entries) {
    const key = entryKey(entry);
    fail((entry.scope === "device" || entry.scope === "host") && !entries.has(key),
      `${stage.id} ${label} has a duplicate or unsupported memory entry`);
    integer(entry.index, `${stage.id} ${label} ${key} index`, entry.scope === "host" ? -1 : 0);
    fail(typeof entry.name === "string" && entry.name && typeof entry.description === "string" && entry.description,
      `${stage.id} ${label} ${key} lacks identity`);
    for (const field of ["free", "total", "model", "context", "compute", "required"] as const) {
      integer(entry[field], `${stage.id} ${label} ${key} ${field}`, field === "total" ? 1 : 0);
    }
    fail(entry.required === entry.model + entry.context + entry.compute,
      `${stage.id} ${label} ${key} required bytes differ from components`);
    fail(entry.free <= entry.total,
      `${stage.id} ${label} ${key} free memory exceeds total memory`);
    if (requireCurrentFree) {
      fail(entry.required <= entry.free,
        `${stage.id} ${label} ${key} exceeds native free memory`);
    }
    entries.set(key, entry);
  }
  fail([...entries.values()].some((entry) => entry.scope === "device" &&
    entry.description === stage.expectedDeviceDescription), `${stage.id} ${label} expected device was not observed`);
  return entries;
}

function allocationShape(entries: Map<string, NativeMemoryEntry>): Array<Record<string, unknown>> {
  return [...entries.values()].map((entry) => ({
    scope: entry.scope,
    index: entry.index,
    name: entry.name,
    description: entry.description,
    total: entry.total,
    model: entry.model,
    context: entry.context,
    compute: entry.compute,
    required: entry.required,
  }));
}

/**
 * Turns adapter-native PLAN output into a load decision. The same function
 * validates post-LOAD MEMORY_ACTUAL evidence; missing actual evidence can
 * authorize LOAD but can never claim runtime allocation conformance.
 */
export function validateNativeDeployment(input: NativeDeploymentInput): NativeDeploymentResult {
  fail(input?.schema === "p4-native-deployment-v1", "unsupported native deployment schema");
  fail(input.model?.id && sha(input.model.fingerprint, 64), "model id and measured fingerprint are required");
  integer(input.model.layerCount, "model layer count", 1);
  integer(input.profile.totalContext, "total context", 1);
  integer(input.profile.resident, "resident", 1);
  integer(input.profile.nBatch, "nBatch", 1);
  integer(input.profile.nUbatch, "nUbatch", 1);
  integer(input.minimumMachines, "minimum machines", 2);
  fail(Array.isArray(input.model.legalCuts) && new Set(input.model.legalCuts).size === input.model.legalCuts.length,
    "legal cuts must be explicit and unique");
  input.model.legalCuts.forEach((cut) => integer(cut, "legal cut", 1));

  const hosts = new Map<string, NativeDeploymentInput["hosts"][number]>();
  const poolState = new Map<string, { availableBytes: number; reserveBytes: number; requiredBytes: number }>();
  for (const host of input.hosts ?? []) {
    fail(sha(host.id, 64) && !hosts.has(host.id), "host identity must be a unique measured SHA-256");
    integer(host.inspectedUnixMs, `${host.id} inspection time`, 1);
    hosts.set(host.id, host);
    for (const pool of host.pools ?? []) {
      fail(typeof pool.id === "string" && pool.id, `${host.id} has an empty pool id`);
      const key = `${host.id}:${pool.id}`;
      fail(!poolState.has(key), `duplicate physical pool ${key}`);
      integer(pool.availableBytes, `${key} available bytes`, 1);
      integer(pool.reserveBytes, `${key} reserve bytes`);
      fail(pool.reserveBytes <= pool.availableBytes, `${key} reserve exceeds available memory`);
      poolState.set(key, { availableBytes: pool.availableBytes, reserveBytes: pool.reserveBytes, requiredBytes: 0 });
    }
  }

  fail(Array.isArray(input.stages) && input.stages.length > 0, "deployment has no stages");
  const stageIds = new Set<string>();
  const usedMachines = new Set<string>();
  const cuts = [0];
  const stages: NativeDeploymentResult["stages"] = [];
  let end = 0;
  let sourceIdentity: string | undefined;
  let allActual = true;
  for (const stage of input.stages) {
    fail(typeof stage.id === "string" && stage.id && !stageIds.has(stage.id), "stage id is empty or duplicated");
    stageIds.add(stage.id);
    fail(hosts.has(stage.hostId), `${stage.id} references an unknown host`);
    usedMachines.add(stage.hostId);
    fail(stage.layerBegin === end, `${stage.id} cut is discontinuous`);
    integer(stage.layerEnd, `${stage.id} layer end`, stage.layerBegin + 1);
    if (stage.layerEnd < input.model.layerCount) {
      fail(input.model.legalCuts.includes(stage.layerEnd), `${stage.id} uses a cut not approved by the adapter`);
    }
    end = stage.layerEnd;
    cuts.push(end);
    const runtime = stage.runtime;
    fail(sha(runtime.binarySha256, 64) && sha(runtime.upstreamCommit, 40) && sha(runtime.patchSet, 64) &&
      typeof runtime.backendInventory === "string" && runtime.backendInventory,
      `${stage.id} has an invalid runtime identity`);
    const currentSourceIdentity = `${runtime.upstreamCommit}:${runtime.patchSet}`;
    sourceIdentity ??= currentSourceIdentity;
    fail(sourceIdentity === currentSourceIdentity, `${stage.id} runtime source/patch differs from the deployment`);

    const planned = validatePlan(input, stage, stage.plan, "PLAN", true);
    let plannedBytes = 0;
    for (const [key, entry] of planned) {
      const poolId = stage.entryPools[key];
      fail(typeof poolId === "string" && poolId, `${stage.id} PLAN entry ${key} has no physical pool binding`);
      const pool = poolState.get(`${stage.hostId}:${poolId}`);
      fail(pool, `${stage.id} PLAN entry ${key} references an unknown physical pool`);
      pool.requiredBytes += entry.required;
      integer(pool.requiredBytes, `${stage.id} accumulated pool requirement`);
      plannedBytes += entry.required;
      integer(plannedBytes, `${stage.id} planned bytes`);
    }
    fail(Object.keys(stage.entryPools).length === planned.size,
      `${stage.id} has unused or missing PLAN entry bindings`);
    if (stage.plan.memory_topology.mode === "host-shared") {
      fail(new Set(Object.values(stage.entryPools)).size === 1,
        `${stage.id} host-shared PLAN must bind every entry to one physical pool`);
    }

    let actualConformant = false;
    if (stage.actual) {
      // MEMORY_ACTUAL is measured after allocation. Its current-free field can
      // be below required and fits_current_free can be false; neither describes
      // the allocation that PLAN admitted. Native equality intentionally binds
      // topology, shape, placement and allocated model/context/compute bytes.
      const actual = validatePlan(input, stage, stage.actual, "MEMORY_ACTUAL", false);
      fail(isDeepStrictEqual(stage.actual.memory_topology, stage.plan.memory_topology) &&
        isDeepStrictEqual(stage.actual.execution_shape, stage.plan.execution_shape) &&
        isDeepStrictEqual(allocationShape(actual), allocationShape(planned)),
        `${stage.id} MEMORY_ACTUAL differs from PLAN`);
      actualConformant = true;
    } else {
      allActual = false;
    }
    stages.push({ id: stage.id, hostId: stage.hostId, layerBegin: stage.layerBegin,
      layerEnd: stage.layerEnd, plannedBytes, actualConformant });
  }
  fail(end === input.model.layerCount, "deployment does not cover the full model layer range");
  fail(usedMachines.size >= input.minimumMachines, "deployment uses too few physical machines");
  if (input.requireActual) fail(allActual, "post-LOAD allocation evidence is required");

  const pools = [...poolState.entries()].map(([key, pool]) => {
    const separator = key.lastIndexOf(":");
    const usableBytes = pool.availableBytes - pool.reserveBytes;
    fail(pool.requiredBytes <= usableBytes, `${key} is overcommitted after reserve`);
    return {
      hostId: key.slice(0, separator), poolId: key.slice(separator + 1),
      availableBytes: pool.availableBytes, reserveBytes: pool.reserveBytes,
      usableBytes, requiredBytes: pool.requiredBytes, headroomBytes: usableBytes - pool.requiredBytes,
    };
  });
  return { schema: "p4-native-deployment-result-v1", modelId: input.model.id, cuts,
    machineCount: usedMachines.size, stages, pools, loadAuthorized: true,
    actualAllocationConformant: allActual };
}

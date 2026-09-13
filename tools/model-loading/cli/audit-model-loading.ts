import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import {
  LOADING_TIERS,
  catalogGgufModels,
  loadingPoolsFromFleetSnapshot,
  recommendModelLoad,
  type LoadingPool,
  type LoadingTier,
} from "../src/model-loading-policy.ts";

function argument(name: string): string {
  const index = process.argv.indexOf(name);
  if (index < 0 || !process.argv[index + 1]) throw new Error(`missing ${name}`);
  return process.argv[index + 1];
}

function values(name: string): string[] {
  const index = process.argv.indexOf(name);
  return index < 0 || !process.argv[index + 1] ? [] : process.argv[index + 1].split(",").filter(Boolean);
}

function reserveValues(): Partial<Record<LoadingTier, number>> {
  const result: Partial<Record<LoadingTier, number>> = {};
  for (const entry of values("--reserve-gib")) {
    const [tier, rawGiB] = entry.split("=");
    if (!LOADING_TIERS.includes(tier as LoadingTier) || !rawGiB || !Number.isFinite(Number(rawGiB))) {
      throw new Error(`invalid --reserve-gib entry: ${entry}`);
    }
    result[tier as LoadingTier] = Math.round(Number(rawGiB) * 1024 ** 3);
  }
  return result;
}

function manualTier(requiredBytes: number, pools: LoadingPool[]): LoadingTier | null {
  for (let index = 0; index < LOADING_TIERS.length; index += 1) {
    const admitted = new Set(LOADING_TIERS.slice(0, index + 1));
    const usable = pools.filter((pool) => pool.enabled !== false && admitted.has(pool.tier))
      .reduce((sum, pool) => sum + Math.max(0, pool.capacityBytes - pool.reserveBytes), 0);
    if (usable >= requiredBytes) return LOADING_TIERS[index];
  }
  return null;
}

const modelRoot = path.resolve(argument("--model-root"));
const fleetPath = path.resolve(argument("--fleet"));
const outputPath = path.resolve(argument("--out"));
const excludedDevices = values("--exclude-devices");
const withoutMachines = values("--without-machines");
const reserveBytes = reserveValues();
const kvBytes = Number(process.argv.includes("--kv-bytes") ? argument("--kv-bytes") : "0");
const runtimeBytes = Number(process.argv.includes("--runtime-bytes") ? argument("--runtime-bytes") : "0");
const snapshot = JSON.parse(fs.readFileSync(fleetPath, "utf8"));
const catalog = catalogGgufModels(modelRoot);
const sha256 = (value: string | Buffer): string => crypto.createHash("sha256").update(value).digest("hex");
const catalogManifest = catalog.map((model) => ({
  model_id: model.modelId,
  weight_bytes: model.weightBytes,
  complete: model.complete,
  files: model.files.map((file) => path.relative(modelRoot, file).replaceAll("\\", "/")),
}));

const profiles = [
  { id: "full-fleet", excludedMachineIds: [] },
  ...(withoutMachines.length ? [{ id: "without-selected-machines", excludedMachineIds: withoutMachines }] : []),
].map((profile) => {
  const pools = loadingPoolsFromFleetSnapshot(snapshot, {
    excludedDeviceIds: excludedDevices,
    excludedMachineIds: profile.excludedMachineIds,
    reserveBytes,
    includeHostDdr: true,
  });
  const rows = catalog.filter((model) => model.complete).map((model) => {
    const demand = { modelId: model.modelId, weightBytes: model.weightBytes, kvBytes, runtimeBytes };
    const expectedTier = manualTier(model.weightBytes + kvBytes + runtimeBytes, pools);
    let actualTier: LoadingTier | null = null;
    let currentlyAdmissible = false;
    let error: string | null = null;
    try {
      const recommendation = recommendModelLoad(demand, pools);
      actualTier = recommendation.maxTier;
      currentlyAdmissible = recommendation.currentlyAdmissible;
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    }
    return {
      model_id: model.modelId,
      weight_bytes: model.weightBytes,
      expected_tier: expectedTier,
      actual_tier: actualTier,
      agrees: expectedTier === actualTier,
      currently_admissible: currentlyAdmissible,
      error,
    };
  });
  const distribution = Object.fromEntries([...LOADING_TIERS, "unplaceable"].map((tier) => [
    tier, rows.filter((row) => (row.actual_tier ?? "unplaceable") === tier).length,
  ]));
  return {
    id: profile.id,
    excluded_machine_ids: profile.excludedMachineIds,
    pool_count: pools.length,
    enabled_pool_count: pools.filter((pool) => pool.enabled !== false).length,
    agreement_count: rows.filter((row) => row.agrees).length,
    disagreement_count: rows.filter((row) => !row.agrees).length,
    distribution,
    rows,
  };
});

const report = {
  schema: "p4-model-loading-audit-v1",
  generated_at: new Date().toISOString(),
  model_root: modelRoot,
  fleet_file: fleetPath,
  fleet_file_sha256: sha256(fs.readFileSync(fleetPath)),
  demand_contract: {
    weight_bytes: "catalogued GGUF file bytes",
    kv_bytes: kvBytes,
    runtime_bytes: runtimeBytes,
    note: "A zero value is a weight-tier audit only. Runtime placement must supply measured PLAN/profile bytes.",
  },
  excluded_device_ids: excludedDevices,
  reserve_bytes_per_pool: reserveBytes,
  catalog: {
    complete_models: catalog.filter((model) => model.complete).length,
    incomplete_models: catalog.filter((model) => !model.complete).map((model) => model.modelId),
    metadata_manifest_sha256: sha256(JSON.stringify(catalogManifest)),
  },
  profiles,
};
fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify({ output: outputPath, catalog: report.catalog, profiles: profiles.map(({ rows: _rows, ...profile }) => profile) }, null, 2));

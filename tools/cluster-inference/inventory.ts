import { createHash } from "node:crypto";

export type AgentTarget = { id: string; agent: string; required?: boolean };

export type AgentSnapshot = {
  schema: number;
  generated_at_unix_ms: number;
  machine: { capability: unknown; occupancy: unknown; probes: unknown };
};

export type MachineInventory = {
  schema: "p4-machine-inventory-v1";
  machineId: string;
  agent: string;
  snapshotSchema: number;
  observedAtUnixMs: number;
  capabilitySha256: string;
  capability: unknown;
  occupancy: unknown;
  probes: unknown;
};

export type FleetInventory = {
  schema: "p4-fleet-inventory-v1";
  collectedAt: string;
  machines: MachineInventory[];
  failures: Array<{ machineId: string; agent: string; required: boolean; error: string }>;
};

function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.entries(value as Record<string, unknown>)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, entry]) => `${JSON.stringify(key)}:${canonical(entry)}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

export function machineInventory(target: AgentTarget, snapshot: AgentSnapshot): MachineInventory {
  if (!target.id || !target.agent) throw new Error("machine target needs id and agent");
  if (!Number.isSafeInteger(snapshot.schema) || snapshot.schema < 1) throw new Error(`${target.id} has invalid snapshot schema`);
  if (!Number.isSafeInteger(snapshot.generated_at_unix_ms) || snapshot.generated_at_unix_ms < 1) {
    throw new Error(`${target.id} has invalid observation time`);
  }
  if (!snapshot.machine || typeof snapshot.machine !== "object" || !("capability" in snapshot.machine)) {
    throw new Error(`${target.id} snapshot omitted machine capability`);
  }
  return {
    schema: "p4-machine-inventory-v1",
    machineId: target.id,
    agent: target.agent,
    snapshotSchema: snapshot.schema,
    observedAtUnixMs: snapshot.generated_at_unix_ms,
    capabilitySha256: createHash("sha256").update(canonical(snapshot.machine.capability)).digest("hex"),
    capability: snapshot.machine.capability,
    occupancy: snapshot.machine.occupancy,
    probes: snapshot.machine.probes,
  };
}

export function fleetInventory(
  collectedAt: string,
  machines: MachineInventory[],
  failures: FleetInventory["failures"],
): FleetInventory {
  const ids = new Set<string>();
  for (const machine of machines) {
    if (ids.has(machine.machineId)) throw new Error(`duplicate machine inventory: ${machine.machineId}`);
    ids.add(machine.machineId);
  }
  return {
    schema: "p4-fleet-inventory-v1",
    collectedAt,
    machines: [...machines].sort((left, right) => left.machineId.localeCompare(right.machineId)),
    failures: [...failures].sort((left, right) => left.machineId.localeCompare(right.machineId)),
  };
}

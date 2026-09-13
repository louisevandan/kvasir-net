import assert from "node:assert/strict";
import test from "node:test";
import { fleetInventory, machineInventory } from "../../../tools/cluster-inference/inventory.ts";

const snapshot = {
  schema: 1,
  generated_at_unix_ms: 1_789_000_000_000,
  machine: {
    capability: {
      os: "macos",
      gpus: [{ vendor: "Apple", backend: "metal", memory_kind: "unified", memory_total_bytes: 68_719_476_736 }],
    },
    occupancy: { memory: { available_bytes: 40_000_000_000 } },
    probes: { gpus: { source: "provider-aggregate", state: "available" } },
  },
};

test("a machine snapshot retains provider-neutral capability and occupancy", () => {
  const inventory = machineInventory({ id: "mac20", agent: "tcp://192.168.0.20:52005" }, snapshot);
  assert.equal(inventory.capabilitySha256.length, 64);
  assert.equal((inventory.capability as typeof snapshot.machine.capability).gpus[0].backend, "metal");
  assert.deepEqual(inventory.occupancy, snapshot.machine.occupancy);
});

test("capability identity is stable across JSON property order", () => {
  const reversed = structuredClone(snapshot);
  reversed.machine.capability = {
    gpus: snapshot.machine.capability.gpus,
    os: "macos",
  } as typeof snapshot.machine.capability;
  assert.equal(
    machineInventory({ id: "a", agent: "tcp://a:1" }, snapshot).capabilitySha256,
    machineInventory({ id: "b", agent: "tcp://b:1" }, reversed).capabilitySha256,
  );
});

test("fleet inventory preserves partial failures and has deterministic machine order", () => {
  const mac = machineInventory({ id: "mac", agent: "tcp://mac:1" }, snapshot);
  const other = { ...mac, machineId: "alpha", agent: "tcp://alpha:1" };
  const fleet = fleetInventory("2026-09-13T00:00:00.000Z", [mac, other], [
    { machineId: "offline", agent: "tcp://offline:1", required: true, error: "timed out" },
  ]);
  assert.deepEqual(fleet.machines.map((machine) => machine.machineId), ["alpha", "mac"]);
  assert.equal(fleet.failures[0].required, true);
});

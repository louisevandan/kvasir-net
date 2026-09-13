import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import {
  fleetInventory,
  machineInventory,
  type AgentSnapshot,
  type AgentTarget,
  type MachineInventory,
} from "../src/inventory.ts";

function argument(name: string, fallback: string | null = null): string | null {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] ?? null : fallback;
}

function atomicJson(file: string, value: unknown): void {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const temporary = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`);
  fs.renameSync(temporary, file);
}

const agentsFile = argument("--agents");
const outDir = argument("--out-dir", path.resolve(import.meta.dirname, "../target/fleet-inventory"))!;
const python = argument("--python", "python")!;
const probe = argument(
  "--probe",
  path.join(process.cwd(), "target", "v11-final-three-turns-20260912", "runtime", "event-probe.py"),
)!;
if (!agentsFile) throw new Error("usage: node collect-inventory.ts --agents <agents.json> [--out-dir <directory>]");
const targets = JSON.parse(fs.readFileSync(agentsFile, "utf8")) as AgentTarget[];
if (!Array.isArray(targets) || targets.length === 0) throw new Error("agents file must be a non-empty array");

const machines: MachineInventory[] = [];
const failures: Array<{ machineId: string; agent: string; required: boolean; error: string }> = [];
for (const target of targets) {
  const result = spawnSync(python, [probe, target.agent, target.agent], {
    encoding: "utf8",
    timeout: 25_000,
    maxBuffer: 8 * 1024 * 1024,
  });
  try {
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error((result.stderr || result.stdout || `probe exited ${result.status}`).trim());
    const replies = JSON.parse(result.stdout) as Array<{ payload: AgentSnapshot }>;
    if (replies.length !== 1) throw new Error(`expected one snapshot, received ${replies.length}`);
    const inventory = machineInventory(target, replies[0].payload);
    machines.push(inventory);
    const stamp = new Date(inventory.observedAtUnixMs).toISOString().replaceAll(":", "-");
    atomicJson(path.join(outDir, "machines", target.id, `${stamp}.json`), inventory);
    atomicJson(path.join(outDir, "machines", target.id, "latest.json"), inventory);
  } catch (error) {
    failures.push({
      machineId: target.id,
      agent: target.agent,
      required: target.required !== false,
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

const collectedAt = new Date().toISOString();
const inventory = fleetInventory(collectedAt, machines, failures);
const stamp = collectedAt.replaceAll(":", "-");
atomicJson(path.join(outDir, "fleet", `${stamp}.json`), inventory);
atomicJson(path.join(outDir, "fleet", "latest.json"), inventory);
process.stdout.write(`${JSON.stringify({ outDir, machines: machines.length, failures: failures.length })}\n`);
if (failures.some((failure) => failure.required)) process.exitCode = 1;

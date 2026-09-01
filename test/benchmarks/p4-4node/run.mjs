#!/usr/bin/env node
// Four-node acceptance run.
//
//   node test/benchmarks/p4-4node/run.mjs <scenario> [--out DIR]
//
// Starts one agent, samples every NVIDIA device, drives the scenario through
// p4-event-drive, then judges whether the answers mean anything. Structural
// success (40/40 delivered) is not acceptance; judge.mjs decides.

import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { judgeArtifact } from "./judge.mjs";
import { openTunnel, proveTunnelIdentity, startRemoteGpuSampler } from "./remote.mjs";
import { scenario } from "./scenarios.mjs";
import { writeConfig } from "./spec.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");

function collect(child) {
  const state = { stdout: "", stderr: "" };
  child.stdout?.on("data", (chunk) => { state.stdout += chunk.toString("utf8"); });
  child.stderr?.on("data", (chunk) => { state.stderr += chunk.toString("utf8"); });
  return state;
}

const waitForExit = (child) => new Promise((resolve, reject) => {
  child.once("error", reject);
  child.once("exit", (code, signal) => resolve({ code, signal }));
});

async function waitForReady(child, state, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (state.stdout.includes("P4_EVENT_AGENT_READY")) return;
    if (child.exitCode !== null) {
      throw new Error(`agent exited before READY: ${state.stderr || state.stdout}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(`agent READY timeout: ${state.stderr || state.stdout}`);
}

async function stopChild(child) {
  if (!child || child.exitCode !== null) return;
  child.kill();
  await Promise.race([
    waitForExit(child).catch(() => undefined),
    new Promise((resolve) => setTimeout(resolve, 3_000)),
  ]);
}

function metrics(artifact) {
  const rows = [];
  let batches = 0;
  let mixed = 0;
  for (const observation of artifact.batch_observations ?? []) {
    for (const batch of observation.physical_batches ?? []) {
      batches += 1;
      rows.push(batch.rows);
      if (batch.prefill_rows > 0 && batch.decode_rows > 0) mixed += 1;
    }
  }
  const seconds = (artifact.elapsed_ms ?? 0) / 1000;
  const decode = artifact.requests.reduce((sum, r) => sum + (r.decode_rows ?? 0), 0);
  const prefill = artifact.requests.reduce((sum, r) => sum + (r.prefill_rows ?? 0), 0);
  const totalRows = rows.reduce((sum, value) => sum + value, 0);
  return {
    wall_s: Number(seconds.toFixed(2)),
    prefill_rows: prefill,
    decode_rows: decode,
    generation_tps: seconds > 0 ? Number((decode / seconds).toFixed(2)) : null,
    total_tps: seconds > 0 ? Number(((decode + prefill) / seconds).toFixed(2)) : null,
    physical_batches: batches,
    rows_per_batch: batches > 0 ? Number((totalRows / batches).toFixed(2)) : null,
    ms_per_batch: batches > 0 ? Number(((seconds * 1000) / batches).toFixed(1)) : null,
    mixed_batches: mixed,
  };
}

async function main() {
  const [name, ...rest] = process.argv.slice(2);
  if (!name) throw new Error("usage: run.mjs <scenario> [--target local|remote] [--out DIR]");
  const outIndex = rest.indexOf("--out");
  const targetIndex = rest.indexOf("--target");
  const target = targetIndex >= 0 ? rest[targetIndex + 1] : "local";
  const spec = scenario(name, target);
  const outDir = path.resolve(
    outIndex >= 0 ? rest[outIndex + 1]
      : path.join(root, "target", "p4-4node", target === "local" ? name : `${name}-${target}`),
  );

  const { file: configPath } = writeConfig(spec, outDir);
  const artifactPath = path.join(outDir, "artifact.json");
  const ingress = new URL(spec.ingress);

  // A remote target already has its agent running as an interactive scheduled
  // task (remote-agent.mjs), because only a process owned by the logged-on
  // user sees the mapped drive the model lives on.
  const agent = spec.target === "remote" ? null
    : spawn(path.join(root, "target", "release", "p4-agent.exe"),
      [`${ingress.hostname}:${ingress.port}`], {
        cwd: root,
        windowsHide: true,
        stdio: ["ignore", "pipe", "pipe"],
        env: { ...process.env, P4_AGENT_STATS: "1", P4_STAGED_LLAMA_INHERIT_STDERR: "1" },
      });
  const agentOutput = agent ? collect(agent) : { stdout: "", stderr: "" };
  let tunnel = null;
  let identity = null;
  let sampler;
  let samplerOutput = { stdout: "" };
  let driveOutput = { stdout: "", stderr: "" };
  let failure;

  try {
    if (agent) await waitForReady(agent, agentOutput, 20_000);
    if (spec.tunnel) {
      // Opening the forward is not proof it is ours: a bind failure leaves a
      // previous forward holding the port and the run would still look
      // remote. The identity probe makes the far side confirm it.
      ({ child: tunnel } = await openTunnel(spec.tunnel));
      identity = proveTunnelIdentity(spec.tunnel.host, spec.tunnel.remotePort);
    }
    sampler = spec.target === "remote"
      ? startRemoteGpuSampler(spec.tunnel.host)
      : spawn("nvidia-smi", [
      "--query-gpu=timestamp,index,name,utilization.gpu,memory.used,power.draw",
      "--format=csv,noheader,nounits", "-lms", "250",
    ], { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
    if (sampler && spec.target !== "remote") samplerOutput = collect(sampler);

    const drive = spawn(path.join(root, "target", "release", "p4-event-drive.exe"),
      [configPath, artifactPath], { cwd: root, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
    driveOutput = collect(drive);
    const result = await waitForExit(drive);
    if (result.code !== 0) failure = `drive exited ${result.code}`;
  } catch (error) {
    failure = error.message;
  } finally {
    if (spec.target === "remote" && sampler) samplerOutput = { stdout: await sampler.stop() };
    else await stopChild(sampler);
    fs.writeFileSync(path.join(outDir, "gpu.csv"), samplerOutput.stdout, "utf8");
    fs.writeFileSync(path.join(outDir, "agent.stderr.log"), agentOutput.stderr, "utf8");
    fs.writeFileSync(path.join(outDir, "drive.stderr.log"), driveOutput.stderr, "utf8");
    await stopChild(agent);
    await stopChild(tunnel);
  }

  if (failure) {
    process.stderr.write(`P4_4NODE_FAILED ${failure}\n${driveOutput.stderr.trim().slice(-3000)}\n`);
    process.exitCode = 1;
    return;
  }

  const artifact = JSON.parse(fs.readFileSync(artifactPath, "utf8"));
  const verdict = judgeArtifact(artifact);
  const report = {
    scenario: spec.name,
    target: spec.target,
    // Which machine actually served the run, taken from the far side rather
    // than from the scenario name.
    host: identity,
    description: spec.description,
    structural: {
      passed: artifact.passed,
      requests: artifact.request_count,
      completed: artifact.completed_count,
      released: artifact.released_count,
    },
    meaning: { passed: verdict.passed, meaningful: verdict.meaningful, total: verdict.total },
    metrics: metrics(artifact),
    sample_answer: artifact.requests[0]?.response?.slice(0, 400) ?? "",
    rejected: verdict.results.filter((r) => !r.meaningful).slice(0, 5),
  };
  fs.writeFileSync(path.join(outDir, "report.json"), `${JSON.stringify(report, null, 2)}\n`, "utf8");
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);

  if (!artifact.passed || !verdict.passed) {
    process.stderr.write("P4_4NODE_NOT_ACCEPTED\n");
    process.exitCode = 1;
  }
}

main().catch((error) => {
  process.stderr.write(`P4_4NODE_FAILED ${error.message}\n`);
  process.exitCode = 1;
});

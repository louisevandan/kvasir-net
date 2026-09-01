#!/usr/bin/env node

import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { startGpuSampler } from "../direct-pipeline/gpu-sampler.mjs";
import { loadConfig } from "./config.mjs";

const ownPath = fileURLToPath(import.meta.url);
const root = path.resolve(path.dirname(ownPath), "../../..");

function collect(child) {
  const state = { stdout: "", stderr: "" };
  child.stdout.on("data", (chunk) => { state.stdout += chunk.toString("utf8"); });
  child.stderr.on("data", (chunk) => { state.stderr += chunk.toString("utf8"); });
  return state;
}

function waitForExit(child) {
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve({ code, signal }));
  });
}

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
  if (child.exitCode !== null) return;
  child.kill();
  await Promise.race([
    waitForExit(child).catch(() => undefined),
    new Promise((resolve) => setTimeout(resolve, 2_000)),
  ]);
}

function executable(relative) {
  return path.join(root, "apps", "p4", "target", "release", relative);
}

async function main() {
  const [configArgument, artifactArgument] = process.argv.slice(2);
  if (!configArgument || !artifactArgument) {
    throw new Error("usage: node run.mjs CONFIG.json ARTIFACT.json");
  }
  const configPath = path.resolve(configArgument);
  const artifactPath = path.resolve(artifactArgument);
  const config = await loadConfig(configPath);
  const ingress = new URL(config.ingress_agent);
  if (ingress.protocol !== "tcp:" || !ingress.hostname || !ingress.port) {
    throw new Error("config ingress_agent must be tcp://HOST:PORT");
  }
  const listen = `${ingress.hostname}:${ingress.port}`;
  await fs.mkdir(path.dirname(artifactPath), { recursive: true });
  const prefix = artifactPath.replace(/\.json$/i, "");
  const resolvedConfigPath = `${prefix}.resolved-config.json`;
  await fs.writeFile(resolvedConfigPath, `${JSON.stringify(config, null, 2)}\n`, "utf8");
  const agent = spawn(executable("p4-agent.exe"), [listen], {
    cwd: root,
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const agentOutput = collect(agent);
  let sampler;
  let driveOutput = { stdout: "", stderr: "" };
  try {
    await waitForReady(agent, agentOutput, 10_000);
    sampler = await startGpuSampler("required");
    const drive = spawn(executable("p4-event-drive.exe"), [resolvedConfigPath, artifactPath], {
      cwd: root,
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    driveOutput = collect(drive);
    const result = await waitForExit(drive);
    if (result.code !== 0) {
      throw new Error(`event drive failed code=${result.code} signal=${result.signal ?? "none"}`);
    }
  } finally {
    if (sampler) {
      const gpu = await sampler.stop();
      await fs.writeFile(`${prefix}.gpu.csv`, gpu.raw, "utf8");
      const { raw, ...summary } = gpu;
      void raw;
      await fs.writeFile(`${prefix}.gpu.json`, `${JSON.stringify(summary, null, 2)}\n`, "utf8");
    }
    await fs.writeFile(`${prefix}.agent.stdout.log`, agentOutput.stdout, "utf8");
    await fs.writeFile(`${prefix}.agent.stderr.log`, agentOutput.stderr, "utf8");
    await fs.writeFile(`${prefix}.drive.stdout.log`, driveOutput.stdout, "utf8");
    await fs.writeFile(`${prefix}.drive.stderr.log`, driveOutput.stderr, "utf8");
    await stopChild(agent);
  }
  process.stdout.write(`${driveOutput.stdout.trim()}\n`);
  process.stderr.write(driveOutput.stderr);
}

main().catch((error) => {
  process.stderr.write(`P4_EVENT_GATE_RUN_FAILED ${error.message}\n`);
  process.exitCode = 1;
});

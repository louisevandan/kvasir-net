#!/usr/bin/env node

// Short load/HELLO/UNLOAD smoke for an already-built staged server.
// It is intentionally separate from GPU-capacity proof.

import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = findRepoRoot(scriptDir);

function findRepoRoot(start) {
  let current = path.resolve(start);
  while (current !== path.dirname(current)) {
    if (fs.existsSync(path.join(current, "package.json")) && fs.existsSync(path.join(current, "apps"))) return current;
    current = path.dirname(current);
  }
  throw new Error("could not locate repository root");
}

function argument(name, fallback) {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

function availablePort() {
  return new Promise((resolve, reject) => {
    const probe = net.createServer();
    probe.once("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const port = probe.address().port;
      probe.close(() => resolve(port));
    });
  });
}

function frame(operation, body = Buffer.alloc(0)) {
  const header = Buffer.alloc(12);
  header.write("LCP4", 0, "ascii");
  header.writeUInt16LE(1, 4);
  header.writeUInt8(operation, 6);
  header.writeUInt8(0, 7);
  header.writeUInt32LE(body.length, 8);
  return Buffer.concat([header, body]);
}

function readFrame(socket) {
  return new Promise((resolve, reject) => {
    let buffer = Buffer.alloc(0);
    const onData = (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      if (buffer.length < 12) return;
      const size = buffer.readUInt32LE(8) + 12;
      if (buffer.length < size) return;
      cleanup();
      resolve({ operation: buffer.readUInt8(6), body: buffer.subarray(12, size) });
    };
    const onError = (error) => { cleanup(); reject(error); };
    const onClose = () => { cleanup(); reject(new Error("stage server socket closed before a response")); };
    const cleanup = () => {
      socket.off("data", onData);
      socket.off("error", onError);
      socket.off("close", onClose);
    };
    socket.on("data", onData);
    socket.once("error", onError);
    socket.once("close", onClose);
  });
}

function waitForReady(child, timeoutMs) {
  return new Promise((resolve, reject) => {
    const lines = [];
    const timer = setTimeout(() => { cleanup(); reject(new Error(`READY timeout after ${timeoutMs} ms`)); }, timeoutMs);
    const onLine = (line) => {
      lines.push(line);
      if (line.startsWith("READY port=")) { cleanup(); resolve(lines); }
    };
    const onExit = (code, signal) => {
      cleanup();
      const tail = lines.slice(-12).join("\\n");
      reject(new Error(`stage server exited before READY code=${code} signal=${signal}${tail ? `\\n${tail}` : ""}`));
    };
    let pending = "";
    const onData = (chunk) => {
      pending += chunk.toString();
      const parts = pending.split(/\r?\n/u);
      pending = parts.pop() ?? "";
      parts.filter(Boolean).forEach(onLine);
    };
    const cleanup = () => {
      clearTimeout(timer);
      child.stderr.off("data", onData);
      child.off("exit", onExit);
    };
    child.stderr.on("data", onData);
    child.once("exit", onExit);
  });
}

function highlightLog(lines) {
  const interesting = /^(warning:|PLAN_APPLIED|READY port=|llama_model_loader: loaded meta|load_tensors:   CPU_Mapped|llama_context: n_ctx|llama_kv_cache:.*KV buffer size|sched_reserve:.*compute buffer|linkcpp graph stage)/u;
  return lines.filter((line) => interesting.test(line));
}

async function main() {
  const model = path.resolve(argument("--model", "S:\\models\\Qwen2.5-1.5B-Instruct-Q8_0.gguf"));
  const executable = path.resolve(argument("--executable", ".cache/staged-server-llama/Release/p4_staged_server.exe"));
  const timeoutMs = Number(argument("--timeout-ms", "45000"));
  const gpuLayers = Number(argument("--n-gpu-layers", "0"));
  const device = argument("--device", "");
  const planSuffix = argument("--plan-suffix", "").trim();
  if (!fs.existsSync(model)) throw new Error(`model not found: ${model}`);
  if (!fs.existsSync(executable)) throw new Error(`stage server executable not found: ${executable}`);
  const { readPlannerModel } = await import("llama_domain/server");
  const modelInfo = await readPlannerModel(model, [model]);
  const layerBegin = Number(argument("--layer-begin", "0"));
  const layerEnd = Number(argument("--layer-end", String(modelInfo.nLayer)));
  const port = Number(argument("--port", String(await availablePort())));
  const deviceArg = device ? ` --device ${device}` : "";
  const plan = `--model "${model}" --layer-begin ${layerBegin} --layer-end ${layerEnd} --ctx-size 128 --batch-size 32 --ubatch-size 32 --parallel 1 --n-gpu-layers ${gpuLayers}${deviceArg} --flash-attn 0${planSuffix ? ` ${planSuffix}` : ""}`;
  const planBytes = Buffer.from(plan, "utf8");
  const prefix = Buffer.alloc(4);
  prefix.writeUInt32LE(planBytes.length);
  const executableDir = path.dirname(executable);
  const child = spawn(executable, ["--port", String(port), "--bind", "127.0.0.1"], {
    cwd: repoRoot,
    stdio: ["pipe", "ignore", "pipe"],
    windowsHide: true,
    env: {
      ...process.env,
      PATH: [
        executableDir,
        path.join(path.dirname(executableDir), "bin", "Release"),
        process.env.PATH ?? "",
      ].join(";"),
    },
  });
  child.stdin.write(Buffer.concat([prefix, planBytes]));
  let socket;
  const startedAt = Date.now();
  try {
    const log = await waitForReady(child, timeoutMs);
    socket = await new Promise((resolve, reject) => {
      const candidate = net.createConnection({ host: "127.0.0.1", port }, () => resolve(candidate));
      candidate.once("error", reject);
    });
    socket.write(frame(1));
    const hello = await readFrame(socket);
    if (hello.operation !== 1) throw new Error(`HELLO response operation=${hello.operation}`);
    const helloCapabilities = hello.body.subarray(2).toString("utf8");
    for (const field of ["mtp_parser=1", "mtp_execution=0", "speculative_parser=1", "speculative_execution=0"]) {
      if (!helloCapabilities.includes(field)) throw new Error(`HELLO missing capability field ${field}: ${helloCapabilities}`);
    }
    socket.write(frame(9));
    const unload = await readFrame(socket);
    if (unload.operation !== 9) throw new Error(`UNLOAD response operation=${unload.operation}`);
    socket.end();
    child.stdin.end();
    const exit = await new Promise((resolve) => child.once("exit", (code, signal) => resolve({ code, signal })));
    const result = {
      status: exit.code === 0 ? "passed" : "failed",
      model,
      executable,
      backend_selection: `--n-gpu-layers ${gpuLayers}${deviceArg}`,
      layer_begin: layerBegin,
      layer_end_exclusive: layerEnd,
      hello_operation: hello.operation,
      hello_capabilities: helloCapabilities,
      unload_operation: unload.operation,
      elapsed_ms: Date.now() - startedAt,
      exit,
      ready_log_line_count: log.length,
      ready_log_highlights: highlightLog(log),
    };
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    if (result.status !== "passed") process.exitCode = 1;
  } finally {
    socket?.destroy();
    if (child.exitCode === null) child.kill();
    // On Windows the native child may outlive Node's direct handle. Reap
    // only this spawned PID and its descendants so repeated real-model
    // smokes cannot accumulate CUDA contexts.
    if (process.platform === "win32" && child.pid) {
      spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    }
  }
}

try {
  await main();
} catch (error) {
  process.stderr.write(`stage smoke failed: ${error.message}\n`);
  process.exitCode = 1;
}

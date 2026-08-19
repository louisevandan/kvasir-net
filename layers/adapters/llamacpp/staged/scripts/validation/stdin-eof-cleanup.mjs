#!/usr/bin/env node

// Real-parent-liveness validation for an already-built staged C++ server.
// The model is loaded, HELLO is exchanged, and the parent's stdin is closed
// without UNLOAD. The server must exit and release its TCP listener.

import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";

const model = path.resolve(argument("--model", "S:\\models\\Qwen2.5-1.5B-Instruct-Q8_0.gguf"));
const executable = path.resolve(argument("--executable", ".cache/staged-server-llama/Release/p4_staged_server.exe"));
const timeoutMs = Number(argument("--timeout-ms", "45000"));
const layerEnd = Number(argument("--layer-end", "28"));

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

function readFrame(socket, timeout) {
  return new Promise((resolve, reject) => {
    let buffer = Buffer.alloc(0);
    const timer = setTimeout(() => finish(new Error("HELLO response timeout")), timeout);
    const finish = (error, value) => {
      clearTimeout(timer);
      socket.off("data", onData);
      socket.off("error", onError);
      socket.off("close", onClose);
      if (error) reject(error); else resolve(value);
    };
    const onData = (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      if (buffer.length < 12) return;
      const size = buffer.readUInt32LE(8) + 12;
      if (buffer.length >= size) finish(null, { operation: buffer.readUInt8(6) });
    };
    const onError = (error) => finish(error);
    const onClose = () => finish(new Error("server socket closed before HELLO response"));
    socket.on("data", onData);
    socket.once("error", onError);
    socket.once("close", onClose);
  });
}

function waitForReady(child, timeout) {
  return new Promise((resolve, reject) => {
    let pending = "";
    const lines = [];
    const timer = setTimeout(() => finish(new Error(`READY timeout after ${timeout} ms`)), timeout);
    const finish = (error) => {
      clearTimeout(timer);
      child.stderr.off("data", onData);
      child.off("exit", onExit);
      if (error) reject(error); else resolve(lines);
    };
    const onData = (chunk) => {
      pending += chunk.toString();
      const parts = pending.split(/\r?\n/u);
      pending = parts.pop() ?? "";
      for (const line of parts) {
        if (!line) continue;
        lines.push(line);
        if (line.startsWith("READY port=")) finish();
      }
    };
    const onExit = (code, signal) => finish(new Error(`server exited before READY code=${code} signal=${signal}`));
    child.stderr.on("data", onData);
    child.once("exit", onExit);
  });
}

function processExists(pid) {
  const result = spawnSync("tasklist", ["/FI", `PID eq ${pid}`, "/FO", "CSV", "/NH"], {
    encoding: "utf8",
    windowsHide: true,
  });
  return result.status === 0 && result.stdout.includes(`"${pid}"`);
}

function portAccepts(port) {
  return new Promise((resolve) => {
    const socket = net.createConnection({ host: "127.0.0.1", port });
    const finish = (open) => {
      socket.destroy();
      resolve(open);
    };
    socket.once("connect", () => finish(true));
    socket.once("error", () => finish(false));
    setTimeout(() => finish(false), 1000);
  });
}

async function main() {
  if (!fs.existsSync(model)) throw new Error(`model not found: ${model}`);
  if (!fs.existsSync(executable)) throw new Error(`stage server not found: ${executable}`);
  const port = await availablePort();
  const plan = `--model "${model}" --layer-begin 0 --layer-end ${layerEnd} --ctx-size 128 --batch-size 32 --ubatch-size 32 --parallel 1 --n-gpu-layers 0 --flash-attn 0`;
  const planBytes = Buffer.from(plan, "utf8");
  const prefix = Buffer.alloc(4);
  prefix.writeUInt32LE(planBytes.length);
  const executableDir = path.dirname(executable);
  const child = spawn(executable, ["--port", String(port), "--bind", "127.0.0.1"], {
    cwd: process.cwd(),
    stdio: ["pipe", "ignore", "pipe"],
    windowsHide: true,
    env: {
      ...process.env,
      PATH: [executableDir, path.join(path.dirname(executableDir), "bin", "Release"), process.env.PATH ?? ""].join(";"),
    },
  });
  const pid = child.pid;
  if (!pid) throw new Error("server process did not provide a PID");
  let socket;
  let cleaned = false;
  try {
    child.stdin.write(Buffer.concat([prefix, planBytes]));
    const readyLines = await waitForReady(child, timeoutMs);
    // READY handling no longer needs to inspect stderr, but the pipe must be
    // drained so the validation process itself can terminate after the child.
    child.stderr.resume();
    socket = await new Promise((resolve, reject) => {
      const candidate = net.createConnection({ host: "127.0.0.1", port }, () => resolve(candidate));
      candidate.once("error", reject);
    });
    socket.write(frame(1));
    const hello = await readFrame(socket, 5000);
    if (hello.operation !== 1) throw new Error(`HELLO operation=${hello.operation}`);

    const exitPromise = new Promise((resolve) => child.once("exit", (code, signal) => resolve({ code, signal })));
    // Deliberately omit UNLOAD. EOF is the abnormal-parent cleanup path.
    child.stdin.end();
    const exit = await Promise.race([
      exitPromise,
      new Promise((_, reject) => setTimeout(() => reject(new Error("server did not exit after parent stdin EOF")), timeoutMs)),
    ]);
    socket.destroy();
    const processStillExists = processExists(pid);
    const portStillAccepts = await portAccepts(port);
    const result = {
      status: !processStillExists && !portStillAccepts ? "passed" : "failed",
      model,
      executable,
      pid,
      port,
      hello_operation: hello.operation,
      exit,
      process_still_exists: processStillExists,
      port_still_accepts: portStillAccepts,
      ready_log_lines: readyLines,
    };
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    if (result.status !== "passed") process.exitCode = 1;
    cleaned = true;
  } finally {
    socket?.destroy();
    if (!cleaned && child.exitCode === null) {
      spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore", windowsHide: true });
    }
  }
}

try {
  await main();
} catch (error) {
  process.stderr.write(`stdin EOF cleanup failed: ${error.message}\n`);
  process.exitCode = 1;
}

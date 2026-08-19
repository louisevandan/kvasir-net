#!/usr/bin/env node

// Real process crash/restart probe. It deliberately kills a READY server,
// verifies that the listener disappears, and starts a replacement on the same
// port. This is not a power-loss or durable-KV atomicity proof.

import net from "node:net";
import { spawn, spawnSync } from "node:child_process";

const model = argument("--model", "S:\\models\\Qwen2.5-1.5B-Instruct-Q8_0.gguf");
const executable = argument("--executable", ".cache/staged-server-llama/Release/p4_staged_server.exe");
const timeoutMs = Number(argument("--timeout-ms", "120000"));
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
  header.writeUInt32LE(body.length, 8);
  return Buffer.concat([header, body]);
}

function waitForReady(child) {
  return new Promise((resolve, reject) => {
    let pending = "";
    const timer = setTimeout(() => finish(new Error("READY timeout")), timeoutMs);
    const finish = (error, value) => {
      clearTimeout(timer);
      child.stderr.off("data", onData);
      child.off("exit", onExit);
      if (error) reject(error); else resolve(value);
    };
    const onData = (chunk) => {
      pending += chunk.toString();
      const parts = pending.split(/\r?\n/u);
      pending = parts.pop() ?? "";
      for (const line of parts) if (line.startsWith("READY port=")) finish(null, line);
    };
    const onExit = (code, signal) => finish(new Error(`exited before READY code=${code} signal=${signal}`));
    child.stderr.on("data", onData);
    child.once("exit", onExit);
  });
}

function waitExit(child) {
  if (child.exitCode !== null) return Promise.resolve();
  return new Promise((resolve) => child.once("exit", resolve));
}

function connectHello(port) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ host: "127.0.0.1", port }, () => {
      socket.write(frame(1));
    });
    let data = Buffer.alloc(0);
    const timer = setTimeout(() => finish(new Error("HELLO timeout")), timeoutMs);
    const finish = (error) => {
      clearTimeout(timer);
      socket.destroy();
      if (error) reject(error); else resolve();
    };
    socket.on("data", (chunk) => {
      data = Buffer.concat([data, chunk]);
      if (data.length >= 12 && data.readUInt32LE(8) + 12 <= data.length) finish();
    });
    socket.once("error", finish);
  });
}

function portAccepts(port) {
  return new Promise((resolve) => {
    const socket = net.createConnection({ host: "127.0.0.1", port });
    const finish = (value) => { socket.destroy(); resolve(value); };
    socket.once("connect", () => finish(true));
    socket.once("error", () => finish(false));
    setTimeout(() => finish(false), 1000);
  });
}

function launch(port) {
  const plan = `--model "${model}" --layer-begin 0 --layer-end ${layerEnd} --ctx-size 256 --batch-size 32 --ubatch-size 32 --parallel 1 --n-gpu-layers 0 --flash-attn 0`;
  const bytes = Buffer.from(plan, "utf8");
  const prefix = Buffer.alloc(4);
  prefix.writeUInt32LE(bytes.length);
  const child = spawn(executable, ["--port", String(port), "--bind", "127.0.0.1"], {
    stdio: ["pipe", "ignore", "pipe"], windowsHide: true,
    env: { ...process.env, PATH: [
      executable.replace(/[\\/][^\\/]+$/, ""), process.env.PATH ?? "",
    ].join(";") },
  });
  child.stdin.write(Buffer.concat([prefix, bytes]));
  return child;
}

async function main() {
  const port = await availablePort();
  let first;
  let second;
  try {
    first = launch(port);
    const firstReady = await waitForReady(first);
    await connectHello(port);
    spawnSync("taskkill", ["/PID", String(first.pid), "/T", "/F"], { stdio: "ignore", windowsHide: true });
    await waitExit(first);
    const portAfterCrash = await portAccepts(port);
    second = launch(port);
    const secondReady = await waitForReady(second);
    await connectHello(port);
    second.stdin.end();
    await waitExit(second);
    const result = {
      status: !portAfterCrash ? "passed" : "failed",
      port, first_pid: first.pid, second_pid: second.pid,
      first_ready: firstReady, second_ready: secondReady,
      port_accepts_after_crash: portAfterCrash,
    };
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    if (result.status !== "passed") process.exitCode = 1;
  } finally {
    for (const child of [first, second]) {
      if (child && child.exitCode === null) {
        spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore", windowsHide: true });
      }
    }
  }
}

main().catch((error) => { process.stderr.write(`crash/restart probe failed: ${error.message}\n`); process.exitCode = 1; });

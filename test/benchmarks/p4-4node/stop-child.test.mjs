import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import test from "node:test";
import { stopChild } from "./run.mjs";

// Real child processes, no model, network or agent. The subject is only
// whether stopChild can tell a stopped process from a running one.
const forever = () => spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"]);

test("a child stopped by a signal is reported as stopped", async () => {
  const child = forever();
  const stopped = await stopChild(child);
  // kill() sends SIGTERM, so the child exits with exitCode null and
  // signalCode set. Reading exitCode alone reported this - the normal
  // outcome - as a failure to stop, and that verdict failed the run.
  assert.equal(child.exitCode, null, "a signalled exit has no exit code");
  assert.notEqual(child.signalCode, null, "it has a signal instead");
  assert.equal(stopped, true, "a signalled child has stopped");
});

test("a child that exited on its own is reported as stopped", async () => {
  const child = spawn(process.execPath, ["-e", "process.exit(0)"]);
  await new Promise((resolve) => child.once("exit", resolve));
  assert.equal(await stopChild(child), true);
});

test("a child that exited nonzero on its own is still stopped", async () => {
  const child = spawn(process.execPath, ["-e", "process.exit(3)"]);
  await new Promise((resolve) => child.once("exit", resolve));
  assert.equal(child.exitCode, 3);
  assert.equal(await stopChild(child), true, "stopped is not the same as succeeded");
});

test("no child is trivially stopped", async () => {
  assert.equal(await stopChild(null), true);
});

#!/usr/bin/env node
// Actual native startup consumer. The two GPU counts are an explicit pin/model
// counterexample, never a production rule for converting cut ordinals to counts.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { spawnSync } from "node:child_process";

const [serverArg, modelArg, layersArg, device, underArg, fullArg, outputArg] = process.argv.slice(2);
const layers = Number(layersArg), under = Number(underArg), full = Number(fullArg);
assert(serverArg && modelArg && device && outputArg && Number.isInteger(layers) && layers > 1);
assert(Number.isInteger(under) && under >= 0 && Number.isInteger(full) && full > under);
const server = path.resolve(serverArg), model = path.resolve(modelArg), out = path.resolve(outputArg);
assert(fs.existsSync(server) && fs.existsSync(model));
fs.mkdirSync(out); // A measured arm is never overwritten.
const quote = s => `"${s.replaceAll('"', '\\"')}"`;
const hash = file => crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const rows = [];
const scenarios = [
  ["unasserted-counterexample", device, under, "", true, 0],
  ["wrong-gpu-plan", device, under, `0:${layers}:${device}`, true, 7],
  ["correct-gpu-plan", device, full, `0:${layers}:${device}`, true, 0],
  ["explicit-cpu-offload", device, under, [`0:1:CPU`, `1:${layers}:${device}`], true, 0],
  ["explicit-cpu", "none", 0, `0:${layers}:CPU`, true, 0],
  ["missing-layer", device, full, `0:${layers - 1}:${device}`, true, 3],
  ["wrong-gpu-load", device, under, `0:${layers}:${device}`, false, 5],
];
for (const [id, selected, count, expectation, inspect, exit] of scenarios) {
  const expectations = Array.isArray(expectation) ? expectation : expectation ? [expectation] : [];
  const plan = [`--model ${quote(model)}`, `--device ${selected}`, `--n-gpu-layers ${count}`,
    `--layer-begin 0 --layer-end ${layers}`, `--kv-layer-begin 0 --kv-layer-end ${layers}`,
    "--memory-topology discrete --ctx-size 512 --batch-size 128 --ubatch-size 128 --parallel 1 --threads 4 --flash-attn on",
    ...expectations.map(value => `--expect-layer-device ${quote(value)}`),
    inspect ? "--inspect-memory-plan" : ""].join(" ");
  const body = Buffer.from(plan); const size = Buffer.alloc(4); size.writeUInt32LE(body.length);
  fs.writeFileSync(path.join(out, `${id}.plan.txt`), plan);
  const run = spawnSync(server, ["--port", "22194", "--bind", "127.0.0.1"], {
    input: Buffer.concat([size, body]), timeout: 120_000, maxBuffer: 32 * 1024 * 1024, windowsHide: true,
  });
  const log = Buffer.concat([run.stdout ?? Buffer.alloc(0), run.stderr ?? Buffer.alloc(0)]);
  fs.writeFileSync(path.join(out, `${id}.log`), log);
  const text = log.toString("utf8");
  const line = text.split(/\r?\n/u).find(value => value.startsWith("MEMORY_PLAN "));
  const measured = line ? JSON.parse(line.slice("MEMORY_PLAN ".length)) : null;
  let error = null;
  try {
    assert.equal(run.error, undefined); assert.equal(run.status, exit);
    if (id === "wrong-gpu-plan" || id === "wrong-gpu-load") {
      assert(text.includes(`layer=0 expected=${device} actual=CPU`));
      assert(!text.includes("MEMORY_ACTUAL "));
    }
    if (id === "missing-layer") assert(text.includes("without gaps or overlaps"));
    if (exit === 0) {
      assert.equal(measured.layer_device_query_supported, true);
      assert.equal(measured.layer_device_expectations_checked, expectations.length > 0);
      assert.equal(measured.layer_default_devices.length, layers);
      for (let layer = 0; layer < layers; ++layer) {
        const expected = id === "explicit-cpu" ||
          ((id === "explicit-cpu-offload" || id === "unasserted-counterexample") && layer === 0) ? "CPU" : device;
        assert.deepEqual(measured.layer_default_devices[layer], { layer, device: expected });
      }
    }
  } catch (failure) { error = failure.message; }
  rows.push({ id, exit: run.status, expected_exit: exit, passed: error === null, error, measured });
}
const result = { at_utc: new Date().toISOString(), server, server_sha256: hash(server), model,
  layers, device, under, full, passed: rows.every(r => r.passed), rows };
fs.writeFileSync(path.join(out, "result.json"), JSON.stringify(result, null, 2) + "\n");
console.log(JSON.stringify({ passed: result.passed, cases: rows.map(({ id, passed, error }) => ({ id, passed, error })) }));
if (!result.passed) process.exitCode = 1;

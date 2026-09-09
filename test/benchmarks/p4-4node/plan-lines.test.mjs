import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const cli = fileURLToPath(new URL("./plan-lines.mjs", import.meta.url));
const head = '{"execution_shape":{"n_seq_max":256,"n_ctx":262144},"fits_current_free":true,"entries":[{"scope":"device","free":24436015104,"model":11276284416,"context":9294577664,"compute":3742433280,"required":24313295360}]}';
const tail = '{"execution_shape":{"n_seq_max":256,"n_ctx":262144},"fits_current_free":false,"entries":[{"scope":"device","free":24436015104,"model":11923591680,"context":9294577664,"compute":4169138176,"required":25387307520}]}';

function run(log) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "p4-plan-lines-"));
  try {
    fs.writeFileSync(path.join(dir, "agent.stderr.log"), log);
    return spawnSync(process.execPath, [cli, dir], { encoding: "utf8", cwd: os.tmpdir() });
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

test("the CLI keeps a head allocation separate from the following tail plan", () => {
  const result = run([
    "llama_memory_recurrent: CUDA0 RS buffer size = 0.00 MiB",
    `MEMORY_PLAN ${head}`,
    "llama_memory_recurrent: CUDA0 RS buffer size = 7504.00 MiB",
    `MEMORY_ACTUAL ${head}`,
    "llama_memory_recurrent: CUDA0 RS buffer size = 0.00 MiB",
    `MEMORY_PLAN ${tail}`,
  ].join("\n"));
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /--- MEMORY_PLAN #1:/);
  assert.match(result.stdout, /--- MEMORY_ACTUAL #2:/);
  assert.match(result.stdout, /--- MEMORY_PLAN #3:.*fits_current_free=false/);
  const actual = result.stdout.split("--- MEMORY_ACTUAL #2:")[1].split("--- MEMORY_PLAN #3:")[0];
  const nextPlan = result.stdout.split("--- MEMORY_PLAN #3:")[1];
  assert.match(actual, /7504\.00 MiB/);
  assert.match(nextPlan, /0\.00 MiB/);
  assert.doesNotMatch(nextPlan, /7504\.00 MiB/);
  assert.match(nextPlan, /required=23\.64 GiB/);
});

test("an ACTUAL-only record retains its explicit kind without inventing a plan", () => {
  const result = run(`MEMORY_ACTUAL ${head}\n`);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /--- MEMORY_ACTUAL #1:/);
  assert.doesNotMatch(result.stdout, /--- MEMORY_PLAN/);
});

test("malformed allocation evidence fails instead of appearing absent", () => {
  const result = run("MEMORY_ACTUAL {broken\n");
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /SyntaxError/);
});

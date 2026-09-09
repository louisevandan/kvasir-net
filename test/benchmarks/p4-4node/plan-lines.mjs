#!/usr/bin/env node
// The memory-plan lines of one run, from its own agent log.
//
//   node test/benchmarks/p4-4node/plan-lines.mjs <run directory>
//
// Prints, per MEMORY_PLAN record: the resident set it planned for, the
// device entry's free / model / context / compute / required in GiB, whether
// it fit, and the RS and KV buffer lines that preceded it. That is the
// arithmetic the 2026-09-09 refusals were diagnosed from, and it is what
// decides whether compat patch 0026 did what it claims: a planning pass
// whose RS buffer is 0.00 MiB no longer spends the memory it is planning.
//
// This reads a log. It never launches anything.

import fs from "node:fs";
import path from "node:path";

const dir = process.argv[2];
if (!dir) throw new Error("usage: plan-lines.mjs <run directory>");
const lines = fs.readFileSync(path.join(dir, "agent.stderr.log"), "utf8").split(/\r?\n/);
const gib = (n) => (n / 1073741824).toFixed(2);

let recent = [];
let plans = 0;
for (const line of lines) {
  if (/RS buffer size|KV buffer size|llama_memory_recurrent: size =|llama_kv_cache: size =/.test(line)) {
    recent.push(line.trim());
    continue;
  }
  const at = line.indexOf("MEMORY_PLAN ");
  if (at < 0) continue;
  plans += 1;
  const plan = JSON.parse(line.slice(at + "MEMORY_PLAN ".length));
  const device = plan.entries.find((entry) => entry.scope === "device");
  console.log(`--- plan #${plans}: n_seq_max=${plan.execution_shape.n_seq_max} n_ctx=${plan.execution_shape.n_ctx} fits_current_free=${plan.fits_current_free}`);
  for (const buffer of recent) console.log(`    ${buffer}`);
  if (device) {
    console.log(
      `    device free=${gib(device.free)} model=${gib(device.model)} context=${gib(device.context)} `
      + `compute=${gib(device.compute)} required=${gib(device.required)} GiB`
      + ` (free+context=${gib(device.free + device.context)})`,
    );
  }
  recent = [];
}
if (plans === 0) console.log("no MEMORY_PLAN record in this log");

#!/usr/bin/env node
// Recompute one run's numbers from its own preserved artifacts.
//
//   node test/benchmarks/p4-4node/measure-run.mjs <run directory>
//
// Reads artifact.json, config.json, report.json and gpu.csv. It recomputes
// throughput from the answers rather than reading the harness's metrics
// block, and prints the harness value beside it so a disagreement is visible
// instead of silent. Everything else here is a distribution the metrics block
// summarises differently or not at all: what the answers actually look like,
// where the batch widths sit against UBATCH, and what the cards were doing.
//
// This reads. It never launches an agent, a stage server or a GPU sampler, so
// it cannot produce a number that no run produced.

import fs from "node:fs";
import path from "node:path";

const dir = process.argv[2];
if (!dir) throw new Error("usage: measure-run.mjs <run directory>");
const read = (name) => JSON.parse(fs.readFileSync(path.join(dir, name), "utf8"));
const artifact = read("artifact.json");
const report = read("report.json");
const config = read("config.json");

const sorted = (xs) => [...xs].sort((a, b) => a - b);
const q = (xs, p) => {
  if (!xs.length) return NaN;
  const s = sorted(xs);
  return s[Math.min(s.length - 1, Math.floor(s.length * p))];
};
const mean = (xs) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : NaN);
const r1 = (x) => (Number.isFinite(x) ? Math.round(x * 10) / 10 : x);
const r2 = (x) => (Number.isFinite(x) ? Math.round(x * 100) / 100 : x);

console.log(`=== ${path.basename(dir)}`);
const requests = artifact.requests;
console.log(
  `delivery: requests=${artifact.request_count} completed=${artifact.completed_count} `
  + `released=${artifact.released_count} error=${artifact.error} cleanup_error=${artifact.cleanup_error}`,
);

// ---- the answers themselves ---------------------------------------------
// Independent of judge.mjs: length, script, degeneracy and how they ended.
const stops = {};
for (const a of artifact.acceptance.requests) {
  stops[a.terminal_stop] = (stops[a.terminal_stop] || 0) + 1;
}
const texts = requests.map((r) => r.response || "");
const lengths = texts.map((t) => t.length);
const hangul = texts.map((t) => (t.match(/[가-힣]/g) || []).length / Math.max(1, t.length));
// A degenerate answer loops one phrase. Report the most repeated 24-character
// window as a share of the answer: near 1 is a loop, near 0 is prose.
const looping = texts.map((t) => {
  const w = 24;
  if (t.length < w * 3) return 0;
  const seen = new Map();
  for (let i = 0; i + w <= t.length; i += 1) {
    const key = t.slice(i, i + w);
    seen.set(key, (seen.get(key) || 0) + 1);
  }
  return (Math.max(...seen.values()) * w) / t.length;
});
const unfinished = texts.filter((t) => !/[.!?。]\s*$/.test(t.trim())).length;
console.log(`answers: distinct=${new Set(texts).size}/${texts.length} terminal_stop=${JSON.stringify(stops)}`);
console.log(`answers: chars p10=${q(lengths, 0.1)} p50=${q(lengths, 0.5)} p90=${q(lengths, 0.9)}`);
console.log(`answers: hangul share min=${r2(Math.min(...hangul))} mean=${r2(mean(hangul))}`);
console.log(`answers: most repeated 24-char window share max=${r2(Math.max(...looping))}`);
console.log(`answers: not ending on sentence punctuation ${unfinished}/${texts.length}`);

// ---- throughput and latency ---------------------------------------------
const generated = artifact.acceptance.requests.reduce((n, a) => n + a.generated_tokens, 0);
const wall = artifact.elapsed_ms / 1000;
const total = (key) => requests.reduce((n, r) => n + (r[key] || 0), 0);
console.log(
  `throughput: generated_tokens=${generated} wall_s=${r2(wall)} `
  + `generation_tps=${r2(generated / wall)} (harness ${report.metrics.generation_tps})`,
);
console.log(
  `throughput: prefill_rows=${total("prefill_rows")} decode_rows=${total("decode_rows")} `
  + `verify_rows=${total("verify_rows")} replay_rows=${total("replay_rows")}`,
);
const perToken = requests.map((r) => r.generation_elapsed_ms / Math.max(1, r.decode_rows));
console.log(`latency: inter-token ms p50=${r1(q(perToken, 0.5))} p90=${r1(q(perToken, 0.9))}`);
// TTFT by arrival rank, because a run whose arrivals exceed the resident set
// has two populations and their median is a number nobody waited.
const byArrival = [...requests].sort((a, b) => a.arrival_ms - b.arrival_ms);
const cohort = (lo, hi) => {
  const slice = byArrival.slice(lo, hi);
  const ttft = slice.map((r) => r.first_output_ms - r.arrival_ms);
  const end = slice.map((r) => r.completed_ms - r.arrival_ms);
  console.log(
    `latency: arrival rank ${lo}-${hi} (t=${slice[0].arrival_ms}..${slice[slice.length - 1].arrival_ms} ms) `
    + `TTFT p50=${q(ttft, 0.5)} ms, end-to-end p50=${q(end, 0.5)} ms`,
  );
};
const step = Math.max(1, Math.floor(byArrival.length / 4));
for (let lo = 0; lo < byArrival.length; lo += step) cohort(lo, Math.min(byArrival.length, lo + 32));

// ---- batch saturation ----------------------------------------------------
const observations = artifact.batch_observations;
const physical = observations.flatMap((o) => o.physical_batches);
const kindOf = (p) => (p.prefill_rows > 0 && p.decode_rows > 0 ? "mixed" : p.prefill_rows > 0 ? "prefill" : "decode");
const ubatch = Number(config.nodes[0].n_ubatch);
const rows = physical.map((p) => p.rows);
console.log(
  `batches: physical=${physical.length} ubatch=${ubatch} rows mean=${r2(mean(rows))} `
  + `p50=${q(rows, 0.5)} p90=${q(rows, 0.9)} max=${q(rows, 1)} -> ubatch fill mean=${r2((100 * mean(rows)) / ubatch)}%`,
);
for (const kind of ["decode", "prefill", "mixed"]) {
  const w = physical.filter((p) => kindOf(p) === kind).map((p) => p.rows);
  if (!w.length) continue;
  console.log(`batches: ${kind} n=${w.length} rows mean=${r2(mean(w))} p50=${q(w, 0.5)} max=${q(w, 1)}`);
}
// Width histogram with the idle that preceded each width: a width that is
// always reached after a long wait is a different problem from a narrow one
// issued immediately.
const buckets = new Map();
for (const o of observations) {
  for (const p of o.physical_batches) {
    const key = `${kindOf(p)}:${p.rows}`;
    const b = buckets.get(key) || { n: 0, idle: 0, stage: 0 };
    b.n += 1;
    b.idle += o.idle_ms;
    b.stage += o.stage_ms;
    buckets.set(key, b);
  }
}
console.log("batches: width histogram (count, mean preceding first-node idle ms, mean first-node stage ms)");
for (const [key, b] of [...buckets].sort((a, b2) => b2[1].n - a[1].n).slice(0, 12)) {
  console.log(`  ${key.padEnd(14)} ${String(b.n).padStart(5)} ${(b.idle / b.n).toFixed(1).padStart(8)} ${(b.stage / b.n).toFixed(1).padStart(8)}`);
}
const idle = observations.map((o) => o.idle_ms);
console.log(
  `batches: first-node idle_ms mean=${r1(mean(idle))} p50=${q(idle, 0.5)} p90=${q(idle, 0.9)} `
  + `sum_s=${r1(idle.reduce((a, b) => a + b, 0) / 1000)} of wall ${r2(wall)} s`,
);
console.log(
  `batches: idle_gated=${observations.filter((o) => o.idle_gated).length} `
  + `ready_sequences p50=${q(observations.map((o) => o.ready_sequences), 0.5)} `
  + `max=${q(observations.map((o) => o.ready_sequences), 1)} `
  + `| harness ready_rows_left ${JSON.stringify(report.metrics.ready_rows_left)}`,
);
console.log(`pipeline: ${report.metrics.pipeline.stages.map((s) => `node${s.node} service=${s.service_pct}% stage_mean=${s.stage_ms.mean}ms`).join(" | ")}`);
console.log(
  `pipeline: any_stage_open=${report.metrics.pipeline.any_stage_open_pct}% `
  + `two_or_more=${report.metrics.pipeline.two_or_more_open_pct}% depth_mean=${report.metrics.pipeline.depth_mean}`,
);

// ---- the cards -----------------------------------------------------------
// nvidia-smi CSV: timestamp, index, name, utilization.gpu, memory.used, power.draw
const samples = fs.readFileSync(path.join(dir, "gpu.csv"), "utf8")
  .split(/\r?\n/).filter(Boolean)
  .map((line) => {
    const f = line.split(",").map((s) => s.trim());
    return { t: f[0], gpu: Number(f[1]), util: Number(f[3]), memory: Number(f[4]), watt: Number(f[5]) };
  });
const stamps = [...new Set(samples.map((s) => s.t))];
const byStamp = new Map(stamps.map((t) => [t, {}]));
for (const s of samples) byStamp.get(s.t)[s.gpu] = s;
const ordered = stamps.map((t) => byStamp.get(t));
// Exclude the model-load head and the teardown tail mechanically: the window
// runs from the first timestamp where either card exceeds 5% to the last.
const busy = ordered.map((e) => Math.max(...Object.values(e).map((s) => s.util)) > 5);
const first = busy.indexOf(true);
const last = busy.lastIndexOf(true);
console.log(`gpu: ${stamps.length} timestamps ${stamps[0]} .. ${stamps[stamps.length - 1]}; >5% window = ${first}..${last}`);
for (const g of [...new Set(samples.map((s) => s.gpu))].sort()) {
  const whole = samples.filter((s) => s.gpu === g).map((s) => s.util);
  const window = ordered.slice(first, last + 1).map((e) => e[g]).filter(Boolean);
  const util = window.map((s) => s.util);
  console.log(
    `gpu${g}: util whole=${r1(mean(whole))}% window mean=${r1(mean(util))}% p50=${q(util, 0.5)}% `
    + `p90=${q(util, 0.9)}% max=${q(util, 1)}% at-zero=${util.filter((u) => u === 0).length}/${util.length}`,
  );
  console.log(
    `gpu${g}: power window mean=${r1(mean(window.map((s) => s.watt)))} W max=${q(window.map((s) => s.watt), 1)} W `
    + `| memory max=${q(window.map((s) => s.memory), 1)} MiB`,
  );
}

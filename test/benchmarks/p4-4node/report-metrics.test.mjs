import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildReport } from "./run.mjs";

// Hand-declared artifact facts, not the implementation's calculated answers.
// This tests report assembly only: no model, worker, tokenizer or network runs.
function report(artifact, verdict = {
  passed: true, meaningful: artifact.requests.length, total: artifact.requests.length, results: [],
}) {
  return buildReport({
    runId: "report-regression",
    spec: { name: "synthetic", target: "fixture", description: "report only", nUbatch: 8, cuts: [1, 2] },
    identity: { host: "fixture" },
    artifact,
    build: { ok: true, reason: null },
    verdict,
    sessionKeys: { passed: true },
    delivery: { passed: true },
    channelFailures: [],
    agentStopped: true,
    fence: { ok: true, reason: null, records: [] },
  });
}

function artifact(requests, batches) {
  return {
    passed: true,
    request_count: requests.length,
    completed_count: requests.length,
    released_count: requests.length,
    elapsed_ms: 2000,
    requests,
    batch_observations: [{
      logical_rows: batches.reduce((sum, batch) => sum + batch.rows, 0),
      physical_batches: batches,
      stage_ms: 12,
      idle_ms: 3,
      idle_gated: 2,
      ready_rows: 20,
      ready_sequences: 2,
    }],
    stage_spans: [],
  };
}

test("report counts prefill plus Verify or Replay as mixed physical work", () => {
  const input = artifact([], [
    { rows: 4, prefill_rows: 2, decode_rows: 0, verify_rows: 2, replay_rows: 0 },
    { rows: 3, prefill_rows: 1, decode_rows: 0, verify_rows: 0, replay_rows: 2 },
    { rows: 2, prefill_rows: 0, decode_rows: 0, verify_rows: 2, replay_rows: 0 },
  ]);
  assert.equal(report(input).metrics.mixed_batches, 2);
});

test("report generation counts approved speculative OUTPUTs, not decode rows", () => {
  const input = artifact([{
    request_id: "speculative",
    prefill_rows: 4,
    decode_rows: 0,
    verify_rows: 3,
    replay_rows: 2,
    response: "a complete answer",
    outcomes: [
      { token: 101, text: "a ", stop: null },
      { token: 201, text: "complete ", stop: null },
      { token: 202, text: "answer", stop: "length" },
    ],
  }], [{ rows: 9, prefill_rows: 4, decode_rows: 0, verify_rows: 3, replay_rows: 2 }]);
  const result = report(input);
  assert.equal(result.metrics.generation_tps, 1.5);
  assert.equal(result.metrics.sampled_output_tps, 1.5);
  assert.equal(result.metrics.sampled_tokens, 3);
  assert.equal(result.metrics.generated_tokens, 3);
  assert.deepEqual(result.metrics.legacy_row_rates, { generation_tps: 0, total_tps: 2 });
  assert.equal(result.metrics.metrics_version, 2);
  assert.equal(result.metrics.scope, "current_outer_run");
  assert.equal(result.metrics.denominator, "drive_elapsed_through_release");
  assert.equal("total_tps" in result.metrics, false, "v1 total_tps moved explicitly to legacy_row_rates");
  assert.equal(result.sample_answer, "a complete answer");
  assert.deepEqual(result.structural, { passed: true, requests: 1, completed: 1, released: 1 });
  assert.match(result.metrics.definitions.per_request_logical_generation_tps, /not migrated/);
});

test("empty EOS, UTF-8 fragments and text-bearing events are distinct counters", () => {
  const input = artifact([
    { request_id: "empty-eos", outcomes: [
      { token: 1, text: "answer", stop: null },
      { token: 2, text: "", stop: "eos" },
    ] },
    { request_id: "utf8-fragment", outcomes: [
      { token: 3, text: "", stop: null },
      { token: 4, text: "한", stop: "stop" },
    ] },
    { request_id: "empty-length", outcomes: [
      { token: 5, text: "answer", stop: null },
      { token: 6, text: "", stop: "length" },
    ] },
    { request_id: "nonempty-eos", outcomes: [
      { token: 7, text: "[end]", stop: "eos" },
    ] },
  ], []);
  const result = report(input).metrics;
  assert.equal(result.sampled_tokens, 7);
  assert.equal(result.empty_terminal_eos_tokens, 1);
  assert.equal(result.generated_tokens, 6);
  assert.equal(result.text_bearing_output_events, 4);
  assert.equal(result.sampled_output_tps, 3.5);
  assert.equal(result.generation_tps, 3);
  assert.match(result.definitions.text_bearing_output_events, /not visible or retokenized token count/);
});

test("a one-token empty EOS is sampled work but not generated answer tokens", () => {
  const input = artifact([{
    request_id: "empty", response: "", outcomes: [{ token: 2, text: "", stop: "eos" }],
  }], []);
  input.passed = false;
  const result = report(input, {
    passed: false, meaningful: 0, total: 1,
    results: [{ request_id: "empty", meaningful: false, failures: ["empty response"] }],
  });
  assert.equal(result.metrics.sampled_tokens, 1);
  assert.equal(result.metrics.generated_tokens, 0);
  assert.equal(result.metrics.sampled_output_tps, 0.5);
  assert.equal(result.metrics.generation_tps, 0);
  assert.equal(result.metrics.text_bearing_output_events, 0);
  assert.equal(result.structural.passed, false);
  assert.equal(result.meaning.passed, false);
  assert.equal(result.rejected.length, 1);
  assert.equal("valid_generation_tps" in result.metrics, false);
  assert.equal("useful_generation_tps" in result.metrics, false);
});

test("ordinary first-token generation and legacy row rates keep different meanings", () => {
  const input = artifact([{
    request_id: "ordinary", response: "abc", prefill_rows: 4, decode_rows: 2,
    outcomes: [
      { token: 1, text: "a", stop: null },
      { token: 2, text: "b", stop: null },
      { token: 3, text: "c", stop: "length" },
    ],
  }], [
    { rows: 4, prefill_rows: 4, decode_rows: 0, verify_rows: 0, replay_rows: 0 },
    { rows: 2, prefill_rows: 0, decode_rows: 2, verify_rows: 0, replay_rows: 0 },
  ]);
  const result = report(input).metrics;
  assert.equal(result.generation_tps, 1.5);
  assert.deepEqual(result.legacy_row_rates, { generation_tps: 1, total_tps: 3 });
  assert.equal(result.prefill_rows, 4);
  assert.equal(result.decode_rows, 2);
  assert.equal(result.mixed_batches, 0);
});

test("empty stop is sampled generation and text length does not estimate token count", () => {
  const input = artifact([{
    request_id: "stop", outcomes: [
      { token: 1, text: "a long piece of decoded text", stop: null },
      { token: 2, text: "", stop: "stop" },
    ],
  }], []);
  const result = report(input).metrics;
  assert.equal(result.sampled_tokens, 2);
  assert.equal(result.generated_tokens, 2);
  assert.equal(result.empty_terminal_eos_tokens, 0);
  assert.equal(result.text_bearing_output_events, 1);
  assert.equal(result.generation_tps, 1);
});

test("the declared denominator remains drive elapsed, not per-request terminal time", () => {
  const input = artifact([
    { request_id: "early", arrival_ms: 0, first_output_ms: 100, completed_ms: 100,
      outcomes: [{ token: 1, text: "a", stop: "length" }] },
    { request_id: "later-wave", arrival_ms: 500, first_output_ms: 800, completed_ms: 900,
      outcomes: [
        { token: 2, text: "b", stop: null },
        { token: 3, text: "c", stop: "length" },
      ] },
  ], []);
  const result = report(input).metrics;
  assert.equal(result.wall_s, 2);
  assert.equal(result.generation_tps, 1.5);
  assert.equal(result.denominator, "drive_elapsed_through_release");
});

test("physical width and pacing are not replaced by an OUTERs owned output count", () => {
  const input = artifact([{
    request_id: "owner-a", response: "a", prefill_rows: 1, decode_rows: 0,
    outcomes: [{ token: 1, text: "a", stop: "length" }],
  }], [
    // Global physical work includes other owners: the reporter must not
    // replace it by this OUTER's one row or its one sampled token.
    { rows: 8, prefill_rows: 4, decode_rows: 0, verify_rows: 4, replay_rows: 0 },
    { rows: 4, prefill_rows: 1, decode_rows: 1, verify_rows: 0, replay_rows: 2 },
  ]);
  const before = structuredClone(input);
  const result = report(input).metrics;
  assert.equal(result.physical_batches, 2);
  assert.equal(result.rows_per_batch, 6);
  assert.equal(result.ubatch_fill_pct, 75);
  assert.equal(result.ms_per_batch, 1000);
  assert.equal(result.mixed_batches, 2);
  assert.equal(result.ready_rows_left.mean, 8);
  assert.equal(result.first_node_stage_ms.mean, 12);
  assert.equal(result.first_node_idle_ms.mean, 3);
  assert.equal(result.first_node_gate_refusals, 2);
  assert.deepEqual(input, before, "report generation cannot rewrite the source artifact");
});

test("zero measurement duration leaves rates unavailable without hiding token counts", () => {
  const input = artifact([{
    request_id: "zero", outcomes: [{ token: 1, text: "a", stop: "length" }],
  }], []);
  input.elapsed_ms = 0;
  const result = report(input).metrics;
  assert.equal(result.sampled_tokens, 1);
  assert.equal(result.generated_tokens, 1);
  assert.equal(result.sampled_output_tps, null);
  assert.equal(result.generation_tps, null);
  assert.deepEqual(result.legacy_row_rates, { generation_tps: null, total_tps: null });
});

test("missing preserved OUTPUT evidence is not invented from decode rows", () => {
  const input = artifact([{ request_id: "missing", decode_rows: 100 }], []);
  assert.throws(() => report(input), /has no preserved OUTPUT array/);
  input.requests[0].outcomes = [{ token: 1, text: null, stop: "eos" }];
  assert.throws(() => report(input), /has an invalid OUTPUT text/);
});

test("exporting the report consumer preserves the direct CLI entry point", () => {
  // No scenario: main rejects before creating directories, agents or samplers.
  // Imported tests above execute the same buildReport consumer without main.
  const run = spawnSync(process.execPath, [fileURLToPath(new URL("./run.mjs", import.meta.url))], {
    encoding: "utf8", timeout: 5000, windowsHide: true,
  });
  assert.equal(run.status, 1);
  assert.match(run.stderr, /P4_4NODE_FAILED usage: run\.mjs/);
});

function span(node, ids = [11, 12]) {
  return {
    node, load_generation: 7, session_id: "s", execution_ids: ids,
    executions: ids.map((id) => ({ execution_id: id,
      owned_requests: [{ request_id: "a", sequence_id: 0, incarnation: 1 }] })),
    rows: 8, ingress_unix_ms: 100, start_unix_ms: 110,
    end_unix_ms: 150, forward_unix_ms: 160,
  };
}

test("report deduplicates replay by stage execution identity, not time or list order", () => {
  const input = artifact([], []);
  input.stage_spans = [span(0), span(0, [12, 11]), span(1)];
  const result = report(input).metrics.pipeline;
  assert.equal(result.spans, 2);
  assert.equal(result.stages[0].batches, 1);
  assert.equal(result.stages[1].batches, 1);
  assert.equal(result.depth_peak, 2);
  const before = structuredClone(input);
  report(input);
  assert.deepEqual(input, before);
});

test("report rejects changed times, owner body and overlapping physical groups", () => {
  for (const field of ["ingress_unix_ms", "start_unix_ms", "end_unix_ms", "forward_unix_ms", "rows"]) {
    const changed = span(0);
    changed[field] += 1;
    const input = artifact([], []);
    input.stage_spans = [span(0), changed];
    assert.throws(() => report(input), /conflicting stage-span body/);
  }
  const input = artifact([], []);
  const changed = span(0);
  changed.executions[0].owned_requests[0].incarnation = 2;
  input.stage_spans = [span(0), changed];
  assert.throws(() => report(input), /conflicting stage-span body/);
  input.stage_spans = [span(0), span(0, [12, 13])];
  assert.throws(() => report(input), /overlapping span groups/);
});

test("report stage scope separates load sessions and rejects unknown stage numbers", () => {
  const input = artifact([], []);
  const other = span(0);
  other.session_id = "another";
  input.stage_spans = [span(0), other];
  assert.equal(report(input).metrics.pipeline.spans, 2);
  assert.equal(report(input).metrics.pipeline.depth_peak, 4);
  input.stage_spans = [span(2)];
  assert.throws(() => report(input), /invalid stage-span identity/);
});

test("late telemetry does not silently change release-boundary TPS", () => {
  const input = artifact([{ request_id: "a", outcomes: [{ token: 1, text: "answer", stop: "length" }] }], []);
  input.elapsed_ms = 2000;
  input.telemetry_complete_elapsed_ms = 5000;
  const result = report(input).metrics;
  assert.equal(result.wall_s, 2);
  assert.equal(result.telemetry_complete_wall_s, 5);
  assert.equal(result.generation_tps, 0.5);
});

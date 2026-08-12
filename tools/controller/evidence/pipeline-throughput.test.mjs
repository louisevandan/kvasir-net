import { test } from 'node:test';
import assert from 'node:assert/strict';
import { summarizeThroughput, throughputReport } from './pipeline-throughput.mjs';

// Shape taken from summary-four-node-c16-coalesce5-20260812-1809.json, the run
// that passed 16/16 with zero errors while every adapter batch held one request.
const fourNodeRun = {
  concurrent_requests: 16,
  agent_slot_width: 1,
  generated_token_events: 249,
  model_load_options: { batching: { max_sequences: 16 } },
  pipeline_occupancy: { active: 1, in_flight: 1, peak: 1, limit: 16, capacity: 16 },
  p4_latency_ms: { parallel_requests_ms: 36731.623, parallel_completed: 16 }
};

test('a correct run whose sessions never overlapped is reported as serialized', () => {
  const throughput = summarizeThroughput(fourNodeRun, { generated_tokens: 256, done: 16 });
  assert.equal(throughput.verdict, 'serialized');
  assert.match(throughput.detail, /NodeSlot holds one permit/);
  assert.equal(throughput.admission.native_peak, 1);
  assert.equal(throughput.admission.native_limit, 16);
  assert.equal(throughput.aggregate_tps, 6.969);
  assert.equal(throughput.per_stream_tps, 0.436);
});

test('a full-width run is reported as batched', () => {
  const throughput = summarizeThroughput(
    { ...fourNodeRun, agent_slot_width: 16, pipeline_occupancy: { peak: 16, limit: 16 } },
    { generated_tokens: 256, done: 16 }
  );
  assert.equal(throughput.verdict, 'batched');
});

test('a partially filled pipeline is separated from a serialized one', () => {
  const throughput = summarizeThroughput(
    { ...fourNodeRun, agent_slot_width: 16, pipeline_occupancy: { peak: 4, limit: 16 } },
    { generated_tokens: 256, done: 16 }
  );
  assert.equal(throughput.verdict, 'partial');
  assert.match(throughput.detail, /4 of 16/);
});

test('missing native telemetry is unknown rather than a fabricated pass', () => {
  const { pipeline_occupancy, ...withoutTelemetry } = fourNodeRun;
  const throughput = summarizeThroughput(withoutTelemetry, { generated_tokens: 256, done: 16 });
  assert.equal(throughput.verdict, 'unknown');
  assert.equal(throughput.admission.native_peak, null);
  assert.equal(throughput.meets_reference, null);
});

test('the reference bound compares aggregate throughput, not per-stream', () => {
  const slow = summarizeThroughput({ ...fourNodeRun, reference_tps: 66 }, { generated_tokens: 256, done: 16 });
  assert.equal(slow.meets_reference, false);
  const fast = summarizeThroughput(
    { ...fourNodeRun, reference_tps: 66, p4_latency_ms: { parallel_requests_ms: 1000, parallel_completed: 16 } },
    { generated_tokens: 256, done: 16 }
  );
  assert.equal(fast.meets_reference, true);
});

test('the report names the verdict and both admission widths', () => {
  const markdown = throughputReport(summarizeThroughput(fourNodeRun, { generated_tokens: 256, done: 16 }));
  assert.match(markdown, /## Throughput and admission/);
  assert.match(markdown, /\| Arrivals \/ Agent slot width \| 16 \/ 1 \|/);
  assert.match(markdown, /\| Native peak \/ limit \| 1 \/ 16 \|/);
  assert.match(markdown, /serialized/);
});

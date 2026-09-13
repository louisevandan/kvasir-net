import { test } from 'node:test';
import assert from 'node:assert/strict';
import { analyzeTrace } from './analyze-trace.mjs';

function fixture() {
  return { request_count: 1, completed_count: 0, released_count: 0, passed: false,
    error: 'deadline', cleanup_error: 'busy', evidence_missing: { stage_executions: 1 },
    stage_builds: [{ node: 'head' }, { node: 'tail' }],
    batch_observations: [{ load_generation: 7, session_id: 's', physical_batches: [
      { execution_id: 1, owned_requests: [{ request_id: 'r', submission_event_id: 'submit',
        incarnation: 2, sequence_id: 0, rows: [{ phase: 'prefill', position: 0 }] }] }] }],
    stage_spans: [{ node: 0, load_generation: 7, session_id: 's', execution_ids: [1],
      ingress_unix_ms: 100, start_unix_ms: 103, end_unix_ms: 113, forward_unix_ms: 115 }] };
}

test('partial work preserves independent missing evidence, first error and cleanup error', () => {
  const r = analyzeTrace(fixture());
  assert.deepEqual(r.stages.map(s => [s.queue_ms, s.rpc_ms, s.return_publish_ms, s.missing_executions]),
    [[3, 10, 2, 0], [0, 0, 0, 1]]);
  assert.equal(r.stages[1].rpc_p50_ms, null);
  assert.equal(r.first_error, 'deadline'); assert.equal(r.cleanup_error, 'busy');
  assert.equal(r.observed_prefill[0].unique_positions, 1);
  assert.equal(r.limits.pure_kernel_ms, null); assert.equal(r.limits.slo_projection, null);
});

test('duplicate, foreign-epoch and malformed clocks cannot inflate measured work', () => {
  for (const mutate of [
    a => a.stage_spans.push(a.stage_spans[0]),
    a => a.stage_spans[0].load_generation++,
    a => a.stage_spans[0].end_unix_ms = 102,
    a => a.batch_observations.push(a.batch_observations[0]),
    a => delete a.stage_spans[0].forward_unix_ms,
  ]) { const a = fixture(); mutate(a); assert.throws(() => analyzeTrace(a)); }
});

test('unobserved work stays unknown and gaps are not filled', () => {
  const a = fixture();
  a.batch_observations[0].physical_batches[0].owned_requests[0].rows.push({ phase: 'prefill', position: 3 });
  const r = analyzeTrace(a);
  assert.equal(r.observed_prefill[0].unique_positions, 2);
  assert.equal(r.observed_prefill[0].contiguous_from_zero, false);
  assert.equal(r.limits.cross_host_transit_ms, null);
});

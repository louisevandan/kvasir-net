import assert from 'node:assert/strict';
import test from 'node:test';
import { buildGroupStats } from './pipeline-group-stats.mjs';

test('records batched tokens per compute frame and latest wavefront occupancy', () => {
  const stats = buildGroupStats({
    phase: 'running',
    processes: [{
      phase: 'running',
      identity: { stageIndex: 0, nodeId: 'gpu-a' },
      logs: [
        { message: '[linker_stage_aggregate] compute_frames=5 batched_tokens=3' },
        '[linker_wire_summary] local_hidden_send_frames=12',
        '[linker_stage_aggregate] compute_frames=41596 batched_tokens=41594',
        '[linker_scheduler] op=wavefront active=2 in_flight=2 peak=32 limit=32 capacity=32'
      ]
    }],
    observability: { transfers: [{ request_id: 'r1', total_bytes: 42 }] }
  }, [{ id: 'gpu-a', gpu_uuid: 'gpu-uuid-a' }]);

  assert.equal(stats.stages[0].tokens_per_frame, 41594 / 41596);
  assert.deepEqual(stats.stages[0].occupancy, {
    active: 2, in_flight: 2, peak: 32, limit: 32, capacity: 32
  });
  assert.deepEqual(stats.pipeline_occupancy, stats.stages[0].occupancy);
  assert.equal(stats.stages[0].shared_memory.sent_frames, 12);
  assert.equal(stats.transfers[0].request_id, 'r1');
});

test('uses stable null diagnostics when a process has not emitted aggregate telemetry', () => {
  const stats = buildGroupStats({
    phase: 'starting',
    processes: [{ phase: 'starting', logs: [] }]
  }, [{ id: 'gpu-a' }]);

  assert.equal(stats.stages[0].tokens_per_frame, null);
  assert.equal(stats.stages[0].occupancy, null);
  assert.equal(stats.pipeline_occupancy, null);
});

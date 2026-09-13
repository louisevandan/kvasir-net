import test from 'node:test';
import assert from 'node:assert/strict';
import { correlateCost } from './correlate-cost.mjs';

function fixture() {
  const artifact = { request_count: 1, completed_count: 1, released_count: 1, passed: true,
    error: null, cleanup_error: null, evidence_missing: null,
    stage_builds: [{ agent: 'tcp://host:1', node: 'head' }, { agent: 'tcp://host:1', node: 'tail' }],
    batch_observations: [{ load_generation: 7, session_id: 's', physical_batches: [
      { execution_id: 1, owned_requests: [{ request_id: 'r', submission_event_id: 'submit',
        incarnation: 2, sequence_id: 0, rows: [{ phase: 'prefill', position: 0 }] }] }] }],
    stage_spans: [0, 1].map(node => ({ node, load_generation: 7, session_id: 's', execution_ids: [1],
      ingress_unix_ms: 100, start_unix_ms: 103, end_unix_ms: 113, forward_unix_ms: 115 })) };
  const sources = [{ agent: 'tcp://host:1', processes: [0, 1].map(node => ({ node, pid: node + 10, binary_sha256: 'a'.repeat(64) })),
    cost: { diagnostic_conformance: true, calls: [11, 10].map(pid => ({ pid, call: 1,
      bindings: [{ execution: 1, load: 7, session: '73', rows: 1 }], end: { total_us: 9000 } })) } }];
  return { artifact, sources };
}

test('join uses witnessed process identity rather than log order', () => {
  const { artifact, sources } = fixture(), r = correlateCost(artifact, sources);
  assert.equal(r.cost_conformance, true);
  assert.deepEqual(r.spans.map(s => [s.node, s.native.pid, s.worker_queue_us, s.worker_return_publish_us]),
    [[0, 10, 3000, 2000], [1, 11, 3000, 2000]]);
  assert.equal(r.spans[0].rpc_minus_native_us, 1000);
  assert.deepEqual(r.spans[0].native.phases, { prefill: 1 });
  assert.equal(r.limits.cross_host_transit_us, null);
});

test('missing cost or return fails conformance; missing return is never zero', () => {
  const { artifact, sources } = fixture();
  sources[0].cost.calls.pop();
  const r = correlateCost(artifact, sources);
  assert.equal(r.cost_conformance, false);
  assert.equal(r.spans[0].native, null);
  delete artifact.stage_spans[0].forward_unix_ms;
  assert.throws(() => correlateCost(artifact, sources), /clock/);
});

test('foreign host, rows, epoch, duplicate mapping or duplicate claim reject', () => {
  for (const mutate of [
    s => s[0].agent = 'tcp://other:1',
    s => s[0].cost.calls[0].bindings[0].rows++,
    s => s[0].cost.calls[0].bindings[0].load++,
    s => s[0].processes.push(s[0].processes[0]),
    s => s[0].cost.calls.push(s[0].cost.calls[0]),
  ]) {
    const { artifact, sources } = fixture(); mutate(sources);
    assert.throws(() => correlateCost(artifact, sources));
  }
});

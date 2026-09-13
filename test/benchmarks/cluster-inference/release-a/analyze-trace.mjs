import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const hash = data => crypto.createHash('sha256').update(data).digest('hex');
const requireValue = (condition, message) => { if (!condition) throw new Error(message); };
const integer = x => Number.isSafeInteger(x) && x >= 0;
const percentile = (values, p) => [...values].sort((a, b) => a - b)[Math.ceil(values.length * p) - 1];
const identity = x => JSON.stringify([x.load_generation, x.session_id]);

// All differences are within one worker's clock. Never infer cross-host transit
// from wall-clock timestamps without independently measured clock skew.
export function analyzeTrace(a) {
  requireValue(Array.isArray(a.stage_builds) && a.stage_builds.length > 0, 'missing stage identities');
  requireValue(Array.isArray(a.stage_spans) && Array.isArray(a.batch_observations), 'missing observations');
  requireValue(integer(a.request_count) && a.request_count > 0, 'missing request count');
  requireValue(integer(a.completed_count) && integer(a.released_count), 'missing terminal counts');
  const batches = new Map(), positions = new Map();
  for (const b of a.batch_observations) {
    requireValue(integer(b.load_generation) && typeof b.session_id === 'string', 'missing batch authority');
    for (const physical of b.physical_batches) {
      const key = identity(b) + ':' + physical.execution_id;
      requireValue(integer(physical.execution_id) && !batches.has(key), 'duplicate execution');
      batches.set(key, physical);
      for (const owner of physical.owned_requests) {
        const request = JSON.stringify([b.load_generation, b.session_id, owner.submission_event_id,
          owner.request_id, owner.incarnation, owner.sequence_id]);
        requireValue(typeof owner.submission_event_id === 'string', 'missing submission authority');
        const set = positions.get(request) ?? new Set();
        for (const row of owner.rows) {
          requireValue(integer(row.position), 'invalid token position');
          if (row.phase === 'prefill') {
            requireValue(!set.has(row.position), 'duplicate prefill position');
            set.add(row.position);
          }
        }
        positions.set(request, set);
      }
    }
  }
  const stages = a.stage_builds.map((build, node) => ({ node, build, spans: [], executions: new Set() }));
  for (const span of a.stage_spans) {
    const stage = stages[span.node];
    requireValue(integer(span.node) && stage, 'unknown stage');
    const times = [span.ingress_unix_ms, span.start_unix_ms, span.end_unix_ms, span.forward_unix_ms];
    requireValue(times.every(integer) && times.every((t, i) => i === 0 || t >= times[i - 1]), 'invalid local clock order');
    requireValue(Array.isArray(span.execution_ids) && span.execution_ids.length > 0, 'missing execution ids');
    for (const id of span.execution_ids) {
      const key = identity(span) + ':' + id;
      requireValue(batches.has(key), 'unmatched stage execution');
      requireValue(!stage.executions.has(key), 'duplicate stage execution');
      stage.executions.add(key);
    }
    stage.spans.push({ queue: times[1] - times[0], rpc: times[2] - times[1], returned: times[3] - times[2] });
  }
  const cost = stages.map(s => ({ node: s.node, build: s.build, observed_spans: s.spans.length,
    observed_executions: s.executions.size, missing_executions: batches.size - s.executions.size,
    queue_ms: s.spans.reduce((v, x) => v + x.queue, 0),
    rpc_ms: s.spans.reduce((v, x) => v + x.rpc, 0),
    return_publish_ms: s.spans.reduce((v, x) => v + x.returned, 0),
    rpc_p50_ms: s.spans.length ? percentile(s.spans.map(x => x.rpc), .5) : null,
    rpc_p95_ms: s.spans.length ? percentile(s.spans.map(x => x.rpc), .95) : null,
  }));
  return { schema: 'p4.release-a.trace-cost.v1', request_count: a.request_count,
    completed_count: a.completed_count, released_count: a.released_count, passed: a.passed,
    first_error: a.error, cleanup_error: a.cleanup_error, evidence_missing: a.evidence_missing,
    elapsed_ms: a.elapsed_ms, batch_count: batches.size,
    observed_prefill: [...positions].map(([request, set]) => {
      const sorted = [...set].sort((a,b) => a-b);
      return { authority: JSON.parse(request), unique_positions: set.size,
        first: sorted[0] ?? null, last: sorted.at(-1) ?? null,
        contiguous_from_zero: sorted.every((v,i) => v === i) };
    }), stages: cost,
    limits: { rpc_includes: 'native request, native execution, transport and capsule decode',
      pure_kernel_ms: null, cross_host_transit_ms: null, cross_host_overlap: null,
      missing_execution_state: 'unknown; neither compute nor lost return is inferred',
      current_runtime_acceptance: false, slo_projection: null } };
}

export function analyzeFile(input) {
  const bytes = fs.readFileSync(input);
  return { input: { path: path.resolve(input), bytes: bytes.length, sha256: hash(bytes) },
    ...analyzeTrace(JSON.parse(bytes)) };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [input, output] = process.argv.slice(2);
  requireValue(input && output, 'usage: node analyze-trace.mjs artifact.json fresh-output.json');
  fs.writeFileSync(output, JSON.stringify(analyzeFile(input), null, 2) + '\n', { flag: 'wx' });
}

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { analyzeTrace } from './analyze-trace.mjs';

const requireValue = (v, message) => { if (!v) throw new Error(message); };
const identity = (load, session, execution) => JSON.stringify([load, session, execution]);

// A PID is meaningful only within its captured host. Never infer node order
// from the order in which stderr records happen to arrive.
export function correlateCost(artifact, sources) {
  const worker = analyzeTrace(artifact), seenNodes = new Set(), stageCalls = new Map();
  const batches = new Map();
  for (const observation of artifact.batch_observations) {
    for (const batch of observation.physical_batches) {
      const rows = batch.owned_requests.flatMap(r => r.rows);
      batches.set(identity(observation.load_generation, observation.session_id, batch.execution_id), rows);
    }
  }
  let nativeComplete = true;
  for (const source of sources) {
    const pids = new Map();
    for (const process of source.processes) {
      const stage = artifact.stage_builds[process.node];
      requireValue(stage && stage.agent === source.agent, 'process host does not match stage');
      requireValue(!seenNodes.has(process.node) && !pids.has(process.pid), 'duplicate process mapping');
      requireValue(Number.isSafeInteger(process.pid) && process.pid > 0 && /^[a-f0-9]{64}$/.test(process.binary_sha256), 'missing process witness');
      seenNodes.add(process.node); pids.set(process.pid, process.node);
    }
    nativeComplete &&= source.cost.diagnostic_conformance === true;
    for (const call of source.cost.calls) {
      requireValue(pids.has(call.pid), 'native call has no observed process');
      const node = pids.get(call.pid);
      const list = stageCalls.get(node) ?? [];
      const keys = [];
      const phases = {};
      for (const binding of call.bindings) {
        const bytes = Buffer.from(binding.session, 'hex'), session = bytes.toString('utf8');
        requireValue(Buffer.from(session).equals(bytes), 'invalid session encoding');
        const key = identity(binding.load, session, binding.execution);
        const rows = batches.get(key);
        requireValue(rows && rows.length === binding.rows, 'native binding differs from admitted rows');
        keys.push(key);
        for (const row of rows) phases[row.phase] = (phases[row.phase] ?? 0) + 1;
      }
      list.push({ pid: call.pid, call: call.call, keys, phases, observation: call });
      stageCalls.set(node, list);
    }
  }
  const spans = [], used = new Set();
  for (const span of artifact.stage_spans) {
    const keys = span.execution_ids.map(id => identity(span.load_generation, span.session_id, id)).sort();
    const matches = (stageCalls.get(span.node) ?? []).filter(c =>
      JSON.stringify([...c.keys].sort()) === JSON.stringify(keys));
    requireValue(matches.length <= 1, 'multiple native calls claim one worker span');
    const call = matches[0];
    if (call) {
      const id = `${span.node}:${call.pid}:${call.call}`;
      requireValue(!used.has(id), 'native cost counted twice'); used.add(id);
    }
    const rpc = (span.end_unix_ms - span.start_unix_ms) * 1000;
    spans.push({ node: span.node, execution_ids: span.execution_ids,
      load_generation: span.load_generation, session_id: span.session_id,
      worker_queue_us: (span.start_unix_ms - span.ingress_unix_ms) * 1000,
      worker_rpc_us: rpc, worker_return_publish_us: (span.forward_unix_ms - span.end_unix_ms) * 1000,
      native: call ?? null,
      // Millisecond worker timestamps and native steady-clock durations have
      // distinct precision; this residual is not a network measurement.
      rpc_minus_native_us: call?.observation.end ? rpc - call.observation.end.total_us : null });
  }
  const unmapped = [...stageCalls].flatMap(([node, calls]) => calls
    .filter(c => !used.has(`${node}:${c.pid}:${c.call}`)).map(c => ({ node, ...c })));
  return { schema: 'p4.release-a.correlated-cost.v1', worker, spans, unmapped_native_calls: unmapped,
    cost_conformance: artifact.passed === true && nativeComplete && seenNodes.size === artifact.stage_builds.length &&
      spans.length > 0 && spans.every(s => s.native !== null) && unmapped.length === 0 &&
      worker.stages.every(s => s.missing_executions === 0),
    limits: { cross_host_transit_us: null, pure_kernel_us: null, release_acceptance: false,
      client_output: 'artifact outcomes/acceptance remain the authority; native sample completion is not delivery' } };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [artifact, cost, processes, output] = process.argv.slice(2);
  requireValue(output, 'usage: node correlate-cost.mjs artifact.json cost.json native-processes.json fresh-output.json');
  const a = JSON.parse(fs.readFileSync(artifact));
  requireValue(new Set(a.stage_builds.map(s => s.agent)).size === 1, 'CLI requires a single host; use explicit sources for fleet');
  const result = correlateCost(a, [{ agent: a.stage_builds[0].agent,
    cost: JSON.parse(fs.readFileSync(cost)), processes: JSON.parse(fs.readFileSync(processes)) }]);
  fs.writeFileSync(output, JSON.stringify(result, null, 2) + '\n', { flag: 'wx' });
  if (!result.cost_conformance) process.exitCode = 1;
}

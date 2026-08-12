import { readFile, writeFile } from 'node:fs/promises';
import { distribution, writeSummary } from './run-evidence.mjs';

const [root, stamp, localHost = 'http://127.0.0.1:18089', remoteHost = 'http://127.0.0.1:28083'] = process.argv.slice(2);
if (!root || !stamp) throw new Error('usage: node recover-pipeline-evidence.mjs <root> <stamp> [localHost] [remoteHost]');

const path = (name) => `${root}\\${name}-${stamp}`;
const traces = (await readFile(`${path('trace')}.jsonl`, 'utf8')).trim().split(/\r?\n/).map(JSON.parse);
const gpuSamples = (await readFile(`${path('summary')}-gpu.jsonl`, 'utf8')).trim().split(/\r?\n/).map(JSON.parse);
const plan = JSON.parse(await readFile(`${path('plan')}.json`, 'utf8'));
const ringLog = await readFile(`${root}\\ingress256-services\\p4-adapter-local.err.log`, 'utf8');
const clientLog = await readFile(`${path('client')}.log`, 'utf8');

const numericFields = (line) => Object.fromEntries(
  [...line.matchAll(/([a-z0-9_]+)=([0-9]+(?:\.[0-9]+)?)/g)].map(([, key, value]) => [key, Number(value)])
);
const asText = (entry) => typeof entry === 'string' ? entry : entry?.message ?? entry?.text ?? '';

async function findGroup(host) {
  const response = await fetch(`${host}/api/runtime-groups`);
  if (!response.ok) throw new Error(`${host} runtime groups HTTP ${response.status}`);
  const groups = (await response.json()).groups ?? [];
  const group = groups.findLast((candidate) => candidate.controller_id?.includes(stamp));
  if (!group) throw new Error(`${host} has no group for ${stamp}`);
  return group;
}

function groupStats(group) {
  return {
    group_phase: group.phase,
    stages: (group.processes ?? []).map((process, index) => {
      const messages = (process.logs ?? []).map(asText);
      const latest = (marker) => [...messages].reverse().find((message) => message.includes(marker)) ?? '';
      const wire = numericFields(latest('[linker_wire_summary]'));
      return {
        stage_index: process.identity?.stageIndex ?? index,
        node_id: process.identity?.nodeId,
        phase: process.phase,
        shared_memory: {
          sent_frames: wire.local_hidden_send_frames ?? 0,
          received_frames: wire.local_hidden_recv_frames ?? 0
        },
        wire,
        request: numericFields(latest('[linker_request_summary]')),
        aggregate: numericFields(latest('[linker_stage_aggregate]'))
      };
    }),
    transfers: (group.observability?.transfers ?? []).map((transfer) => ({
      request_id: transfer.request_id,
      total_bytes: transfer.total_bytes,
      elapsed_ms: transfer.elapsed_ms,
      average_bytes_per_second: transfer.average_bytes_per_second,
      source_to_destination: transfer.source_to_destination
    }))
  };
}

function gpuSummary(samples) {
  const groups = new Map();
  for (const sample of samples) groups.set(sample.uuid, [...(groups.get(sample.uuid) ?? []), sample]);
  return [...groups].map(([uuid, values]) => ({
    uuid,
    samples: values.length,
    gpu_pct: distribution(values.map((sample) => sample.gpu_pct)),
    memory_pct: distribution(values.map((sample) => sample.memory_pct)),
    memory_mib: distribution(values.map((sample) => sample.memory_mib)),
    power_w: distribution(values.map((sample) => sample.power_w))
  }));
}

const ringResults = [...ringLog.matchAll(/P4_ADAPTER_BATCH_RESULT\s+(\{[^\r\n]+\})/g)].map((match) => JSON.parse(match[1]));
const acceptedAbsolute = traces.map((trace) => trace.controller_wait_ms + trace.responses.find((item) => item.type === 'INGRESS_ACCEPTED').elapsed_ms);
const doneAbsolute = traces.map((trace) => trace.controller_wait_ms + trace.responses.find((item) => item.type === 'DONE').elapsed_ms);
const waits = traces.map((trace) => trace.controller_wait_ms);
const elapsed = Number(clientLog.match(/P4_PARALLEL_PASS[^\r\n]*elapsed_ms=([0-9.]+)/)?.[1]);
const local = groupStats(await findGroup(localHost));
const remote = groupStats(await findGroup(remoteHost));
const stages = [...local.stages, ...remote.stages].sort((left, right) => left.stage_index - right.stage_index);
const transfers = [...local.transfers, ...remote.transfers];
const runStats = {
  model: 'unsloth/Ornith-1.0-35B-GGUF/Ornith-1.0-35B-UD-Q5_K_S.gguf',
  prompt_fixture: 'apps/p4/fixtures/prefill-prompts-ko-400t.json',
  prompt_target_tokens: 400,
  benchmark: true,
  benchmark_ignore_eog: true,
  requested_max_tokens: 600,
  parallel: 256,
  concurrent_requests: 256,
  execution_window: 256,
  native_parallel: 16,
  batch: 256,
  ubatch: 128,
  stage_layers: [[0, 11], [11, 17], [17, 29], [29, 40]],
  stage_nodes: ['local-3090', 'local-4080', 'remote-3090-a', 'remote-3090-b'],
  context_tokens: 18432,
  context_per_request_tokens: 1152,
  generated_token_events: traces.reduce((sum, trace) => sum + trace.responses.filter((item) => item.type === 'TOKEN').length, 0),
  response_chars: traces.reduce((sum, trace) => sum + trace.final_text.length, 0),
  p4_latency_ms: { parallel_requests_ms: elapsed },
  ingress_concurrency: {
    controller_wait_ms: distribution(waits),
    accepted_absolute_ms: distribution(acceptedAbsolute),
    first_done_absolute_ms: Math.min(...doneAbsolute),
    last_accepted_absolute_ms: Math.max(...acceptedAbsolute),
    all_accepted_before_first_done: Math.max(...acceptedAbsolute) < Math.min(...doneAbsolute),
    observed_peak_agent_connections: 256,
    observed_peer_pipeline_connections: 1
  },
  native_batches: {
    count: Math.ceil(traces.length / 16),
    results: ringResults.length,
    prompt_tokens: ringResults.reduce((sum, result) => sum + result.prompt_n, 0),
    generated_tokens: ringResults.reduce((sum, result) => sum + result.predicted_n, 0),
    prefill_tps: distribution(ringResults.map((result) => result.prompt_n * 1000 / result.prompt_ms)),
    generation_tps: distribution(ringResults.map((result) => result.predicted_n * 1000 / result.predicted_ms)),
    request_duration_ms: distribution(ringResults.map((result) => result.request_duration_ms))
  },
  gpu_telemetry: { trace_file: `${path('summary')}-gpu.jsonl`, summary: gpuSummary(gpuSamples) },
  group_phase: local.group_phase,
  stages,
  transfers,
  request_plan_count: plan.requests?.length
};

const summaryFile = `${path('summary')}.json`;
const reportFile = `${path('report')}.md`;
await writeSummary(summaryFile, reportFile, runStats, traces);
const summary = JSON.parse(await readFile(summaryFile, 'utf8'));
console.log(JSON.stringify({ run: summary.run, request_response: summary.request_response }, null, 2));

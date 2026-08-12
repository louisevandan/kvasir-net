// Throughput and admission verdict for a parallel Pipeline run.
//
// The run that motivated this module completed 16/16 with zero errors and was
// still wrong: every adapter batch carried one request because the Agent
// NodeSlot held a single permit. Correctness alone cannot report that, so the
// summary carries the two numbers that can: aggregate tokens per second, and
// how wide the native scheduler actually ran against how wide it was allowed.

const ratio = (value) => Number.isFinite(value) ? Number(value.toFixed(3)) : null;

export function summarizeThroughput(runStats, requestResponse) {
  const wallMs = runStats?.p4_latency_ms?.parallel_requests_ms;
  const seconds = Number.isFinite(wallMs) && wallMs > 0 ? wallMs / 1000 : null;
  const tokens = requestResponse?.generated_tokens ?? runStats?.generated_token_events ?? 0;
  const completed = runStats?.p4_latency_ms?.parallel_completed ?? requestResponse?.done ?? 0;
  const aggregate = seconds ? tokens / seconds : null;
  const admission = admissionState(runStats);
  const reference = Number.isFinite(runStats?.reference_tps) ? runStats.reference_tps : null;
  return {
    wall_ms: Number.isFinite(wallMs) ? Number(wallMs.toFixed(3)) : null,
    generated_tokens: tokens,
    aggregate_tps: ratio(aggregate),
    per_stream_tps: completed && aggregate !== null ? ratio(aggregate / completed) : null,
    reference_tps: reference,
    meets_reference: reference !== null && aggregate !== null ? aggregate >= reference : null,
    admission,
    ...verdict(admission)
  };
}

function admissionState(runStats) {
  const occupancy = runStats?.pipeline_occupancy ?? null;
  return {
    concurrent_requests: runStats?.concurrent_requests ?? null,
    agent_slot_width: runStats?.agent_slot_width ?? null,
    declared_max_sequences: runStats?.model_load_options?.batching?.max_sequences ?? null,
    native_peak: occupancy?.peak ?? null,
    native_limit: occupancy?.limit ?? null
  };
}

// Named so a reader can act on it without rereading the pipeline: each verdict
// points at the one tier that has to change.
function verdict(admission) {
  const { concurrent_requests: concurrency, native_peak: peak, agent_slot_width: slotWidth } = admission;
  if (peak === null) {
    return {
      verdict: 'unknown',
      detail: 'native occupancy is absent; run the stages with LINKER_PIPELINE_AUDIT=detail to record op=wavefront'
    };
  }
  if (!Number.isFinite(concurrency) || concurrency <= 1) {
    return { verdict: 'single-stream', detail: 'one arrival at a time; batching is not exercised' };
  }
  if (peak <= 1) {
    return {
      verdict: 'serialized',
      detail: slotWidth === 1
        ? 'the Agent NodeSlot holds one permit, so no two requests reach the adapter together; widen node_spec.p4_max_inflight'
        : `${concurrency} arrivals never overlapped in the native scheduler; check the adapter queue and the Agent slot before the native scheduler`
    };
  }
  if (peak < concurrency) {
    return { verdict: 'partial', detail: `native peak ${peak} of ${concurrency} arrivals` };
  }
  return { verdict: 'batched', detail: `native peak ${peak} covers ${concurrency} arrivals` };
}

export function throughputReport(throughput) {
  if (!throughput) return '';
  const { admission } = throughput;
  const rows = [
    ['Aggregate tokens/s', throughput.aggregate_tps ?? '-'],
    ['Per-stream tokens/s', throughput.per_stream_tps ?? '-'],
    ['Reference tokens/s', throughput.reference_tps ?? '-'],
    ['Meets reference', throughput.meets_reference === null ? '-' : throughput.meets_reference ? 'PASS' : 'FAIL'],
    ['Arrivals / Agent slot width', `${admission.concurrent_requests ?? '-'} / ${admission.agent_slot_width ?? '-'}`],
    ['Native peak / limit', `${admission.native_peak ?? '-'} / ${admission.native_limit ?? '-'}`],
    ['Verdict', `${throughput.verdict} — ${throughput.detail}`]
  ];
  return `## Throughput and admission\n\n| Field | Value |\n| --- | --- |\n${rows.map(([name, value]) => `| ${name} | ${value} |`).join('\n')}\n\n`;
}

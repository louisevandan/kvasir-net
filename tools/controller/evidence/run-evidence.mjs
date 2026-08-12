import { writeFile } from 'node:fs/promises';

export async function writeTrace(traceFile, traces) {
  if (!traceFile) return;
  await writeFile(traceFile, `${traces.map((trace) => JSON.stringify(trace)).join('\n')}\n`, 'utf8');
  const markdownFile = traceFile.replace(/\.jsonl$/i, '.md');
  const rows = traces.map((trace, index) => {
    const accepted = trace.responses.find((message) => message.type === 'INGRESS_ACCEPTED');
    const done = trace.responses.find((message) => message.type === 'DONE');
    return `| ${index} | ${accepted?.session_id ?? '-'} | ${escapeCell(trace.request.prompt)} | ${escapeCell(trace.final_text || '-')} | ${done?.reason ?? 'ERROR'} |`;
  });
  const name = traceFile.split(/[\\/]/).pop();
  await writeFile(markdownFile, `# P4 parallel request trace\n\nFull P4 message frames are in [${name}](${name}).\n\n| Session | Session ID | Input prompt | Final streamed text | Terminal |\n| ---: | --- | --- | --- | --- |\n${rows.join('\n')}\n`, 'utf8');
  console.log(`P4_TRACE jsonl=${traceFile} markdown=${markdownFile} requests=${traces.length}`);
}

export async function writePlan(planFile, requests) {
  if (!planFile) return;
  const evidence = { protocol: 'P4B1-v5', generated_at: new Date().toISOString(), requests };
  await writeFile(planFile, `${JSON.stringify(evidence, null, 2)}\n`, 'utf8');
  console.log(`P4_PLAN json=${planFile} requests=${requests.length}`);
}

export async function writeSummary(summaryFile, reportFile, runStats, traces) {
  if (!summaryFile || !traces.length) return;
  const timings = (kind) => traces.map((trace) => trace.responses.find((response) => response.type === kind)?.elapsed_ms).filter(Number.isFinite);
  const done = traces.map((trace) => trace.responses.find((response) => response.type === 'DONE'));
  const summary = {
    protocol: 'P4B1-v5', generated_at: new Date().toISOString(), run: runStats,
    request_response: {
      planned: traces.length,
      accepted: timings('INGRESS_ACCEPTED').length,
      first_token: timings('TOKEN').length,
      done: done.filter(Boolean).length,
      errors: traces.filter((trace) => trace.responses.some((response) => response.type === 'ERROR')).length,
      token_events: traces.reduce((total, trace) => total + trace.responses.filter((response) => response.type === 'TOKEN').length, 0),
      generated_tokens: done.reduce((total, response) => total + (response?.generated_tokens ?? 0), 0),
      finish_reasons: Object.fromEntries(done.filter(Boolean).reduce((counts, response) => counts.set(response.reason, (counts.get(response.reason) ?? 0) + 1), new Map())),
      latency_ms: { accepted: distribution(timings('INGRESS_ACCEPTED')), first_token: distribution(timings('TOKEN')), done: distribution(timings('DONE')) }
    }
  };
  await writeFile(summaryFile, `${JSON.stringify(summary, null, 2)}\n`, 'utf8');
  if (reportFile) await writeFile(reportFile, traceReport(summary, traces), 'utf8');
  console.log(`P4_SUMMARY json=${summaryFile}${reportFile ? ` markdown=${reportFile}` : ''} requests=${traces.length}`);
}

export function distribution(values) {
  if (!values.length) return null;
  const sorted = [...values].sort((left, right) => left - right);
  const percentile = (value) => sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * value) - 1))];
  return { min: sorted[0], mean: Number((sorted.reduce((total, value) => total + value, 0) / sorted.length).toFixed(3)), p50: percentile(0.5), p95: percentile(0.95), p99: percentile(0.99), max: sorted.at(-1) };
}

function traceReport(summary, traces) {
  const stats = summary.request_response;
  const rows = traces.map((trace) => {
    const accepted = trace.responses.find((response) => response.type === 'INGRESS_ACCEPTED');
    const firstToken = trace.responses.find((response) => response.type === 'TOKEN');
    const done = trace.responses.find((response) => response.type === 'DONE');
    return `| ${trace.request.index} | ${accepted?.session_id ?? '-'} | ${escapeCell(trace.request.prompt)} | ${escapeCell(trace.final_text || '-')} | ${done?.generated_tokens ?? 0} | ${done?.reason ?? 'ERROR'} | ${firstToken?.elapsed_ms ?? '-'} | ${done?.elapsed_ms ?? '-'} |`;
  });
  return `# P4 ${summary.run.parallel}-session request/response report\n\n## Run\n\n| Field | Value |\n| --- | --- |\n| Submitted / concurrent requests | ${summary.run.parallel} / ${summary.run.concurrent_requests} |\n| Native slots / execution window | ${summary.run.native_parallel} / ${summary.run.execution_window} |\n| Max output tokens | ${summary.run.requested_max_tokens} |\n| Context total / per request | ${summary.run.context_tokens} / ${summary.run.context_per_request_tokens} |\n| Planned / accepted / DONE / ERROR | ${stats.planned} / ${stats.accepted} / ${stats.done} / ${stats.errors} |\n| P4 text events / native generated tokens | ${stats.token_events} / ${stats.generated_tokens} |\n| Request wall-clock | ${summary.run.p4_latency_ms.parallel_requests_ms.toFixed(3)} ms |\n\n## Per-session latency (ms)\n\n| Event | min | mean | p50 | p95 | p99 | max |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: |\n${['accepted', 'first_token', 'done'].map((name) => { const value = stats.latency_ms[name]; return `| ${name} | ${value?.min ?? '-'} | ${value?.mean ?? '-'} | ${value?.p50 ?? '-'} | ${value?.p95 ?? '-'} | ${value?.p99 ?? '-'} | ${value?.max ?? '-'} |`; }).join('\n')}\n\n## All request/response pairs\n\n| # | Session ID | Input prompt | Final streamed text | Generated | Terminal | First token ms | DONE ms |\n| ---: | --- | --- | --- | ---: | --- | ---: | ---: |\n${rows.join('\n')}\n`;
}

function escapeCell(value) {
  return String(value).replaceAll('|', '\\|').replaceAll('\n', '<br>');
}

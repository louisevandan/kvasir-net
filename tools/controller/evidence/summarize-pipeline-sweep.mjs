import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

const [summaryFile, reportFile, ...inputFiles] = process.argv.slice(2);
// Sweep evidence contract: see ../../docs/runtime-evidence.md#2026-08-09-500-token-concurrency-sweep-100501021.
if (!summaryFile || !reportFile || !inputFiles.length) throw new Error('usage: summarize-pipeline-sweep.mjs <summary.json> <report.md> <run-summary.json>...');
const runs = await Promise.all(inputFiles.map(async (file) => ({ file, ...(JSON.parse(await readFile(file, 'utf8'))) })));
const rows = runs.map(({ file, run, request_response: result }) => ({
  parallel: run.parallel,
  concurrent_requests: run.concurrent_requests,
  max_tokens: run.requested_max_tokens,
  wall_clock_ms: run.p4_latency_ms.parallel_requests_ms,
  token_events: result.token_events,
  generated_tokens: result.generated_tokens,
  events_per_second: Number((result.token_events / (run.p4_latency_ms.parallel_requests_ms / 1000)).toFixed(3)),
  accepted_p95_ms: result.latency_ms.accepted?.p95,
  ttft_p50_ms: result.latency_ms.first_token?.p50,
  ttft_p95_ms: result.latency_ms.first_token?.p95,
  done_p50_ms: result.latency_ms.done?.p50,
  done_p95_ms: result.latency_ms.done?.p95,
  finish_reasons: result.finish_reasons,
  artifact: path.basename(file),
  plan: path.basename(file).replace(/^summary-/, 'plan-'),
  trace: path.basename(file).replace(/^summary-/, 'trace-').replace(/\.json$/, '.jsonl'),
  report: path.basename(file).replace(/^summary-/, 'report-').replace(/\.json$/, '.md')
})).sort((left, right) => right.concurrent_requests - left.concurrent_requests);
const suite = { protocol: 'P4B1-v6', generated_at: new Date().toISOString(), runs: rows };
await writeFile(summaryFile, `${JSON.stringify(suite, null, 2)}\n`, 'utf8');
const tableRows = rows.map((row) => `| ${row.concurrent_requests} | ${row.token_events} | ${row.generated_tokens} | ${row.events_per_second} | ${row.ttft_p50_ms} | ${row.ttft_p95_ms} | ${row.done_p50_ms} | ${row.done_p95_ms} | ${row.finish_reasons.stop ?? 0} / ${row.finish_reasons.length ?? 0} | [plan](${row.plan}) [trace](${row.trace}) [responses](${row.report}) [summary](${row.artifact}) |`);
await writeFile(reportFile, `# P4 concurrency sweep\n\nAll runs use the same model, generated request family, max output token cap, and per-request context. Each row links the exact plan, frame trace, full request/response report, and summary. The input set is a deterministic prefix of the same generated prompt family, so output length still varies by request; compare the trend as a measured operating signal, not a statistically repeated benchmark.\n\n| Concurrent sessions | P4 events | Generated tokens | Events/s | TTFT p50 ms | TTFT p95 ms | DONE p50 ms | DONE p95 ms | stop / length | Evidence |\n| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n${tableRows.join('\n')}\n`, 'utf8');
console.log(`P4_SWEEP summary=${summaryFile} report=${reportFile} runs=${rows.length}`);

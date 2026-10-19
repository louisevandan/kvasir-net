# Live ring measurement checkpoint

Base: ed09f366c8a60030f1458057a4b3798aa14c78ac.

Node status adds `localBenchmark` and `liveRing`; `measured` remains for older renderers. No stored data or existing field is renamed. No reward/performance report is changed.

The read-only adapter tries the configured staking origin's `/api/contributions`, without credentials or redirects. The observed public gateway returned HTTP 404 on 2026-10-19. No working public bridge feed has been established. The owned agent returned an empty stage list in a read-only INSPECT; therefore this machine has no observed pipeline stage. Expert worker presence/relay traffic is not proof of inference.

Current repository contribution records provide node_id, agent, perf_tps, rows and requests, but no model or last-work timestamp. This checkpoint deliberately displays unavailable for those records. Positive live-state tests use a proposed additive feed shape with `model` and `last_work_at_ms`; these are fixtures, not deployed fields. Exact stage and agent address matching is required. Relay-advertised versus local agent address mapping is unresolved and fails closed. No aggregate sums of pipeline rates are shown.

A future separate upstream change must define the model/last-work contract, a reachable wallet-readable route and authenticated agent address mapping. This checkpoint does not implement it. Bridge perf_tps is an EWMA of achieved request rates including prompt/request time, not decode-only throughput. For decode-only instrumentation, separately capture first/last output event times, output-token count, source/model/generation, observation window, and last actual work time. Do not infer these from gateway polling or relay bytes.

Fresh inspection expires after 15s and last-work metadata after 60s. Unknown metadata, duplicate matches, mismatched agent identity and stale work suppress numeric throughput. UI labels local benchmark separately; no ring load/restart/configuration changes.

## Proposal for Tony's decision — bridge/gateway follow-up

Proposal only; no upstream implementation, schema migration, route publication or deployment in this checkpoint.

1. **Additive contribution contract:** emit `model` (canonical catalog model ID) and `last_work_at_ms` (Unix milliseconds of actual completed work, never poll time) alongside existing fields. Bind each observation to stage/node ID, authenticated agent identity and model/load generation; define the relay-to-agent mapping server-side. Unknown metadata stays absent/null, never zero or inferred from traffic. Preserve existing consumers and reward accounting. On reload/restart, invalidate old-generation observations rather than carrying their rates forward.
2. **Read-only gateway route:** decide whether a minimal public projection of `GET /api/contributions` is acceptable or wallet authentication is required. The current desktop adapter sends no credentials, so an authenticated contract requires a separate client change. Gateway alone reads the private bridge using its service credentials; never send those credentials, private addresses or raw operational records to the wallet. Return only approved identity/model/timing fields plus observation time and generation; bounded responses, rate limits and explicit unavailable errors. No load, restart or placement side effects. Tony decides visibility and identity exposure before implementation.
3. **First/last-token timing:** instrument actual output-token events with a monotonic clock at a defined bridge/engine boundary. Report output-token count, first/last event elapsed times, model/generation, observation window and last-work wall time. For N >= 2 tokens, label `(N - 1) / (last - first)` as observed inter-token throughput, excluding startup, prompt processing and first-token latency. Keep first-token latency and whole-request achieved rate separate. Chunk arrivals are not token events; if the source cannot supply token-level timing, report unavailable rather than relabeling chunks or the existing EWMA as decode throughput. Never sum pipeline-stage rates.

Acceptance before rollout: contract/route tests for authentication policy and redaction, missing or duplicate identity, reload generations, stale timestamps, zero/one-token output and batched chunks; a controlled real-model comparison against engine timings; authenticated wallet UI checks. Tony chooses the route policy and approves a separate implementation/deployment plan.

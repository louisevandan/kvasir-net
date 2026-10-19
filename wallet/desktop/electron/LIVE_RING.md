# Live ring measurement checkpoint

Base: ed09f366c8a60030f1458057a4b3798aa14c78ac.

Node status adds `localBenchmark` and `liveRing`; `measured` remains for older renderers. No stored data or existing field is renamed. No reward/performance report is changed.

The read-only adapter tries the configured staking origin's `/api/contributions`, without credentials or redirects. The observed public gateway returned HTTP 404 on 2026-10-19. No working public bridge feed has been established. The owned agent returned an empty stage list in a read-only INSPECT; therefore this machine has no observed pipeline stage. Expert worker presence/relay traffic is not proof of inference.

Current repository contribution records provide node_id, agent, perf_tps, rows and requests, but no model or last-work timestamp. This checkpoint deliberately displays unavailable for those records. Positive live-state tests use a proposed additive feed shape with `model` and `last_work_at_ms`; these are fixtures, not deployed fields. Exact stage and agent address matching is required. Relay-advertised versus local agent address mapping is unresolved and fails closed. No aggregate sums of pipeline rates are shown.

A future separate upstream change must define the model/last-work contract, a reachable wallet-readable route and authenticated agent address mapping. This checkpoint does not implement it. Bridge perf_tps is an EWMA of achieved request rates including prompt/request time, not decode-only throughput. For decode-only instrumentation, separately capture first/last output event times, output-token count, source/model/generation, observation window, and last actual work time. Do not infer these from gateway polling or relay bytes.

Fresh inspection expires after 15s and last-work metadata after 60s. Unknown metadata, duplicate matches, mismatched agent identity and stale work suppress numeric throughput. UI labels local benchmark separately; no ring load/restart/configuration changes.

# Expert measurement design / where we are · next

Base: polish `536d22ecc14a4d0abd1c417e4bb7d80277f836a7` (on `897a0a9`).
Scope: wallet only; no gateway/bridge/native worker changes, release, login, or operational reconfiguration.

## Contract and implementation plan

Add per-slot expert measurements to node status; retain existing local GGUF decode fields unchanged. Metrics are expert FFN requests, not full-model generated tokens and not reward evidence. Count only a complete framed request followed by its complete response on the same relay connection. Decode legacy, v2 and v2f16 headers using the native worker framing. Observe without buffering hidden states or changing forwarded bytes; bound pending request metadata and fail closed on unknown framing. Drop partial observations at disconnect/reassignment. Rolling 60-second window: calls/s and input rows/s (rows are not expert pairs), nearest-rank p50/p95 wall latency from first request byte to final response byte, last work timestamp and model/layer/expert range. No traffic means idle, not inferred performance.

The Measure action prefers an existing owned serving slot: send small synthetic legacy requests with its n_embd and local expert IDs over loopback to the existing worker. One warmup + five timed requests, bounded timeout and concurrency, no additional process/VRAM allocation or network contribution report. Keep this synthetic result separate from real relay metrics. Preserve the local llama-server benchmark fallback only when a full-model GGUF exists. No cached-only worker launch in this increment: a non-serving cached shard lacks current ownership and would require an extra worker allocation; report this limitation explicitly. Existing worker `--bench` exists but uses unchecked compute returns and only averages, so the wallet uses the documented serving protocol instead.

## Read-only relay finding

`p4bridge/wsrelay.js:177-204` dials `target.host:target.port` from the bridge; its TCP error handler sends 1011 `upstream unavailable`. Wallet `expertHost.cjs:362-367` logs the received close and retries. `p4bridge/expert-dispatch-probe.py` describes the coordinator-listener direction and the no-listener case. A successful local worker start/coverage assignment does not establish an upstream dispatch listener. No deployed server logs inspected; actual ECONNREFUSED/timeout/address cause remains unverified.

## Visual layer card and scope decision

The PM follow-up accepts measuring the owned serving worker; cached-only worker launch is not required. With no serving worker and no full-model GGUF, the UI shows a clear en/ko unavailable-target message. `VERSION` remains 0.0.13 as requested.

`ExpertLayersCard` uses only assignment/GGUF metadata exposed by ExpertHost: model layer count, expert count and optional leading dense count. It does not guess missing topology from model names. Per-slot layer and expert-range highlights, file basename, port, state, window, live metrics and 12 five-second input-row buckets are rendered without new services or UI dependencies. At most 10,000 completed requests are retained; if that bound is reached the measured window shortens to the retained history. Real request latency includes local queue/transfer, not just GPU compute. The synthetic loopback run rejects non-finite outputs but is not a numerical correctness oracle.

Fixture screenshot: `/Volumes/Transcend/Projects/expert-layers-fixture.png` (synthetic fixture, not live measurements). Dense prefix is unknown when neither assignment nor shard metadata supplies it. No native worker or real wallet runtime benchmark has been executed for this change.

## Verification plan

Implement → self-review protocol bounds, connection association, ownership, no synthetic/live mixing → separate verification commands. Tests for fragmented/coalesced frames, v1/v2/f16, incomplete/invalid frames, idle expiry, benchmark framing, timeout and reassign behavior, UI i18n, selection/fallback. Run wallet tests, typecheck, build:web; report native-runtime and authenticated-UI limitations. PM receives SHA and evidence; no release.

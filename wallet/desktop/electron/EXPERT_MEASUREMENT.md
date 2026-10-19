# Expert measurement design / where we are · next

Base: polish `536d22ecc14a4d0abd1c417e4bb7d80277f836a7` (on `897a0a9`).
Scope: wallet only; no gateway/bridge/native worker changes, release, login, or operational reconfiguration.

## Contract and implementation plan

Add per-slot expert measurements to node status; retain existing local GGUF decode fields unchanged. Metrics are expert FFN requests, not full-model generated tokens and not reward evidence. Count only a complete framed request followed by its complete response on the same relay connection. Decode legacy, v2 and v2f16 headers using the native worker framing. Observe without buffering hidden states or changing forwarded bytes; bound pending request metadata and fail closed on unknown framing. Drop partial observations at disconnect/reassignment. Rolling 60-second window: calls/s and input rows/s (rows are not expert pairs), nearest-rank p50/p95 wall latency from first request byte to final response byte, last work timestamp and model/layer/expert range. No traffic means idle, not inferred performance.

The Measure action snapshots all existing owned serving slots and measures them sequentially: send small synthetic legacy requests with each slot's n_embd and local expert IDs over loopback to its existing worker. One warmup + 30 timed requests per slot, a 2-second total deadline per slot, pool/host concurrency guards, and no additional process/VRAM allocation or network contribution report. Any target's reassignment aborts the run, including while another target is being measured. A failed run has no node total; completed per-slot results remain distinguishable. Successful totals sum the sequential rows/s and take the worst p95; they are not measured concurrent capacity. Keep this synthetic result separate from real relay metrics. Preserve the local llama-server benchmark fallback only when a full-model GGUF exists. No cached-only worker launch in this increment: a non-serving cached shard lacks current ownership and would require an extra worker allocation; report this limitation explicitly. Existing worker `--bench` exists but uses unchecked compute returns and only averages, so the wallet uses the documented serving protocol instead.

## Read-only relay finding

`p4bridge/wsrelay.js:177-204` dials `target.host:target.port` from the bridge; its TCP error handler sends 1011 `upstream unavailable`. Wallet `expertHost.cjs:362-367` logs the received close and retries. `p4bridge/expert-dispatch-probe.py` describes the coordinator-listener direction and the no-listener case. A successful local worker start/coverage assignment does not establish an upstream dispatch listener. No deployed server logs inspected; actual ECONNREFUSED/timeout/address cause remains unverified.

## Original visual layer card and scope decision (superseded below)

The PM follow-up accepts measuring the owned serving worker; cached-only worker launch is not required. With no serving worker and no full-model GGUF, the UI shows a clear en/ko unavailable-target message. `VERSION` remains 0.0.13 as requested.

`ExpertLayersCard` uses only assignment/GGUF metadata exposed by ExpertHost: model layer count, expert count and optional leading dense count. It does not guess missing topology from model names. Per-slot layer and expert-range highlights, file basename, port, state, window, live metrics and 12 five-second input-row buckets are rendered without new services or UI dependencies. At most 10,000 completed requests are retained; if that bound is reached the measured window shortens to the retained history. Real request latency includes local queue/transfer, not just GPU compute. The synthetic loopback run rejects non-finite outputs but is not a numerical correctness oracle.

Fixture screenshot: `/Volumes/Transcend/Projects/expert-layers-fixture.png` (synthetic fixture, not live measurements). Dense prefix is unknown when neither assignment nor shard metadata supplies it. No native worker or real wallet runtime benchmark has been executed for this change.

## Node-card follow-up / where we are · next

Base: `d6e0269f3dc92ccdea9e0da90ddbedc0eca6f790`, branch `fix/wallet-node-cards`.

- Consolidated duplicate expert blocks into one node summary and a default-collapsed per-layer table. Phase distinguishes downloading, starting, serving/relay-down and failed. The table separates live rows/s from synthetic rows/s and synthetic p50/p95. File/port and window/sample metadata remain in tooltips; full topology grids and sparklines are no longer rendered.
- Live ring work only renders for `nodes.length > 0`. A null inspection timestamp is not inspected yet; only an aged previous inspection is stale. Failed fresh inspections suppress throughput without calling the snapshot old.
- A disconnected serving worker with close 1011 gets one bilingual note. Wording says the coordinator cannot be reached, rather than asserting no listener exists: the bridge code also uses 1011 for network failures. Old close codes do not override a connected relay or starting worker.
- Additive `expertWork.measurement` status and `node:benchmarkProgress` IPC contain progress/aggregate only. Progress polls every 100ms during Measure, without hardware/network probing, with one pending poll and cleanup on finish/unmount. Button is disabled with spinner and localized layer/count; errors render inline and controls re-enable.
- `VERSION` bumped from 0.0.13 to 0.0.14 per repository guidance for user-visible/runtime-affecting changes. No release or deployment.

### Read-only memory check (2026-10-19)

Ran `gpuReadiness()` from `electron/executors.cjs` on this Apple M4 Pro. Reported `totalBytes = 19,327,352,832` (18 GiB), which the current code computes as 75% of 24 GiB physical RAM. **This is an estimate, not a query of Metal's exact recommendedMaxWorkingSetSize.** With no configured override (absence reported by PM, not independently read), `vramBudgetBytes()` derives `9,663,676,416` (9 GiB). Configured Metal model: 9,499,993 resident bytes/expert, fixed 64 MiB and scratch 320 MiB per slot, headroom 256 MiB once.

- Seven × 64: 7,343,004,608 bytes (6.84 GiB).
- Eight × 64: 8,353,657,344 bytes (7.78 GiB), leaving 1,310,019,072 bytes (1.22 GiB) under the default budget.
- `capacityForBudget(defaultBudget, model, null)` directly returned 512 experts / 8 slots. Therefore no configured-budget overrun is established. This does not prove sufficient free RAM under current system pressure. Allocation policy unchanged; UI flags excess held count or modeled bytes.

`ps -axo pid,rss,comm` reproduced three ~10 MiB RSS worker processes. Read-only `vmmap -summary` showed their physical footprints at 608.3 / 607.7 / 607.4 MiB, with writable swapped-out values 605.7 / 604.7 / 604.5 MiB and ~580.2 MiB shared memory swapped out each. A newer worker had 606.1 MiB footprint and 581.2 MiB resident writable memory. Thus low RSS does **not** show released/lost buffers: allocations remain accounted for but are paged/compressed out. This is OS residency behavior and a potential cold-request latency concern, not proof of healthy real serving. No native workload, buffer-content read, allocation change or wallet login was performed.

## Verification plan

Implement → self-review protocol bounds, connection association, ownership, no synthetic/live mixing → separate verification commands. Tests for fragmented/coalesced frames, v1/v2/f16, incomplete/invalid frames, idle expiry, benchmark framing, timeout and reassign behavior, UI i18n, selection/fallback. Run wallet tests, typecheck, build:web; report native-runtime and authenticated-UI limitations. PM receives SHA and evidence; no release.

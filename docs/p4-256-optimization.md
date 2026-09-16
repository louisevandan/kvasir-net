# P4 256-session communication and inference optimization: measurement report

> Document status (2026-09-06): **historical / old plan**. It preserves the plans and observations of that time. Do not use it for current status, execution order or promotion criteria.
> For current goals, status and order see the [execution roadmap](distributed-batching-roadmap.md); for document authority and reading paths see the [document map](document-map.md).

Reference date: 2026-08-10. This document reconstructs the local 2-GPU Pipeline execution of P4 (Proxy Pipeline Parallel Protocol)
from real request/response traces, stage instrumentation and raw GPU samples.
It does not claim success from configuration, login or model load alone.

## Conclusion

With a 14:14 layer split, a fixed backend sampler and a shared-memory boundary, **4 independent
controllers/Pipelines each handled 64 concurrent sessions, and all 256/256 requests succeeded**.
After that, reverse 14:14, which moved the terminal role to the RTX 4080, also passed 256/256 and
lowered terminal compute and wall time further. A single native context with
`parallel=batch=ubatch=256` also passed: 256/256 complete, 0 errors, 0 violations of the actual generated-token cap
and 0 TCP fallbacks.

To keep natural EOG termination from skewing comparisons, we added the benchmark-only `benchmark_ignore_eog`.
In this mode, reverse 14:14 with **4 × 64 physical microbatches** processed exactly
500 completion tokens per request, 128,000 tokens in total, in 37.985 s, recording **3,369.71 tok/s**.
The reverse 15:13 rebalance also passed with 256/256, 128,000 tokens and 0 TCP fallbacks,
at 37.827 s and **3,383.82 tok/s**, a difference of only 0.4%. Moving one layer
is not a real fix for the current bottleneck.

Shared-memory transfer and terminal return are small, and TCP batch fallback stays at 0. With 4 × 64
at fixed length, the 4080 terminal GPU's mean active rate is about 80–82%, while the 3090 first stage
reaches only about 42–44%. Stage 0's downstream wait is on the same scale as stage 1 compute.
Re-running the same 256 sessions under Nsight Systems showed a CUDA graph execution
union of 70.55% on the terminal 4080, a p95 graph gap of 17.127 ms, and a mean kernel queue time of
12.962 ms on stage 1. **P4 relay is not the bottleneck, but the terminal CUDA submit/synchronize path still
has idle intervals.** This measurement alone cannot reveal tensor-core instruction occupancy, so
the next verdict is limited to Nsight Compute counters on the terminal stage.

## Scope and verdict rules

| Item | Fixed condition |
| --- | --- |
| Model | Qwen2.5-1.5B-Instruct-Q8_0, local CUDA 2-stage Pipeline |
| Request | Korean-language request to explain Rust, generation cap of 500 per request |
| sampler | terminal backend: `temperature=0.2`, `top_p=0.9`, `top_k=20`, `seed=7` |
| Sessions | all three physical microbatch shapes measured: 4 × 64, 2 × 128, 1 × 256 |
| Boundary | CUDA P2P/IPC unavailable (`can_access:false` in both directions), so host shared memory is used |
| Valid trace | prompt and final sentence non-empty, `max_tokens=500`, accepted exactly 1, DONE exactly 1, `completed=true` |

4 × 64 and 2 × 128 are real 256-concurrent-session system tests on the same physical GPUs,
but each lane is a separate `llama_context`. 1 × 256 verifies 256 sequences in a single context.
So interpret the context graph limit and the scheduler's actual batch width separately.

## Experiment history so far

| Status | Run/change | Result | Interpretation |
| --- | --- | --- | --- |
| Pass | TCP hidden-state boundary | 256/128, 54,550 events, 96.749 s | initial communication baseline |
| Pass | shared-memory boundary | 256/128, 54,956 events, 80.402 s | host shared memory reduced boundary cost |
| Pass | terminal batch return + persistent CPU sampler | 256/256, 56,357 events, 58.621 s | reusing sampler workers/buffers reduced CPU sampler cost |
| Pass | CPU sampler, 4 × 64, 11:17 | 256/256, 14,189 events, slowest 8.793 s | short 64-lane baseline |
| Pass | backend sampler, 4 × 64, 11:17 | 256/256, 14,231 events, slowest 19.950 s | terminal 3090 mean 89.73%, first 4080 mean 28.79% |
| Rejected | backend sampler, physical 256/256 | context graph short by 368 bytes | static sampler graph budget was missing |
| Pass | new graph budget, 15:13, 64 | 64/64, 3,612 token events, 15.239 s | reservation formula and backend sampler verified through actual generation |
| Pass | new graph budget, 12:16/13:15/14:14/15:13 | 64/64 each, 0 errors | 14:14 chosen as the 256 candidate |
| Pass | new graph budget, 14:14, 4 × 64 | **256/256, 14,082 token events, wall 18.011 s** | current reference result |
| Rejected | 15:13, 4 × 64, 4080 first / 3090 terminal | 256/256 but wall 26.029 s, terminal compute 64.495 s | moving one layer made this layout worse |
| Pass | reverse 14:14, 4 × 64 | **256/256, 14,151 token events, wall 16.527 s** | 3090 first / 4080 terminal, current best |

Because generation length differs per request, the layer cut was not chosen from wall time alone.
Under identical conditions we looked at stage compute/token, downstream wait and trace completeness together.

## Static sampler graph failure and fix

At 15:13 with 64 sessions, the initial reservation formula counted only the tensors the model stage actually holds,
and did not count enough of the context metadata the full graph needs when it builds the sampler chain.
The failure occurred not in the Pipeline cut-set but in the context
metadata pool before or during upstream `build_sampling()`.

| run | Attempt | Observation |
| --- | --- | --- |
| `backend-sampler-15-64-v4` | stage metadata condition | needed 1,273,296; available 1,272,928 |
| `...-v5` | estimate of 8 per sampler | needed 738,080; available 737,712 |
| `...-v6`~`v9` | Pipeline reserve 16→17→32, debug marker | 368-byte gap every time; failed before the marker |
| `...-v10` | 2× stage base + 8/sequence | metadata pool exhausted at the 47th sampler chain |

Instrumentation in v10 confirmed that the first sampler chain adds 37 graph nodes, each later chain adds 36 graph nodes,
and each chain needs 37 metadata tensor objects. Because the same `res`
value drives both graph capacity and the tensor metadata pool, matching only the graph node count
can fail again.

The current compat layer sets the stage-aware base to 2× and reserves **64 + 1 safety
object** for each active sampler. 64 is a conservative upper bound with headroom over the observed 37 objects,
and it is a small increase in host metadata, not in VRAM or transfer volume. The change exists only in a version-pinned compatibility patch
outside the official llama.cpp checkout.

| Verification | Result |
| --- | --- |
| compat patch set | `b7f842e37ca8642bbaab86ebfe208cdda19642ed586abda7af899675e6dddc21` |
| upstream preparation | `prepare-pipeline-upstream.mjs --json` passed |
| native pipeline stability | CTest `pipeline-stability` 5/5 passed after the CUDA build |
| generation verification | `backend-sampler-15-64-v11`: 64/64 trace-valid, 0 errors |

The related sources are [`0004-llama-context.patch`](../native/compat/3e3a7a416/0004-llama-context.patch) and
[`0006-llama-graph.patch`](../native/compat/3e3a7a416/0006-llama-graph.patch).
No Linker/P4 changes were put into `apps/p4/layers/adapters/llamacpp/upstream`.

## 64-session layer-cut comparison

The GPU means below are secondary indicators because output length differs per run. The main basis for the choice is
the compute/token and completion latency of the two stages.

| Split | Generated token events | Completion window | 4080 compute/token | 3090 compute/token | first downstream wait | Completion p95 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 12:16 | 3,624 | 14.304 s | 1.623 ms | 1.997 ms | 7.454 s | 13.247 s |
| 13:15 | 3,660 | 15.501 s | 1.865 ms | 2.022 ms | 7.739 s | 14.733 s |
| **14:14** | **3,386** | **10.023 s** | **0.924 ms** | **1.723 ms** | **5.977 s** | **9.344 s** |
| 15:13 | 3,612 | 15.239 s | 2.005 ms | 1.874 ms | 7.068 s | 14.285 s |

In this comparison, 14:14 had the lowest maximum compute/token across the two stages and the lowest completion latency.
However, neither GPU was saturated with 64 sessions alone, so the choice had to be re-confirmed with the 256-concurrent
test.

## Final 256-concurrent-session measurement

The run started four lanes at the same moment, each using distinct P4 listen, Pipeline listen and native stage ports.
The outer PowerShell job reported an abnormal exit code at the end,
but that is the exit status of the job wrapper. The P4 run itself recorded
`P4_PIPELINE_E2E_CLIENT_PASS` on all four lanes, and the independent artifact verification below takes precedence over the wrapper status.

| lane | Completed/errors/integrity violations | token events | Request window | Completion p95 | 4080 compute | 3090 compute |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| lane1 | 64 / 0 / 0 | 3,596 | 17.711 s | 15.581 s | 2.543 s | 13.214 s |
| lane2 | 64 / 0 / 0 | 3,372 | 17.693 s | 15.388 s | 2.515 s | 13.313 s |
| lane3 | 64 / 0 / 0 | 3,472 | 18.011 s | 15.035 s | 2.463 s | 13.348 s |
| lane4 | 64 / 0 / 0 | 3,642 | 16.973 s | 15.877 s | 2.341 s | 12.688 s |
| **Total/wall clock** | **256 / 0 / 0** | **14,082** | **18.011 s** | overall p95 15.667 s | 9.862 s sum | 52.563 s sum |

Across all traces, accepted mean/p95 is 41/51 ms, first token 5.103/5.446 s,
and completion 10.606/15.667 s (max 17.966 s). The final prompt trace preserves
“Rust 언어를 한국어로 간단히 설명해. 핵심 특징을 한 문장으로 포함해.” (English: "Briefly explain the Rust language in Korean. Include its key features in one sentence.").
The final sentence of each lane's first response is also kept as a real inference result.

### Communication and GPU evidence

| Metric | 4080 first stage | 3090 terminal stage | Meaning |
| --- | ---: | ---: | --- |
| GPU mean utilization | 27.809–28.078% | 86.297–87.044% | only the terminal is almost continuously active |
| GPU p95 utilization | 47% | 99–100% | the first stage has large headroom |
| first downstream wait sum | 56.517 s | — | matches waiting for terminal results |
| stage compute sum | 9.862 s | 52.563 s | terminal decode is about 5.3× larger |
| TCP batch fallback | 0 | — | no TCP queue at the shared-memory/batched boundary |
| hidden transfer rate | about 46–120 MB/s (per-lane samples) | — | payload delivery does not dominate, even next to the small compute |

Terminal return sends take tens of ms per lane, and sampler apply is also in the tens of ms. By contrast,
the 3090's `decode_submit_us` is about 11.7–13.0 s per lane. So at this point there is no basis for changing the socket,
event loop or shared-memory structure again. The communication layer relayed 256
concurrent sessions without loss; the next bottlenecks are model placement and terminal decode.

## Reverse rank 14:14: moving the terminal to the RTX 4080

The existing planner chose the 4080 as the first rank and the 3090 as the terminal rank. The P4 plan already carries
per-stage `node`/`node_id`, so we added `P4_PIPELINE_STAGE_NODE_ORDER` to the benchmark runner
without changing the wire. It is an experimental input that records an explicit placement
in the plan,
and native launch uses that node mapping unchanged through the existing `ringProcessLaunches()`.

A preliminary 64-session run (`backend-sampler-reverse-14-64-v1`) first verified the mapping and the generation path with
`stage_nodes=[p4-gpu-3090,p4-gpu-4080]`, 64/64 complete, 0 errors and 0 TCP fallbacks.
We then ran 4 × 64 concurrently with the same mapping.

| Comparison: 14:14, 4 × 64 | Default rank (4080 first → 3090 terminal) | Reverse rank (3090 first → 4080 terminal) | Change |
| --- | ---: | ---: | ---: |
| valid completions / errors / integrity violations | 256 / 0 / 0 | 256 / 0 / 0 | same |
| generated token events | 14,082 | 14,151 | similar length |
| wall time | 18.011 s | 16.527 s | -8.2% |
| first stage compute sum | 9.862 s | 10.501 s | +6.5% |
| terminal stage compute sum | 52.563 s | 40.972 s | -22.1% |
| first downstream wait sum | 56.517 s | 43.228 s | -23.5% |
| terminal GPU mean / p95 | 86.3–87.0% / 99–100% | 73.2–78.7% / 94% | terminal dominance eased |
| first GPU mean / p95 | 27.8–28.1% / 47% | 30.1–30.7% / 47–63% | still large headroom |
| TCP batch fallback | 0 | 0 | communication path unchanged |

Reverse rank cut the stage compute ratio from about 5.3:1 to about 3.9:1, but the stages are still not balanced.
The comparison that moved 15:13 onto the default rank passed 256/256, but both token-normalized
terminal cost and wall time got worse, so it is not adopted. The next comparison checks 15:13
**on the reverse rank only**, to decide whether giving the 3090 first stage more work actually lowers the 4080 terminal
load further.

### Reverse 15:13 rejected, and the next hypothesis

Reverse 15:13 also passed both the preliminary 64-session run and the 4 × 64 256-session run as trace-valid.
But at 256 sessions it recorded 14,709 token events, wall 21.647 s, first/terminal compute sums of
13.074/52.058 s and a first downstream wait of 55.410 s. All of these are worse than reverse 14:14's
10.501/40.972 s and 43.228 s, so this cut is rejected.

The cause is not communication but the 4 × 64 execution shape. The scheduler's physical microbatch is
`min(batch, ubatch)`, and each independent native context has only 64 active sessions, so
with `batch=64`, `ubatch=64` any single batch sent to the GPU is at most 64. Adding controllers
does not merge batches across different native contexts. The next minimal
experiment is therefore **reverse 14:14, one 128-session lane**. If it passes, build 256 concurrent sessions from
2 × 128 lanes. The aim is to grow the physical microbatch to 128 without changing the communication structure,
improving CUDA kernel shapes and the idle time of both stages.

### 2 × 128 physical microbatch: pass and re-measurement

A single 128-session lane (`backend-sampler-reverse-14-128-v1`) was verified first with 128/128 trace-valid,
`max_batch_size=128` and 0 TCP fallbacks. The first attempt to bring up both lanes at once
failed before inference, because 539xx/540xx fell inside the Windows TCP excluded range
53851–54550. The preserved native stderr showed exactly
`cannot listen on <port>`; it was not a VRAM, graph or transport failure. We deleted that
failed group and took only the retry on 531xx, outside the excluded range, as a performance result.

The first `r2` result was taken while the P4 adapter still wrote the number of text chunks into `DONE.generated_tokens`.
To pin down the meaning of the compared value, we fixed the adapter and re-measured the same conditions as
`v4`. In `v4`, the completion token count is `completion_tokens` from the last native SSE usage,
with a fallback to the text chunk count only when usage is absent.

| Comparison: reverse 14:14, 256 sessions | 4 × 64 (batch 64) | 2 × 128 (batch 128, v4) | Change |
| --- | ---: | ---: | ---: |
| valid completions / errors / integrity violations | 256 / 0 / 0 | 256 / 0 / 0 | same |
| completion tokens / text chunks | old instrumentation | 34,693 / 34,016 | chunks and native tokens told apart |
| completion window | 16.527 s | 40.041 s | output lengths differ, so raw wall is not compared on its own |
| completion tokens/s | old instrumentation | **866.43** | baseline under the new semantics |
| 3090 first compute/token | 0.742 ms | 0.535 ms | microbatch growth benefit holds |
| 4080 terminal compute/token | 2.895 ms | 1.572 ms | microbatch growth benefit holds |
| first downstream wait/token | 3.055 ms | 1.612 ms | reduced with no communication fallback |
| native observed maximum batch | 64 | 128 | grew as intended |
| TCP batch fallback | 0 | 0 | communication path unchanged |

This is direct evidence that a 128 physical microbatch actually raised kernel/dispatch
efficiency. The two contexts still share the same GPUs, and the 4080 terminal is relatively
heavier. The actual upper limit of native `n_seq_max` is 256, so the next step verified a single 256
context.

### 1 × 256 physical microbatch: capacity passes, throughput drops

`backend-sampler-reverse-14-256-1x256-v2` ran reverse 14:14 with
`parallel=concurrent=batch=ubatch=256`. Strict trace verification after the adapter fix shows
submitted/accepted/done all at 256, 0 errors and 0 integrity violations.
235 requests ended with `stop` and 21 with `length`, and the maximum native completion token count is exactly
500.

| Comparison: exact P4 completion-token instrumentation | 2 × 128 (v4) | 1 × 256 (v2) | Verdict |
| --- | ---: | ---: | --- |
| completed / errors / integrity violations | 256 / 0 / 0 | 256 / 0 / 0 | both valid |
| completion tokens / text chunks | 34,693 / 34,016 | 56,700 / 55,654 | chunk count need not equal token count |
| max completion tokens | 500 | 500 | request cap respected |
| completion window | 40.041 s | 107.261 s | natural-termination distributions differ |
| completion tokens/s | **866.43** | 528.62 | 2 × 128 is 63.9% higher |
| 3090 first compute/token | **0.535 ms** | 0.614 ms | 2 × 128 ahead |
| 4080 terminal compute/token | 1.572 ms | **1.161 ms** | terminal kernels are efficient with one large batch |
| 3090 first downstream wait/token | 1.612 ms | 1.164 ms | terminal wait itself decreased |
| observed maximum batch | 128 | 256 | proves 256 native graph/sequence capacity |
| TCP batch fallback | 0 | 0 | transport is non-dominant in both cases |

With 1 × 256, the 250 ms GPU-util means of the two GPUs were 26.53% on the 3090 and 26.99% on the 4080
(p95 74% and 55%). This does not mean "256 could not be batched". The native stage aggregates
each record 70,728 batched tokens and 256 completed sessions. The cause is that after 235 requests ended with EOG,
the active batch width kept shrinking inside the same single lane. Under production semantics, where termination length
is not fixed, mean GPU-util and overall throughput must not be read directly as a
tensor-core limit.

### Correction to P4 completion-token semantics

A `TOKEN` frame is a **text chunk** emitted by the UTF-8 text filter. One native token
can become several chunks, and several native tokens can merge into one chunk, so
the `TOKEN` frame count is not an authoritative generated-token count. The Rust P4 adapter now sends
`completion_tokens` from the last SSE usage as `DONE.generated_tokens`. For backends
without usage it safely falls back to the chunk count. The verification invariant is `DONE.generated_tokens <=
INGRESS_SUBMIT.max_tokens`; equality with the chunk count is not required.

## Fixed-length 500-token benchmark: re-measurement without the natural-termination variable

To build comparable physical batches without changing EOG termination for production requests,
only benchmarks pass `--benchmark-ignore-eog` to the first stage. Even if the terminal sees a sampled
EOG, the first stage ends the session only at `max_tokens`, not at EOG. P4
rejects this option without `benchmark=true`, and a 64-token regression in normal mode
completed with `stop` at 37 tokens because of EOG, confirming that the existing semantics are preserved.

In the first fixed-length 256 run, the launch log showed that the native flag was missing because the supervisor bundle was older
than the domain build. We therefore discarded the result, ran `npm run build:server --workspace
llama`, confirmed the flag was in the bundle, and re-ran. This is a case of artifact-identity verification failure,
not part of the performance comparison.

| Configuration: reverse 14:14, 500 native completion tokens per request | Completed/errors | Total tokens | Slowest request window | Throughput | 3090 compute/token | 4080 compute/token | 3090 downstream wait/token | TCP fallback |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 × 256, batch=ubatch=256 | 256 / 0 | 128,000 | 170.650 s | 750.07 tok/s | 0.583 ms | 0.661 ms | 0.662 ms | 0 |
| 2 × 128, batch=ubatch=128 | 256 / 0 | 128,000 | 78.005 s | 1,640.92 tok/s | 0.420 ms | 0.711 ms | 0.714 ms | 0 |
| **4 × 64, batch=ubatch=64** | **256 / 0** | **128,000** | **37.985 s** | **3,369.71 tok/s** | **0.256 ms** | **0.806 ms** | **0.834 ms** | **0** |

The fixed-length 1 × 256 run showed that stage execution and hand-off inside one context grow long serially;
it is not a natural-drain effect. A separate attempt to split a single 256 context into two
windows with `ubatch=128` was stopped by an upstream GGML assertion before execution.
With `n_tokens=128`, `n_seqs=256`, the runtime rounded up to 256 and then went past the view range.
This is not a P4 transport result. Without changing the upstream/compat boundary right away, it is recorded as an unsupported
batch shape.

### Negative result of the 15:13 rebalance

In the 14:14 4 × 64 result, the terminal 4080 had high GPU activity and the first 3090 had
headroom, so we compared only 15:13, which gives the first stage one more layer while wire, sampler, batch, prompt and max tokens
stay unchanged. The first attempt was excluded from performance results because a mismatch between load profile
`0.8/0.95/40/123` and request profile `0.2/0.9/20/7` made native exit defensively.
Only `v3`, re-run with the same profile, is valid.

| Configuration: 4 × 64, 256 sessions, 500 per request | 14:14 | 15:13 | Change |
| --- | ---: | ---: | ---: |
| completed / errors / token-cap violations | 256 / 0 / 0 | 256 / 0 / 0 | same |
| total native completion tokens | 128,000 | 128,000 | same |
| slowest request window | 37.985 s | 37.827 s | -0.4% |
| throughput | 3,369.71 tok/s | 3,383.82 tok/s | +0.4% |
| 3090 compute/token | 0.256 ms | 0.277 ms | worse |
| 4080 compute/token | 0.806 ms | 0.779 ms | slightly better |
| 3090 downstream wait/token | 0.834 ms | 0.809 ms | slightly better |
| 3090 / 4080 GPU mean active rate | about 42% / 82% | 43.882% / 79.747% | imbalance remains |
| shared-memory frames (per lane) | 560/560 | 559/559 | normal |
| TCP batch fallback | 0 | 0 | normal |

Even after moving one layer from the 4080 to the 3090, the final stage still takes longer, and the change in overall
throughput is within run-to-run noise. The next change target is not the layer cut but **how independent contexts on the same GPU
submit to the CUDA stream concurrently**, together with the
physical scheduling of terminal decode.

## Nsight Systems: CUDA execution and queue verdict for 256 sessions

The next capture used the same reverse 14:14, 4 × 64 and exactly 500 native
completion tokens per request as the baseline above. The native stage's binary stdout is the P4 control stream, so
we did not wrap individual `linker-node` processes in the profiler. Instead we started the parent Node supervisor under Nsight
Systems child-process tracing, which preserved P4 pipe semantics. During the capture
all four lanes still recorded `P4_SHARED_MEMORY_PASS` (559/559 frames each),
`P4_PIPELINE_E2E_CLIENT_PASS`, 256/256 complete, 0 errors and 0 TCP fallbacks.

| Item | Nsight capture result | Relation to the baseline |
| --- | ---: | --- |
| valid completions / errors / total completion tokens | 256 / 0 / 128,000 | fixed-length invariant holds |
| wall time / throughput | 39.606 s / 3,231.87 tok/s | 4.1% below the 3,369.71 tok/s baseline because of profiler overhead; not used as a performance comparison value |
| 3090 stage 0 compute / downstream wait sum | 35.334 s / 111.930 s | downstream wait is 3.17× stage 0 compute |
| 4080 terminal stage 1 compute sum | 109.399 s | dominant work of the four terminal contexts |
| CUDA native processes | 8 (3090: 4, 4080: 4) | both stages of all four lanes were captured |

The `nvidia-smi` 250 ms samples averaged 41.93%/79.94% on the 3090/4080. In the finer-grained
Nsight CUDA graph timeline, the execution intervals of the four contexts on the same GPU must be merged as a union
to see physical GPU gaps. Merging only generic kernel events counts the kernels inside a CUDA graph
as separate short events and underestimates actual graph execution time, so the verdict below
uses the union of `CUPTI_ACTIVITY_KIND_GRAPH_TRACE`.

| Physical GPU / P4 role | graph execution union / observed span | Occupancy | graph gap p50 / p95 / p99 / max | Interpretation |
| --- | ---: | ---: | ---: | --- |
| RTX 3090, first stage | 11.582 / 37.813 s | 30.63% | 12.218 / 50.499 / 91.947 / 143.792 ms | waiting on downstream/terminal shows up as large headroom |
| RTX 4080, terminal stage | 26.716 / 37.867 s | 70.55% | 1.175 / 17.127 / 36.341 / 500.699 ms | dominant stage, but about 29.45% graph-level gap remains |

CUDA runtime calls also concentrate on the terminal. The 4080 terminal recorded `cudaStreamSynchronize`
449,804 calls/51.719 s, `cudaMemcpyAsync` 705,319 calls/15.524 s,
`cudaLaunchKernel` 1,505,700 calls/15.398 s and `cudaGraphLaunch` 6,027 calls/9.978 s.
The same values on the 3090 first stage are 38,008 calls/11.941 s, 20,956 calls/0.796 s,
647,472 calls/5.055 s and 1,660 calls/0.772 s. The mean launch queue in the execution summary is also
**12.962 ms** on the 4080 versus 2.470 ms on the 3090. The 4080 is already receiving a submit backlog from multiple contexts,
yet its graph timeline is not free of gaps.

| GPU / role | CUDA launches | Launches with a queue | Mean queue time | Total kernel time | Top 2 kernels by time |
| --- | ---: | ---: | ---: | ---: | --- |
| RTX 3090, first | 328,328 | 326,364 | 2.470 ms | 4.676 s | `mul_mat_q` 1.897 s, `flash_attn_ext_f16` 0.995 s |
| RTX 4080, terminal | 794,490 | 793,956 | **12.962 ms** | 5.067 s | `mul_mat_q` 2.216 s, `flash_attn_ext_f16` 0.765 s |

This establishes the following facts.

1. P4 transport runs at 256 sessions with no loss and no fallback path, so it is not the next optimization target.
2. The terminal 4080 carries CUDA graph scheduling and synchronization load that
   moving the layer cut by one layer did not relieve. The 3090's low utilization is consistent with
   waiting for terminal completion.
3. However, 70.55% graph occupancy and a 12.962 ms queue are not measurements of SM occupancy, tensor-core active
   cycles or memory bandwidth. They cannot be overstated as tensor-core saturation,
   and changing the kernels themselves without hardware counters also lacks grounds.

## Preserved artifacts

All raw results are kept in `apps/p4/target/pipeline-e2e/`.

- `trace-backend-sampler-14-256-lane{1..4}.jsonl` — all 256 request/response pairs
- `trace-...md`, `report-...md` — human-readable request, final sentence and session reports
- `summary-...json` — stage/wire/latency aggregates
- `summary-...-gpu.jsonl` — 250 ms raw GPU samples
- `plan-...json`, `client-...log`, `p4-agent-...log`, `p4-pipeline-...log` — plans and run logs
- `trace-backend-sampler-reverse-14-256-2x128-v4-lane{1,2}.jsonl` — 256 request/response pairs re-measured under the new
  completion-token semantics
- `summary-backend-sampler-reverse-14-256-2x128-v4-lane{1,2}.json` — native stage aggregates, shared-memory transfer and sampler
  statistics for the same run
- `trace-backend-sampler-reverse-14-256-1x256-v2.jsonl` and
  `summary-backend-sampler-reverse-14-256-1x256-v2.json` — 256 sequences in a single native context,
  batch=256 capacity and strict token-cap verification
- `trace-fixed-length-reverse-14-256-4x64-v1-lane{1..4}.jsonl` and
  `summary-fixed-length-reverse-14-256-4x64-v1-lane{1..4}.json` — current benchmark baseline with EOG suppressed and
  exactly 128,000 native completion tokens
- `trace-fixed-length-reverse-15-256-4x64-v3-lane{1..4}.jsonl` and
  `summary-fixed-length-reverse-15-256-4x64-v3-lane{1..4}.json` — negative 15:13 rebalance comparison on the same
  workload; includes prompts, DONE and raw stage/wire/GPU instrumentation
- `summary-fixed-length-*-gpu.jsonl` — raw GPU utilization, VRAM and power samples at 250ms intervals.
  Every valid fixed-length run keeps both `P4_SHARED_MEMORY_PASS` and `P4_PIPELINE_E2E_CLIENT_PASS`
  in its client log.
- `.cache/p4-nsys-256/p4-fixed-reverse-14-256-4x64-v1.nsys-rep` — Nsight Systems original of the parent supervisor and
  the 8 native CUDA processes under it (153.6 MB)
- `.cache/p4-nsys-256/p4-fixed-reverse-14-256-4x64-v1.sqlite` and
  `analysis_cuda_{kern_exec,gpu_kern,api}_sum.csv` — reproducible extracts used for this document's graph union, runtime
  API and launch queue calculations
- `trace-nsys-fixed-reverse-14-256-4x64-v1-lane{1..4}.jsonl` and
  `summary-nsys-fixed-reverse-14-256-4x64-v1-lane{1..4}.json` — the 256 request/response pairs during the profiler capture
  and the P4 stage/wire aggregates

## 2026-08-10: chosen P4 improvement

The 256-session traces and Nsight results showed that shared-memory P4 transport works without request loss
and with 0 TCP fallbacks. Terminal CUDA execution does have remaining idle intervals, but
llama.cpp compatibility changes that assume a CUDA-only backend have not yet been verified as a performance gain on the same fixture.
So the only structural change adopted at this stage is the **ingress execution-credit** in
[`AgentProcessor`](../../p4/runtime/src/agent.rs).

The Agent now sends `INGRESS_ACCEPTED` only after it has first secured a ready binding and a NodeSlot permit.
A saturated request ends with `ERROR` without an accept, so an external
controller can read acceptance as an execution slot actually being secured. This policy uses only the opaque
`p4_max_inflight` and does not interpret CUDA, the llama.cpp private ABI, Pipeline hidden state or sampling
options. The mock adapter TCP test verifies that ready credit yields only
`INGRESS_ACCEPTED → DONE` and saturated credit yields only `ERROR`.

The earlier CUDA/llama.cpp boundary-copy change candidates remain adapter-private experiments; they are
neither an acceptance basis for this P4 improvement nor a protocol requirement.

## Proposed next stage

1. **Keep 4 × 64 as the fixed benchmark baseline.** It is currently the only configuration that ends all 256
   requests at exactly 500 native tokens and shows 3.37k tok/s.
   `benchmark_ignore_eog` is not a production default; runs that allow natural EOG stay a separate
   quality/real-use metric.
2. **The next minimal measurement is Nsight Compute on the terminal RTX 4080.** Limited to `mul_mat_q` and
   `flash_attn_ext_f16`, collect SM active, tensor-pipe active, achieved occupancy and
   DRAM throughput. The capture must again meet 256/256, 128,000 tokens, shared-memory
   pass and 0 TCP fallbacks, and profiler throughput is never mixed with the baseline.
3. **Narrow the change target to a single place based on the counter verdict.** If tensor/SM are high and DRAM is also
   saturated, stop touching P4 and the scheduler; the next options are model quantization/placement or a larger GPU.
   If tensor/SM are low, or graph gaps and launch stalls persist, reduce the native
   scheduler's terminal return wakeups, cross-context CUDA graph submits and synchronization frequency
   one change at a time. Even then, do not change the P4 wire or the shared-memory ABI.
4. **Re-measure a scheduler improvement with only 1 run each, before and after, against the baseline.** The acceptance criteria are 256/256 exact
   500-token completion, preserved shared-memory ordering, 0 TCP fallbacks, a larger terminal graph union,
   lower stage 0 downstream wait, and a reproducible throughput gain over the 3,369.71 tok/s
   baseline. If the counters and timeline do not improve, revert the change and form a different
   hypothesis.
5. **Only after that, resume microbatch/placement work.** `256×128` in one context is currently unsupported because of the GGML
   assertion, and 15:13 gives only +0.4%, so neither experiment is repeated.
   Only after the scheduler is fixed, re-measure 2×128 against 4×64 and the stage cut.

This order preserves the already verified P4 relay path and limits the next change to the CUDA submit/execute
layer. It is not a declaration that communication optimization is finished; it rules out the communication hypothesis with sufficient
measurement and narrows the next target to the GPU scheduler hypothesis.

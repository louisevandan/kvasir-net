# Runtime evidence

## 2026-08-13: 246 tok/s, and stage cost follows the role rather than the device

Same 35B MoE, 32 concurrent requests capped at 1000 tokens, 400-token prompts,
`batch 256 / ubatch 128`, measured after the terminal-clearing fix below.

| Run | placement | aggregate | vs single session |
| --- | --- | ---: | ---: |
| single session | 20/20, 3090 first | 66.0 tok/s | 1.00x |
| `d3`, before any of this | 16/24, 4080 first | 74.3 tok/s | 1.13x |
| baseline | 20/20, 3090 first | 196.2 tok/s | 2.97x |
| terminal fix | 20/20, 3090 first | 206.4 tok/s | 3.13x |
| stage swap | 20/20, **4080 first** | **246.1 tok/s** | **3.73x** |

The swap is the measurement that reinterprets the earlier stage timings:

| placement | first-stage compute | last-stage compute |
| --- | ---: | ---: |
| 3090 first | 3090 **64.2 s** | 4080 39.5 s |
| 4080 first | 4080 **64.6 s** | 3090 31.7 s |

Whichever card leads costs about 64 s and whichever trails costs 32-40 s, so
the per-layer cost belongs to the stage role, not to the device. The earlier
reading that the 4080 was 1.62x faster per layer was wrong; the last stage is
simply cheaper than the first, and the 3090 is the faster card once it holds
the role that shows it, at 31.7 s against the 4080's 39.5 s.

It also closes the `d3` question. A 4080 in the first position cost 311.5 s of
compute then and 64.6 s now, so the 4.75x penalty was the reservation defect
against a 16 GiB card, not the role or the device.

At 3.23 s per first-stage layer and 1.59 s per last-stage layer, an even split
of work looks like 13/27 rather than 20/20. Measuring it says otherwise.

| first-stage layers (4080) | aggregate | wall | stage compute | first-stage wait |
| ---: | ---: | ---: | --- | ---: |
| 13 | 229.1 tok/s | 137.8 s | 49.7 / 51.2 | 85.9 s |
| **20** | **246.1 tok/s** | 129.0 s | 64.6 / 31.7 | 62.0 s |
| 24 | 9.1 tok/s | 3519.1 s | 3283.5 / 72.8 | 233.3 s |

Balancing the two stages made it slower. The wall tracks
`first compute + last compute + about 32 s`, so the stages do not overlap at
all and what matters is the sum, not the balance: moving seven layers to the
last stage saved 14.9 s of first-stage compute and added 23.9 s of waiting.
The 24-layer row is the other bound — 13,586 MiB of weights plus buffers
brushes the 4080's 16 GiB and the driver spills to host memory, which costs
50x. Twenty layers, about 12.1 GiB, is the practical ceiling for a 16 GiB
first stage.

Session count is the other axis the fix opened, and it is close to saturation:

| sessions | aggregate | wall | stage compute | first-stage wait |
| ---: | ---: | ---: | --- | ---: |
| 32 | 246.1 tok/s | 129.0 s | 64.6 / 31.7 | 62.0 s |
| 48 | 254.6 tok/s | 185.1 s | 94.4 / 43.8 | 87.3 s |

Half again as many sessions bought 3.5%. Per-step cost now grows nearly
linearly with batch width, so the remaining headroom is the 47% of wall clock
the first stage spends waiting for the round trip, which only overlap can
recover.

## 2026-08-13: a stale graph terminal was reserving the unowned layers

`llm_graph_result::reset()` cleared `t_linkcpp_inputs`,
`t_linkcpp_input_nodes`, `t_linkcpp_outputs` and `linkcpp_tensor_layers`, but
not `t_linkcpp_terminals`. A non-final stage expands its terminals into the
freshly pruned graph, and a terminal left from an earlier build still depends
on every layer of the full graph, so it dragged the layers the stage does not
own back in. Those weights are `no_alloc` metadata tensors, so once reachable
the scheduler reserved their bytes as compute buffer.

An audit added to `apply_linkcpp_stage`, now behind
`LINKER_STAGE_REACH_AUDIT`, walks the final graph and reports every reachable
tensor with no buffer whose layer falls outside the stage range. On the 35B
MoE with `layers=[0,20)`:

| Build | unowned tensors | unowned MiB | holder |
| --- | ---: | ---: | --- |
| first | 0 | 0.00 | none |
| second | 269 | 8,525.25 | `ffn_moe_down-34` (`MUL_MAT_ID`, layer 34) |
| third | 340 | 10,837.41 | same |

The accumulation across builds is the leak, and 10,837.41 MiB matched the
stage's 10,847.68 MiB compute buffer. The final stage never expands terminals
and reported zero throughout, which is why only non-final stages carried the
term.

Adding `t_linkcpp_terminals.clear()` to `reset()` removes it:

| First stage `[0,20)` | before | after |
| --- | ---: | ---: |
| reachable unowned tensors | 340 | **0** |
| `CUDA0` compute buffer | 10,847.68 MiB | **15.27 MiB** |
| stage total | 22,169 MiB | **11,337 MiB** |

That returns 10.6 GiB to the first-stage device. A node-array aliasing
hypothesis was tested first — snapshotting the order before
`ggml_graph_clear` — and rejected: the audit reported byte-identical numbers.

## 2026-08-13: placement alone takes 32 sessions from 74 to 196 tok/s

The first multi-session measurement on the placement the single-session run
validated: `0:20` on the 3090 as first stage, `20:40` on the 4080 as last, 32
concurrent requests capped at 1000 tokens, 400-token Korean prompts,
`batch 256 / ubatch 128`. Every earlier throughput run used `0:16` on the
4080 as first stage.

| | `d3` (4080 first, 16/24) | this run (3090 first, 20/20) |
| --- | ---: | ---: |
| accepted / done / errors | 32 / 32 / 0 | 32 / 32 / 0 |
| wall clock | 417.1 s | **160.3 s** |
| **aggregate throughput** | 74.3 tok/s | **196.2 tok/s** |
| per-stream throughput | 2.32 tok/s | 6.13 tok/s |
| first-stage compute | 311.5 s | **65.5 s** |
| last-stage compute | 56.7 s | 41.0 s |
| first-stage downstream wait | 103.2 s | 91.5 s |
| native occupancy | peak 32 | `active=32 in_flight=32 peak=32` |

Aggregate throughput is 2.97x the 66 tok/s single session, so the objective in
the handoff's section 0 is met for the first time. The gain is entirely
placement: first-stage compute fell 4.75x for four *more* layers.

That also corrects an earlier reading. The "front stage costs about eight times
the last stage per layer" figure was not a property of being the front stage.
It was the 4080 holding the front role while a non-final stage reserves the
whole model's 22.17 GiB against its 16.0 GiB of VRAM. Move the front role to a
24 GiB card and the term disappears. The memory defect and the throughput
collapse are the same defect seen from two sides.

Headroom remains and it is now the pipeline bubble, not admission. With all 32
sequences admitted and alive in the native scheduler, `nvidia-smi` sampled
through generation gives the 3090 a 32.5% mean and the 4080 53.4%. Stage
compute sums to 106.5 s against a 160.3 s wall, and the first stage spends
91.5 s waiting downstream. If the stages overlapped, the wall would approach
the slower stage's 65.5 s, which is about 480 tok/s.

## 2026-08-12: three all-3090 stages reach 8-way concurrency, then the middle stage faults

Dropping the 16 GiB 4080 and running three RTX 3090 stages — local plus the two
on `192.168.0.29` — follows directly from the previous entry: a non-final stage
needs the whole model's footprint, about 22.17 GiB, which a 24 GiB card can
hold and a 16 GiB card cannot. Artifacts are under
[`target/three-node-20260812/`](../target/three-node-20260812/).

The topology is sound. All three stages loaded, `P4_HEALTH` reported ready, and
the first stage logged
`op=wavefront active=8 in_flight=8 peak=8 limit=8 capacity=8`.

That is the first concurrency observed since the arrival-axis regression: the
four-node run and every run after `p4_max_inflight` was dropped from
`node_spec` reported `peak=1`. It is not a repository first — `c1c` reached
`peak=16` and both `c3b` and `d3` reached `peak=32` before the regression. What
it establishes is narrower and still useful: the fix restores the width those
earlier runs had, across hosts, and the native scheduler was never the tier
that refused concurrency.

Then `remote-3090-a`, `stage_index=1`, exited with `exit_code=3221225477`
(`0xC0000005`, access violation) and all eight requests failed with
`pipeline stage control pipe closed`. This is the same fault class seen earlier
on an asymmetric local placement, now on a 24 GiB card, so card size alone does
not explain it. A three-stage group has two non-final stages, each reserving
about 22.17 GiB of a 24 GiB card and leaving roughly 1.8 GiB for KV cache,
context and fragmentation. The middle stage is the one that takes both the
`cut_at(begin)` input path and the `cut_at(end)` output path.

Iteration then stopped for an environmental reason worth recording. The remote
supervisor does not survive its child vanishing, which is the standing defect,
and after that first crash it would no longer stay up at all: it starts, serves
`/api/runtime` with 200, and exits within about two minutes leaving nothing in
`supervisor.log`, `launcher.out.log` or `launcher.err.log`. Restarting the
remote agent and adapter cleared their stale NodeSlot state but not this. No
native stage log was captured for the crash because the sink dies with the
supervisor; `LINKER_NATIVE_LOG_DIR` was set on the remote for later attempts
and produced no files for the same reason.

## 2026-08-12: a non-final stage reserves the weight of the layers it does not own

Loads of `Ornith-1.0-35B-UD-Q5_K_S.gguf` (qwen35moe, `n_expert=256`,
`n_expert_used=8`, `n_embd=2048`) on a local 3090 + 4080, layers `0:16` and
`16:40`, driven through the owned E2E with one request capped at one token.
Only the load path matters here. Logs are under
[`target/graph-buffer-sweep-20260812/native-logs/`](../target/graph-buffer-sweep-20260812/native-logs/).

| parallel | `n_ctx` | `n_ubatch` | first `CUDA0` | last `CUDA0` | first `CUDA_Host` |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1,024 | 128 | **13,124.56 MiB** | 124.77 MiB | 125.73 MiB |
| 8 | 8,192 | 128 | **13,194.40 MiB** | 124.25 MiB | 134.22 MiB |
| 32 | 32,768 | 128 | **13,461.90 MiB** | 425.27 MiB | 163.33 MiB |
| 1 | 1,024 | 16 | **13,110.96 MiB** | 15.53 MiB | 16.79 MiB |

Both stages receive identical `llama_context_params`. The last stage scales
with context and micro-batch exactly as expected. The first stage does not
scale with any of them: 32x the context, 32x the sequences and an eighth of the
micro-batch all leave it within 350 MiB of the same ~13.1 GiB. It is a fixed
reservation, present with a single sequence and a 1,024-token context, and it
is 105x the last stage's allocation under the same parameters. Across 16
layers that is 819 MiB of compute buffer per layer against the last stage's
0.6 MiB.

The same sweep on `Qwen2.5-1.5B-Instruct-Q8_0` (dense) shows no such term —
first `CUDA0` moves 15.71 / 17.46 / 43.08 MiB across the same parallel values
while the last stage holds 74.94 MiB. The reservation is specific to the MoE
model on the first-stage code path.

This falsifies the earlier reading recorded in the pipeline throughput handoff,
which attributed the first stage's buffer and its `graph splits = 2` to the
boundary hidden-state copy and called it normal. At `n_ubatch=16` the boundary
frame is 64 KiB while `CUDA_Host` is 16.79 MiB and `CUDA0` is still 13.1 GiB,
so the copy cannot account for either. Because the term is independent of
batching, no admission, queue or scheduler change can remove it.

Two consequences follow. The first-stage device loses 13.1 GiB before any
weight is placed, which is why a 16 GiB 4080 in the first position is planned
with very few layers, and it is the leading suspect for the first stage costing
roughly eight times the last stage per layer during generation.

### The compute buffer is the weight of the layers the stage does not own

Holding the model, `parallel=1`, `n_ctx=1024` and `n_ubatch=16` fixed and
moving the stage boundary. Stage 0 ran on the 4080 and stage 1 on the 3090 in
every row; process-to-range mapping is taken from the supervisor spawn records,
not inferred from file order.

| stage 0 layers | stage 0 model | stage 0 compute | sum | stage 1 layers | stage 1 model | stage 1 compute |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0:4 | 2,264.35 | 19,900.82 | 22,165 | 4:40 | 20,996.45 | 15.53 |
| 0:16 | 9,057.39 | 13,110.96 | 22,168 | 16:40 | 14,203.41 | 15.53 |
| 0:30 | 16,986.11 | 5,185.43 | 22,172 | 30:40 | 6,274.69 | 15.53 |

One layer of this model weighs 566.2 MiB, and every model-buffer figure above
is that figure times the layers the stage owns. The first stage's compute
buffer is then `566.2 MiB x (40 - owned) - 478 MiB`: it is the weight of the
layers it does not own. Model plus compute is 22.17 GiB in all three rows, so
splitting the model changes only how the first stage's memory is labelled, not
how much it takes. The last stage carries no such term and sits at 15.53 MiB
whether it holds 10 layers or 36.

The reservation is real, not a reported estimate. Sampling `nvidia-smi` through
the `0:4` run shows the 4080 holding 15.7 GiB of its 16.0 GiB for the whole
group lifetime while owning four layers, or 2.26 GiB of weights.

The dense `Qwen2.5-1.5B` control does not show it: its first stage stays at
15-43 MiB rather than reserving its unowned layers. The term therefore belongs
to the MoE expert tensors of unowned layers on the non-final stage path, where
`llama_model_loader` substitutes a metadata tensor for every tensor outside
`[linkcpp_layer_begin, linkcpp_layer_end)` and `apply_linkcpp_stage` is
expected to prune the corresponding nodes. The final stage proves the pruning
works there; the first stage shows it does not, and its debug line reports
`inputs=0 outputs=1`, so the earlier guess that a large output cut-set retained
per-layer scratch is ruled out.

Consequence: pipeline splitting currently gives a non-final stage almost no
memory relief. A four-stage group has three such stages, which is why the
planner gives the 16 GiB 4080 six layers in the four-node plan.

## 2026-08-12: four-node run completed 16/16 while every session ran alone

Local RTX 3090 + RTX 4080 and remote RTX 3090×2, four stages
`[0,11] [11,17] [17,29] [29,40]`, 16 concurrent requests capped at 16 tokens.
Artifacts are under
[`target/four-node-current-20260812/`](../target/four-node-current-20260812/).

| Observation | Result |
| --- | --- |
| planned / accepted / `DONE` / error | 16 / 16 / 16 / 0 |
| `INGRESS_ACCEPTED` latency mean | 1.852 ms — async acceptance works |
| first token mean / max | 18,082 ms / 36,061 ms |
| wall clock / generated tokens | 36,731 ms / 256 |
| aggregate throughput | **6.97 tok/s** |
| declared adapter capacity | `max_sequences=16 source=stage_plan` |
| adapter batch sizes | `requests=1`, all 16 of 16 batches |
| native occupancy | `active=1 in_flight=1 peak=1 limit=16 capacity=16` |

The three tiers agreed on a width of 16 and the run still executed one session
at a time. The cause is upstream of both the adapter and the native scheduler:
`run-pipeline-e2e.mjs` stopped sending `p4_max_inflight` when the plan-width
copy was removed, so each NodeSlot fell back to its one-permit default. The
async relay waits for that permit and holds it to the terminal response, so the
Agent released one execution at a time, the adapter's 5 ms coalescing window
had nothing to merge, and the native scheduler never saw a second sequence.

Two corrections followed. The runner now declares the arrival axis explicitly
(`P4_AGENT_SLOT_WIDTH`, defaulting to `concurrent_requests`), and every summary
carries `throughput.aggregate_tps` with an admission verdict, so a run that is
correct but serialised reports `serialized` instead of passing silently. Rerun
this configuration before quoting any four-node throughput number; the table
above measures the defect, not the topology.

## 2026-08-11: P4B1 v5 persistent-route and stock llama.cpp continuous-batch proof

The stock E2E launched unchanged `llama-server.exe` with `parallel=8`,
`ctx-size=8192` (1024 per slot), `batch-size=2048`, and `ubatch-size=512`.
One Node.js process used two logical layers of multiplexing: all eight external
ingress streams shared one Agent link, and all eight Agent execution routes
shared one adapter link. The adapter reported its independent bounded admission
as `max_inflight=256`, `max_queued=1024`.

| Observation | Result |
| --- | --- |
| requested / accepted / text / `DONE` / error | 8 / 8 / 8 / 8 / 0 |
| session IDs | eight distinct controller-issued IDs |
| completion order | `4, 0, 2, 3, 6, 7, 5, 1`; no batch barrier |
| llama-server slot evidence | slots `0..7` each logged `processing task` |
| generated text | one `안녕하세요!`; seven `안녕하세요 (Annyeonghaseyo)` |
| adapter errors | none |

The raw server proof is
[`llama-server-20260811170006.err.log`](../target/real-e2e/llama-server-20260811170006.err.log),
and the eight adapter admissions are in
[`p4-llamacpp-20260811170006.log`](../target/real-e2e/p4-llamacpp-20260811170006.log).
This proves that the enlarged adapter scheduler can fill all configured stock
server slots while preserving independently completed P4 streams. It does not
claim that eight slots or these batch sizes are optimal for another model,
device, or backend.

A follow-up run used the explicit state-machine settings `max_batch=8` and
`partial_linger_ms=1000`. All eight routes again emitted text and `DONE`; the
adapter accepted eight executions and llama-server logged eight task launches.
The deterministic scheduler tests separately held the one-second linger and
proved both transitions: the second item completed a two-item full batch and
dispatched it after about 20 ms, while an execution-completion cycle hint
released a one-item partial batch without waiting for the remaining linger.
Evidence: [`p4-llamacpp-20260811170738.log`](../target/real-e2e/p4-llamacpp-20260811170738.log)
and [`llama-server-20260811170738.err.log`](../target/real-e2e/llama-server-20260811170738.err.log).
For stock HTTP the cycle hint is request completion, not direct GPU kernel-cycle
telemetry; the scheduler exposes a separate hint/heuristic seam for that future
improvement.

## 2026-08-10: abstract Agent ingress-credit correction

The 256-session experiments showed that P4 transport retained every valid
request/response pair and that CUDA-specific work was concentrated below the
adapter boundary. The selected protocol improvement is therefore not another
llama.cpp/CUDA change: the Agent now acquires the generic NodeSlot execution
credit before it emits `INGRESS_ACCEPTED`. A saturated slot produces only
`ERROR`, so an accepted ingress is no longer a promise that can immediately be
withdrawn by admission failure.

| Verification | Result |
| --- | --- |
| ready credit | mock adapter observes `EXECUTE`; client receives `INGRESS_ACCEPTED` then `DONE` |
| saturated credit | client receives `ERROR` and no `INGRESS_ACCEPTED` |
| backend assumptions | none; test uses only P4 frames and a local TCP mock adapter |
| wire revision | unchanged (`P4B1 v3`) |

This does not claim a CUDA throughput gain. It makes multi-controller ingress
backpressure truthful at the Agent boundary and applies unchanged to llama.cpp,
vLLM, SGLang, CPU, CUDA, HIP, Vulkan, Metal, or another adapter.

## 2026-08-09: native microbatch 256×500 success

The native first-stage scheduler now groups ready sequence windows into a
physical `min(batch, ubatch)` microbatch and waits for a complete replacement
batch before refilling an in-flight wavefront. The proof used an isolated CUDA
runtime pack (build ID suffix `498d73d6b9ad`) on a temporary supervisor at port
`18083`; it did not replace the active host runtime.

| Artifact | Contents |
| --- | --- |
| [plan-20260809161709.json](../target/pipeline-e2e/plan-20260809161709.json) | 256 persisted Korean prompt requests, `max_tokens=500`. |
| [trace-20260809161709.jsonl](../target/pipeline-e2e/trace-20260809161709.jsonl) | Every P4 ingress, token, and terminal response. |
| [summary-20260809161709.json](../target/pipeline-e2e/summary-20260809161709.json) | Counts and latency distributions. |
| [report-20260809161709.md](../target/pipeline-e2e/report-20260809161709.md) | All 256 prompt/final-output pairs. |

| Measurement | Observed value |
| --- | ---: |
| planned / accepted / first token / `DONE` / `ERROR` | 256 / 256 / 256 / 256 / 0 |
| P4 text events / aggregate event rate | 56,975 / 550.206 events/s |
| concurrent wall-clock | 103,552.076 ms |
| accepted / TTFT / `DONE` p95 | 131.114 / 8,365.671 / 102,766.373 ms |
| stage-local observed maximum batch size | 128 on both stages |
| pipeline peak / terminal credit | 256 / 0 (`issued = returned = 72,093`) |

The prior 256×500 artifact took 302,396.880 ms for 57,439 events. This new run
is a comparable harness result, not a controlled repeated experiment, but it
reduced wall-clock by 65.8% and raised aggregate event rate by about 2.9× while
preserving all request/response pairs. The runtime is documented by
[`apps/llama` scheduler internals](../../llama/docs/internals.md#multi-token-prefill).

## 2026-08-09: 500-token concurrency sweep, 100/50/10/2/1

Five fresh two-GPU Pipeline runs used the same model, 1,024 context tokens per request, a 500-token output cap, deterministic prefixes of one prompt family, and full plan/trace/report artifacts. Every planned request was accepted, streamed text, and ended in `DONE`; no run had a P4 `ERROR`. The complete linked evidence is [sweep-20260809150430.md](../target/pipeline-e2e/sweep-20260809150430.md) and [sweep-20260809150430.json](../target/pipeline-e2e/sweep-20260809150430.json).

| Concurrent sessions | P4 events/s | TTFT p50 / p95 ms | `DONE` p50 / p95 ms | Natural stop / length cap |
| ---: | ---: | ---: | ---: | ---: |
| 100 | 247.311 | 3,437.945 / 3,592.087 | 18,761.322 / 25,505.134 | 99 / 1 |
| 50 | 243.363 | 2,132.378 / 2,202.624 | 7,869.504 / 9,268.574 | 50 / 0 |
| 10 | 178.148 | 1,101.835 / 1,117.138 | 1,882.353 / 2,068.411 | 10 / 0 |
| 2 | 89.067 | 452.436 / 506.389 | 829.396 / 880.565 | 2 / 0 |
| 1 | 14.705 | 433.752 / 433.752 | 2,440.899 / 2,440.899 | 1 / 0 |

The observed aggregate-event-rate peak is 100 sessions, but it is only 1.62% above 50 sessions while its p95 TTFT is 63.1% higher and p95 completion latency is 175.2% higher. For this model and host, use 50 as the current balanced batch setting, use 100 only when maximizing aggregate throughput outweighs tail latency, and use 10 or fewer for latency-sensitive traffic. The rise from 1→50 is consistent with better GPU utilization from concurrent sequence work; the 50→100 plateau with rising latency is consistent with a saturated shared Pipeline/native execution path and increased queueing. This is one deterministic sweep with different prompt-prefix sizes and naturally varying output lengths, so repeat it before turning the operating guidance into a hard policy.

## 2026-08-09: scripted 256×500 request/response proof

The owned Pipeline E2E first generated and persisted exactly 256 input requests, then opened all 256 ingress streams concurrently with `max_tokens=500`. It persisted the frame-level response trace and generated the report only after all streams terminated. Plan and trace `request_id` sets both contain 256 unique IDs and match exactly.

| Artifact | Contents |
| --- | --- |
| [plan-20260809144839.json](../target/pipeline-e2e/plan-20260809144839.json) | Exact 256 prompts and fixed sampling request fields before ingress. |
| [trace-20260809144839.jsonl](../target/pipeline-e2e/trace-20260809144839.jsonl) | Every `INGRESS_ACCEPTED`, `TOKEN`, and terminal `DONE`/`ERROR` per request. |
| [summary-20260809144839.json](../target/pipeline-e2e/summary-20260809144839.json) | Machine-readable count and latency distributions derived from the trace. |
| [report-20260809144839.md](../target/pipeline-e2e/report-20260809144839.md) | All 256 input/final-output pairs and session-level first-token/completion latency. |

| Measurement | Observed value |
| --- | ---: |
| planned / accepted / first token / `DONE` / `ERROR` | 256 / 256 / 256 / 256 / 0 |
| maximum output tokens per request | 500 |
| natural stop / output-length stop | 228 / 28 |
| P4 text events / native generated tokens | 57,439 / 57,439 |
| concurrent request wall-clock | 302,396.880 ms |
| model load to ready binding | 20,968.571 ms |
| native KV allocation | 7,985,976,320 B |
| total / per-request context | 262,144 / 1,024 tokens |

| Per-request event latency | min | mean | p50 | p95 | p99 | max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| ingress accepted | 128.288 | 133.993 | 133.745 | 138.224 | 138.731 | 138.848 |
| first text token | 532.918 | 7,656.837 | 7,702.827 | 8,146.075 | 8,183.449 | 8,190.316 |
| `DONE` | 917.304 | 166,673.330 | 144,088.042 | 302,172.541 | 302,173.294 | 302,173.666 |

The report preserves the exact prompt/final-streamed-text relationship. Text answer quality is separate from transport correctness; 28 responses stopped at the configured 500-token cap and 228 stopped naturally.

## 2026-08-09: traceable `parallel=256` request/response proof

The maximum-native-capacity test was repeated with per-session evidence. The runner wrote the submitted `INGRESS_SUBMIT` data, the `INGRESS_ACCEPTED`, every `TOKEN`, terminal `DONE`/`ERROR`, and the joined final streamed text for each concurrent request. The result contains 256 JSONL rows: 256 accepted, 256 `DONE`, zero `ERROR`, and 4,084 P4 text events.

| Artifact | Contents |
| --- | --- |
| [trace-20260809144038.md](../target/pipeline-e2e/trace-20260809144038.md) | All 256 prompt → final-text pairs, session IDs, and terminal reasons. |
| [trace-20260809144038.jsonl](../target/pipeline-e2e/trace-20260809144038.jsonl) | Full per-session P4 request/response event sequence, including every streamed text chunk. |
| [client-20260809144038.log](../target/pipeline-e2e/client-20260809144038.log) | Lifecycle, aggregate timing, resource draft, and trace artifact paths. |

| Input prompt | Final streamed text | Terminal |
| --- | --- | --- |
| `Rust 언어를 한국어로 간단히 설명해. 핵심 특징을 한 문장으로 포함해.` | `Rust은 고성능이고 안전한 프로그래밍 언어` | `length`, 16 |
| `C 언어를 한국어로 간단히 설명해. 핵심 특징을 한 문장으로 포함해.` | `C 언어는 간단하고 효율적인 프로그래밍 언` | `length`, 16 |
| `C++ 언어를 한국어로 간단히 설명해. 핵심 특징을 한 문장으로 포함해.` | `C++는 객체지향 언어로, 데이터와 함수를 분리` | `length`, 16 |
| `C# 언어를 한국어로 간단히 설명해. 핵심 특징을 한 문장으로 포함해.` | `C#은 객체지향 언어로, 변수와 함수를 쉽게 사용` | `length`, 16 |
| `Java 언어를 한국어로 간단히 설명해. 핵심 특징을 한 문장으로 포함해.` | `Java는 객체지향 프로그래밍 언어로, 복잡` | `length`, 16 |

Each request used the 16-token output cap, so `reason=length` and incomplete sentences are expected. This is complete transport evidence, not answer-quality evidence.

## 2026-08-09: native Pipeline capacity ceiling and `parallel=256` proof

The planner estimated that `parallel=1000` with `1,024` context tokens per request would fit GPU memory, but the measured native load failed before KV allocation: `llama_init_from_model: failed to initialize the context: n_seq_max must be <= 256`. This is a native Pipeline/llama runtime ceiling, not a P4 controller or Agent queue result, and means VRAM cannot establish a higher usable session count for the currently linked binary.

The owned E2E then set the actual maximum, `parallel=256`, `ConcurrentRequests=256`, `P4_AGENT_MAX_INFLIGHT=256`, and `262,144` total context tokens. It generated 256 distinct Korean prompts in the form `<language> 언어를 한국어로 간단히 설명해. <style>` from programming and human language names. Every stream emitted text and reached `DONE`; no session was queued for later execution.

| Measurement | Observed value |
| --- | ---: |
| failed native probe | `parallel=1000`, `n_seq_max <= 256` |
| completed concurrent ingress requests | 256 / 256 |
| distinct prompts | 256 |
| output cap per request | 16 tokens |
| aggregate P4 streamed text events | 4,083 |
| concurrent request wall-clock | 22,631.502 ms |
| aggregate streamed-event rate | 180.41 events/s |
| native KV allocation | 7,985,976,320 B |
| total / per-request context | 262,144 / 1,024 tokens |
| model load to ready binding | 20,890.301 ms |

The final native request summary is a last-request sample, not a percentile: 57 prompt tokens, 56 prefill tokens, 8,414.878 ms native TTFT, and 2,741.520 ms queue wait. The runner removed its exact runtime group and P4 processes. Raw artifact: `target/pipeline-e2e/client-20260809141921.log`.

## 2026-08-09: native Pipeline `parallel=20` simultaneous ingress

Command:

```powershell
.\scripts\run-pipeline-e2e.ps1 -P4ListenPort 29221 -Prompt '러스트에 대해 한국어로 설명하라.' -MaxTokens 32 -Parallel 20 -ConcurrentRequests 20
```

The planner and Pipeline runtime both received `parallel=20`. Because native context is divided among the parallel slots, the E2E calculated a total context of `20,480` tokens to preserve `1,024` tokens per request. The Agent gave the bound NodeSlot `p4_max_inflight=20`, opened 20 ingress streams concurrently, and every stream emitted text and reached `DONE`.

| Measurement | Observed value |
| --- | ---: |
| concurrent ingress requests completed | 20 / 20 |
| output cap per request | 32 tokens |
| aggregate P4 streamed text events | 637 |
| concurrent request wall-clock | 2,751.773 ms |
| aggregate streamed-event rate | 231.49 events/s |
| native KV allocation | 623,924,224 B |
| context total / per request | 20,480 / 1,024 tokens |

The final post-stress request also completed with `DONE(reason=length)`. The script removed its exact runtime group and P4 processes. Raw artifact: `target/pipeline-e2e/client-20260809140406.log`.

## 2026-08-09: Tokio Agent admission and two-controller proof

`p4-agent` accepted ingress through a Tokio multi-thread I/O runtime at `127.0.0.1:29221`; its relay pool is bounded by `P4_AGENT_MAX_INFLIGHT=64` by default. The owned Pipeline E2E created two distinct NodeSlots concurrently from two ControllerInstances (`pipeline-2gpu` and a marker slot), then loaded only `pipeline-2gpu` as a two-GPU Pipeline deployment. While one execution held that slot's default `p4_max_inflight=1` permit, a second execution received an immediate admission `ERROR`; it was not queued. The held stream completed, and the requested 500-token-cap Korean Rust prompt then completed with 267 streamed text events and `reason=stop`.

| Measurement | Observed value |
| --- | ---: |
| concurrent controllers / created NodeSlots | 2 / 2 |
| model-load to ready binding | 20,947.912 ms |
| P4 ingress to first text event | 149.015 ms |
| P4 ingress to `DONE` | 2,829.497 ms |
| native stage 0 / stage 1 compute | 1,293.745 / 176.339 ms |
| stage 0 → 1 hidden state | 311 frames / 1,008,884 B |
| stage 1 → 0 sampled token | 272 frames / 3,185 B |

The final adapter error log was empty and the script removed its owned P4 processes and Pipeline group. Raw artifact: `target/pipeline-e2e/client-20260809135821.log`.

## 2026-08-08: P4B1 v3 agent inventory, NodeSlot, binding, and ingress

The owned stock E2E started `p4-agent` at `127.0.0.1:29111` before the self-registering stock adapter. The external Node.js controller received `HARDWARE_REPORT` with one registered adapter and two NVIDIA GPUs, created model-free `gpu-0`, bound the configured model at generation `1`, then submitted ingress without a session ID. ControllerProcessor returned `nodejs-controller-example-session-1`, streamed `12` events, and removed only the binding; the process-owned llama-server remained available.

The owned two-GPU Pipeline E2E used the same lifecycle at `127.0.0.1:29211`: one registered adapter, two discovered GPUs, `NODE_CREATED(ready)` for `pipeline-2gpu`, progress and `DRAFT_REPORT`, `MODEL_BOUND(generation=1)`, ingress-issued session, `16` streamed events, and `MODEL_UNBOUND`. The model deployment was independently deleted in cleanup while the NodeSlot contract remained valid.

| Measurement | Observed Pipeline value |
| --- | ---: |
| GGUF model / layer / KV bytes | 1,640,622,080 / 1,392,656,384 / 15,619,072 B |
| native decode / P4 streamed events | 16 / 16 |
| time to first token | 328.686 ms |
| stage 0 -> 1 hidden-state traffic | 52 frames / 168,688 B |
| stage 1 -> 0 sampled-token traffic | 16 frames / 167 B |
| host-observed average transfer rate | 52,209,223.15 B/s |

This proves agent-side lifecycle separation and ingress relay, not durable controller registry, public authentication, or a throughput guarantee.

## 2026-08-08: P4B1 v2 adapter-owned execution options

`cargo test --workspace` passed the P4B1 v2 `EXECUTE` codec round trip and both adapter policy tests: stock llama.cpp preserves a backend option such as `top_p` while restoring P4-owned `model` and `stream`; Pipeline copies `top_p`, `top_k`, and `seed` while omitting an unsupported `repeat_penalty`.

The owned stock E2E used `run-inference.mjs`, whose `infer()` call supplied `options: { top_p: 0.9, top_k: 20, seed: 7 }`, at agent listener `127.0.0.1:29101`. The model streamed `12` P4 token events and ended with `P4_DONE`.

The owned two-stage Pipeline E2E used the same options at `127.0.0.1:29201` with prompt `러스트에 대해 설명하라.` and a `16` token cap. The current Pipeline parser accepted its supported subset and returned:

| Measurement | Observed value |
| --- | ---: |
| GGUF model / layer / KV bytes | 1,640,622,080 / 1,392,656,384 / 15,619,072 B |
| P4 streamed text events / native decode tokens | 16 / 16 |
| finish reason | `length` |
| time to first token | 342.563 ms |
| stage 0 -> 1 hidden-state traffic | 52 frames / 168,688 B |
| stage 1 -> 0 sampled-token traffic | 16 frames / 167 B |
| host-observed average transfer rate | 49,731,132.08 B/s |

The exact temporary Pipeline group and spawned P4 relay processes were removed by the scripts' `finally` blocks. This validates v2 transport and adapter filtering, not model-answer quality or a universal llama.cpp option set.

## 2026-08-08: real two-stage Pipeline request

Command:

```powershell
.\scripts\run-pipeline-e2e.ps1 -Prompt '러스트에 대해 설명하라.' -MaxTokens 128
```

The current `S:\models` inventory contained one GGUF file, `Qwen2.5-1.5B-Instruct-Q8_0.gguf`; this run did not select among multiple model sizes. The test created one P4 controller identity and routed its logical `pipeline-2gpu` node to a native Pipeline group with these stages:

| Stage | Logical node | GPU |
| --- | --- | --- |
| 0 | `p4-gpu-3090` | RTX 3090 (`GPU-38e6dbac-fee5-ac16-62d4-cfacbe02f8ed`) |
| 1 | `p4-gpu-4080` | RTX 4080 (`GPU-79caabbe-c843-631f-3cea-9c01e652c78c`) |

P4 received progress `0..99`, then `DRAFT_REPORT`, then `LOAD_PROGRESS=100` only after the group became `running`. Its health response was `ready=true`.

| Measurement | Observed value |
| --- | ---: |
| GGUF model bytes | 1,640,622,080 B |
| GGUF layer bytes | 1,392,656,384 B |
| native KV allocation bytes | 15,619,072 B |
| FFN allocation bytes | unavailable (`0`, not measured zero) |
| prompt / prefill tokens | 37 / 36 |
| native decode-token budget/count | 128 / 128 |
| P4 streamed text events | 122 |
| time to first token | 340.844 ms |
| stage 0 prefill / decode compute | 98.208 ms / 1,944.521 ms |
| stage 1 prefill / decode compute | 110.016 ms / 104.062 ms |
| stage 0 -> 1 hidden-state traffic | 164 frames, 532,016 B |
| stage 1 -> 0 sampled-token traffic | 128 frames, 1,421 B |
| host-observed average transfer rate | 54,666,666.67 B/s |

The generated response was truncated by the requested token cap and included factual inaccuracies. It proves transport, resource reporting, and cleanup behavior; it does not certify model answer quality.

After the run, the exact `p4-adapter-e2e-*` group was deleted and no listeners remained on P4 ports 19201-19203 or Pipeline ports 52221-52222. The retained raw client artifact is `target/pipeline-e2e/client-20260808190619.log`.

## 2026-08-08: combined `p4-agent` reconstruction verification

The relay implementation was rebuilt into a policy-free listener/frame-forwarding base (`layers/runtime/src/foundation/transport/mod.rs`) and independent ControllerProcessor/NodeProcessor policies (`layers/runtime/src/application/routing/processor/mod.rs`). The default `p4-agent` invokes the latter two in-process; only the adapter boundary remains a P4 TCP connection.

`tools/scripts/e2e/stock/run-real-e2e.ps1` then completed an owned stock CPU `llama-server` request through `p4-agent` with 12 streamed tokens and `P4_DONE`.

`tools/scripts/e2e/pipeline/run-pipeline-e2e.ps1 -Prompt 'P4 agent 경로가 동작하는지 한 문장으로 답하라.' -MaxTokens 16` completed the actual two-GPU Pipeline path through the same combined agent:

| Measurement | Observed value |
| --- | ---: |
| P4 health | `ready=true` |
| GGUF model / layer / KV bytes | 1,640,622,080 / 1,392,656,384 / 15,619,072 B |
| prompt / prefill / native decode tokens | 47 / 46 / 16 |
| P4 streamed text events | 16 |
| time to first token | 350.292 ms |
| stage 0 prefill / decode compute | 100.878 ms / 591.312 ms |
| stage 1 prefill / decode compute | 117.116 ms / 37.466 ms |
| stage 0 -> 1 hidden-state traffic | 62 frames, 201,128 B |
| stage 1 -> 0 sampled-token traffic | 16 frames, 193 B |
| host-observed average transfer rate | 77,297,463.49 B/s |

The exact owned Pipeline group and all P4 relay processes are removed by the scripts' `finally` blocks; the raw artifact is retained under `target/pipeline-e2e/`.

## 2026-08-08: startup-selected listener ports

The rebuilt agent accepts its sole `LISTEN_ENDPOINT` argument and prints the resolved listener address. The owned stock E2E completed at `127.0.0.1:29017` with 12 streamed tokens. The owned 2-GPU Pipeline E2E completed at `127.0.0.1:29201` with `LOAD`, `DRAFT_REPORT`, `HEALTH ready=true`, 16 P4 token events, and `DONE`.

For the Pipeline run, the host reported 362.875 ms TTFT, 207,616 B of stage-0-to-stage-1 hidden-state traffic, 172 B of sampled-token return traffic, and 82,980,015.99 B/s average observed transfer rate. This proves that the listener port is a startup configuration, not a protocol constant.

## 2026-08-08: controller-supplied dynamic node route

The default agent was started with only `p4-agent 127.0.0.1:29019`; it received no node, GPU, model, or backend argument. The stock E2E first sent `LOAD` with `p4_agent.adapter_endpoint=127.0.0.1:19103`, received 0% then 100% progress, and completed 12 streamed tokens.

The two-GPU Pipeline E2E started the agent with only `p4-agent 127.0.0.1:29202`, sent the same control envelope with its Adapter endpoint and opaque Pipeline `adapter_request`, then completed `LOAD`, `DRAFT_REPORT`, `HEALTH ready=true`, 16 token events, and `DONE`. Its observed TTFT was 357.091 ms; hidden-state traffic was 62 frames / 201,128 B, sampled-token return traffic was 16 frames / 200 B, and host-observed transfer rate was 44,675,255.44 B/s.

This proves that concrete inference topology is protocol-supplied runtime state, not agent startup configuration.

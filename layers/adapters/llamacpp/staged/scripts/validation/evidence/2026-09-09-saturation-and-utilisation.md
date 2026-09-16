# 2026-09-09 — 10 experiments aimed at saturation and higher GPU utilisation

Type: measurement experiments. Model size, split, resident, issue policy and arrival pattern were changed one at a time.
Responses were judged **by reading the raw text directly**, not with `judge.mjs`.
Location: `m42-server2` (RTX 3090 ×2), working tree after commit `6901f236a`.

The numbers were recomputed from each run's preserved artifacts. To reproduce:

```
node test/benchmarks/p4-4node/measure-run.mjs target/saturation-20260909/<run id>
```

## Measurement table

`st`=stage count, `seq`=resident, `decMn/decMx`=mean/max decode batch rows, `fill%`=UBATCH(512) fill rate,
`idle%`=share of wall-clock time the head is idle, `ovl2`=share of time with 2 or more stages open at once,
`gpu0/1`=mean utilisation over the stage run window, `zero`=number of samples at 0 % in that window, `W`=mean power.

```
run                      scenario                 st seq    TPS   ITL  batch  decMn decMx  fill% idle%  ovl2 depth  gpu0  gpu1  zero0 zero1   W0    W1
20260909T034439Z-4ce8e2b pressure          (2B)    4 256 382.15   692   1367  76.21   160  15.95  59.0  62.2  3.18  21.4  31.9  299/1052  168/1052  98.8 193.8
20260909T055218Z-c9f80e0 pressure_2stage   (2B)    2 256 419.39 590.1   1637  63.44   128  13.32  42.4  53.0  3.81  26.2  32.9   184/959    93/959 111.6 160.0
20260909T060229Z-174791b pressure_35b              2  96 275.35 314.9   2622  39.34    64   8.43  24.7  66.0  2.24  41.5  39.4   28/1469    9/1469 198.4 206.4
20260909T064046Z-7674d59 pressure_35b_4stage_96    4  96 183.00 517.5   2416  42.67    64   9.15  57.0  68.5  2.02  36.8  37.4  452/2207  342/2207 181.2 192.7
20260909T062820Z-57406fe pressure_35b_4stage       4 256 276.62 952.0   1546  67.25   128  14.29  36.0  93.6  3.60  50.7  47.1  169/1461  104/1461 199.9 212.7
20260909T065812Z-e5b6a12 pressure_35b_wide         2 160 289.79 537.3   2430  42.49    64   9.09  21.0  73.0  3.29  44.5  40.9   23/1396    8/1396 198.7 206.9
20260909T071025Z-5cc7fc5   + min-batch-rows 64     2 160 234.58 641.5    879 119.45   160  25.14  55.4   3.0  1.03  27.1  26.6  815/1721  837/1721 175.0 184.4
20260909T072421Z-a198673 pressure_35b_stagger      2 160 223.10 659.9   4526  22.76    52   4.88  16.2  80.8  6.09  30.1  31.0   24/1813    0/1813 160.0 163.0
20260909T073806Z-d565861 pressure_35b_burst 64/2s  2 160 313.80 478.7   2021  51.12    64  10.94  22.9  71.2  2.70  41.8  40.8   16/1289   22/1289 203.4 210.5
20260909T074931Z-8853b3e pressure_35b_burst 128/4s 2 160 302.49 478.4   1929  53.65    96  11.46  22.9  68.0  2.55  43.7  38.7  100/1337   78/1337 197.6 205.4
```

All ten runs completed 512 requests and released 512, with `error` and `cleanup_error` both `null`.

## 1. The kernel-active ratio differed widely between 2B and 35B

| | 2B | 35B |
| --- | ---: | ---: |
| Mean over kernel-active window | 21.4–32.9 % | **36.8–50.7 %** |
| Power | 98.8–160 W | **175–212.7 W** |
| Samples at 0 % in the window | 9.7–28.4 % | 0–20.5 % |

**How to read it.** `utilization.gpu` from `nvidia-smi` is **the share of the sample interval during which at least one kernel
was running**. It is neither SM occupancy nor the saturation of compute resources. So **having no 0 % samples does not mean the card never
rested** — it only means that no sample interval passed entirely with nothing running.
Power is a separate observation and moved in the same direction.

**This is not attributed to a single cause.** Going from 2B to 35B changes more than model size. Architecture (hybrid
recurrent), quantization, flash attention, KV format, resident and cut all change together. "Model size determines utilisation"
is **a hypothesis not yet verified**; all this table says is that there was a large difference between the two
configurations. Attribution needs a comparison within one model with everything else fixed.

## 2. One stage per card. Putting two on a card loses 33 %

Resident was fixed at 96 and only the split was changed.

| | 2 stage (1/card) | 4 stage (2/card) |
| --- | ---: | ---: |
| Generation TPS | **275.35** | 183.00 |
| GPU0 / GPU1 | **41.5 / 39.4 %** | 36.8 / 37.4 % |
| Head idle | **24.7 %** | 57.0 % |
| Stage time | 105.8 / 127.5 ms (20 layers each) | 98.3 / 96.7 / 95.9 / 125.1 ms (10 layers each) |

A 10-layer stage takes about 97 ms and a 20-layer stage about 117 ms. The line through those two points has an intercept of about 77 ms per stage, but
**this value is not a fixed cost attributed by measurement.** The two points are averages of runs that differ in batch width, in stage role (head, middle, tail)
and in the number of processes sharing the same card. The intercept is an empirical value obtained by joining such averages by layer count,
and **it must not be read as a recoverable cost that can be subtracted from wall-clock time.**
Decomposition must use the parse/decode/sample/encode breakdown of the [STEP instrumentation](../../../server/src/server/server_physical.cpp).
The direction matches what the repository recorded on 09-04: "one stage per card is 33 % faster on 35B".

So the fact that `pressure_35b_4stage` (resident 256) had the highest utilisation at 50.7 % is **due to resident, not
to the split**. With 4 stages each stage carries half the weight share, which is merely what lets it pass the memory plan.

## 3. The load verdict blocks the best configuration

Collecting the plans of the 5 35B runs, the rule fits exactly. The card's initial free memory is 22.76 GiB (units are GiB).

| arm | n_seq | `CUDA0 RS buffer` | plan `free` | free + RS | `required` | Verdict |
| --- | ---: | ---: | ---: | ---: | ---: | :-- |
| 2 stage | 96 | 2,814 MiB = 2.75 | 20.01 | **22.76** | 14.01 | ✓ |
| 2 stage | 256 | 7,504 MiB = 7.33 | 15.43 | **22.76** | 19.72 | ✗ |
| 2 stage (retry) | 256 | 7,504 MiB = 7.33 | 15.43 | **22.76** | 19.72 | ✗ |
| 4 stage | 96 | 1,407 MiB = 1.37 | 21.38 | **22.75** | 6.96 | ✓ |
| 4 stage | 256 | 3,752 MiB = 3.66 | 19.09 | **22.75** | 10.14 | ✓ |

The recurrent state buffer is **already allocated before** the plan is built, so it is missing from `free`,
yet the `context` term of `required` counts the same buffer again. The verdict expression is
`entry.required() <= entry.free` in [stage_memory_plan.cpp:107](../../../../../server/src/runtime/stage_memory_plan.cpp),
and `free` is the `ggml_backend_dev_memory` value after context creation.

Strictly speaking, the cause is **not double-counting of weights but the actual allocation of the recurrent buffer used for planning**.
The planning model is created with `no_alloc=true`, but the recurrent constructor takes a real buffer. The `context` term contains
both that recurrent buffer and the attention KV (7,504 MiB and 680 MiB respectively at r256), and the two are handled differently, so
`model + compute + 2 × context` is a convenient approximation, not an exact condition.

The effect is clear. This hybrid 35B uses **32.0 MiB per sequence** (recurrent, independent of context length), so
with 2 stages the resident cap is cut far below the actual requirement. The 19.72 GiB actually needed at r256
fits on a 24 GiB card.

**This is the staged server (C++), a different layer under the isolation contract, so it was not fixed in this session.**
The fix direction is to align the no-alloc contract with the basis for computing available memory; **removing the fit check or compensating by adding
context to `free` is inappropriate.** "Fixed the false rejection" and "r256 runs safely" are different
completion conditions, and the combined reservation when several processes share one card cannot be replaced by individual stages passing.

## 4. UBATCH fill rate is not a target metric

Same configuration (2 stage, resident 160), with only `P4_STAGED_MIN_BATCH_ROWS=64` turned on.

| | Default | min-batch-rows 64 |
| --- | ---: | ---: |
| **Fill rate** | 9.09 % | **25.14 %** |
| Decode width mean / max | 42.49 / 64 | 119.45 / 160 |
| Physical batches | 2,430 | 879 |
| **Generation TPS** | **289.79** | 234.58 |
| **GPU0 / GPU1** | **44.5 / 40.9 %** | 27.1 / 26.6 % |
| GPU p50 | 47 / 39 % | **5 / 4 %** |
| 2+ stages overlapping | 73 % | **3 %** |
| Pipeline depth | 3.29 | **1.03** |
| `idle_gated` | 0 | 837 |

Waiting to gather width serializes the pipeline. Fill rate rose 2.8 times, while TPS fell 19 % and
the kernel-active ratio fell 40 %. **Fill rate is not a target metric.**

What `min-batch-rows` actually does is also recorded precisely. The gate in [drive.rs](../../../adapter/src/v2/node/worker/drive.rs)
defers issuing only when **work is in flight** and the number of eligible rows is below the threshold. If nothing is in flight,
it ignores the threshold. So this is not an option that "guarantees a minimum row count for every physical batch".

Joining 98 ms at width 32 and 278 ms at width 160 gives 1.41 ms per row and an intercept of 53 ms, but **the two points are averages of different
runs.** The min64 run's own mean at width 32 is 88.62 ms. This intercept is an empirical estimate of the same kind as the 77 ms in §2,
not a measured cost per cause.

## 5. Width is set by arrival bursts, not by resident

Raising resident from 96 to 160 (1.67 times) left the decode width distribution with **only two values, 32 and 64**, and the maximum unchanged.

| Decode width | resident 96 | resident 160 |
| ---: | ---: | ---: |
| 32 | 1,996 times | 1,612 times |
| 64 | 594 times | 786 times |
| 96 or more | **0 times** | **0 times** |

Requests that arrive together with the same prompt decode in the same step, while different bursts are out of phase in the pipeline
and cannot be issued together. So **the set that can be issued at once is the size of one or two bursts**.
Fixing the arrival rate at 32 per second and changing only the burst size shows this directly.

| Burst | Decode width mean / max | Physical batches | Generation TPS | Inter-token gap p50 | GPU0 / GPU1 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 4 / 125 ms | 22.76 / 52 | 4,526 | 223.10 | 659.9 ms | 30.1 / 31.0 % |
| 32 / 1 s | 42.49 / 64 | 2,430 | 289.79 | 537.3 ms | 44.5 / 40.9 % |
| **64 / 2 s** | **51.12 / 64** | **2,021** | **313.80** | **478.7 ms** | 41.8 / 40.8 % |
| 128 / 4 s | 53.65 / 96 | 1,929 | 302.49 | 478.4 ms | 43.7 / 38.7 % |

**Spreading arrivals evenly halves the width, increases the batch count by 86 %, and drops TPS by 23 %.**
Larger bursts widen batches without waiting, and between 32 and 64 requests per burst both TPS and latency improve.

**But width alone does not explain it.** From 64 to 128, the mean width rose from 51.12 to 53.65 while TPS
fell from 313.80 to 302.49. The kernel-active ratio **moved in opposite directions on the two cards** — gpu0 rose from 41.8 to 43.7 %
and gpu1 fell from 40.8 to 38.7 %. It must not be read as moving in one direction.
"When width gained without waiting grows, TPS and kernel activity rise together" **does not hold.**
What this table shows is **sensitivity to the arrival pattern**, not an improvement obtained with the input held fixed.

`pressure_35b_stagger` matters for a different reason. The tail's stage occupancy is 97.2 %, while the kernel-active ratio is
31 %. However, that 97.2 % is the **RPC occupancy from native request to response** measured by [drive.rs](../../../adapter/src/v2/node/worker/drive.rs),
not GPU compute time. The fact that the RPC stays open **cannot establish** which of
sampler, transfer, synchronization or kernel dominates. The gap between the two values is an observation that needs decomposition,
not a conclusion.

## 6. Responses — read and judged directly

The prompt was written by reading this model GGUF's `tokenizer.chat_template` so that it matches the string that template
emits with `enable_thinking: false`.

```
<|im_start|>user\n타입스크립트에 대해 한국어로 설명하라<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n
// English: "Explain TypeScript in Korean"
```

Across all 8 35B runs:

| Check | Result |
| --- | --- |
| `<think>` tag leakage | **0 / 512** (all 8 runs) |
| ChatML marker leakage | **0 / 512** (all 8 runs) |
| Most-repeated 24-character window ratio | 0.06–0.11 — confirmed by reading the raw text as repeated markdown structure, not degeneration |
| Distinct responses | 283–417 / 512 |
| Stop reason | 512/512 `length` — truncated at `max_tokens` 200 |

**Thinking suppression actually worked.** The Hangul ratio of 0.45, lower than 2B (0.54), is due to
TypeScript code blocks, not language drift (147/512 contain a code fence).

**The acceptance scope is stated explicitly.** The user decided to accept truncated responses as normal responses in this
**fixed-length load test**. That decision allows the figures of this experiment to be cited, but **it does not replace judgement of content, format
and completeness for a real service.** All 11 successful runs stopped with `length` at 200 tokens per request, and in the default r160
99 responses have an unclosed code block. Every TPS here is **generation speed under fixed-length load, before quality
approval**.

## What this document does not claim

- **Neither a hardware limit nor an optimum.** 313.80 TPS is **the best single-run value among the conditions tested**.
  The earlier statement "35B saturates at about 276 TPS" was contradicted by later runs and is **withdrawn**.
- Cause attribution. The difference between 2B and 35B, the gap between tail RPC occupancy and kernel activity, and the 53–77 ms intercepts are all
  observations before decomposition. parse, decode, sample and encode must be measured separately with `P4_STAGED_TRACE_STEP`.
- Service performance. Differences obtained by changing the arrival pattern are **input sensitivity**, and cannot be placed in the same column as
  results improved with the input held fixed.
- Normal service acceptance. All requests arrive within 14–16 seconds, followed by a backlog of hundreds of seconds. With 512 copies of the same short
  question, this falls short of the sustained condition requiring mixed prompts, at least 8 waves and a total of 8R or more.
  In the default r160, **median TTFT is 93.9 seconds and p90 is 197.6 seconds**. TTFT is not a measure of admission waiting alone, so
  it is not attributed to an admission bottleneck. Still, looking only at inter-token gaps hides this wait entirely.
- Statistical significance. Each condition had 1 run, with no paired repetitions. The differences serve only as candidate-selection data.
- Generalization to other models or cards. 32.0 MiB per sequence is a property of this hybrid model.
- Multi-physical-computer acceptance. The two 3090s are in one host.

## Judgements the current evidence allows

| Item | Judgement |
| --- | --- |
| 35B 2-stage/r96 vs 4-stage/r96 | 2-stage was better in that single run. Use as a **baseline candidate** |
| `min-batch-rows 64` | About 19 % TPS drop on the same input. **Excluded from adoption candidates** |
| Arrival pattern | Large effect on performance. Classified as an **input-sensitivity experiment** |
| Maximum throughput | 313.80 is **the best single-run value among the conditions tested**. Neither a limit nor an optimum |
| Width and throughput | From 64 to 128, width rose and TPS fell. **Width alone cannot explain it** |
| Kernel activity | Kernel-active time, RPC occupancy and SM occupancy must be distinguished |
| Intercepts 77 ms·53 ms | Estimates from averages under different conditions. **Do not cite as measured costs** |
| Memory plan | A defect where the actual allocation of the planning recurrent buffer shrinks available memory. **Sufficient basis for a fix** |

## Next steps

The execution order is owned by the [roadmap](../../../../../../../docs/distributed-batching-roadmap.md).
Only the items this document provides evidence for are listed.

1. **Preserve partial results of failures.** The 4 failed 4-stage runs had no `artifact.json`, so the first cause could not be separated
   from missing observation evidence, stream interruption or 10054. This is not a problem closed by a successful retry.
2. **Fix and verify the no-alloc recurrent defect (section 3).**
3. **B2/B3 acceptance and return budget.** This work was scenarios, measurement and documentation; that implementation did not advance.
4. **Repeated A/B after securing a complete-response baseline and STEP traces.** Do not promote a single-run best to an optimum.

**Keep the `outstanding > 0` check in `state.rs`.** The next decode needs the result of the previous token, so
removing this check to increase eligible rows is not an optimization but a dependency violation. Introducing speculative execution
would need a separate contract and verification for proposal, Verify, Replay and SETTLE.

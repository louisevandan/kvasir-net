# 2026-09-09 — Measured baseline of the first run in which `pressure` completed

Kind: measurement report. Response quality was judged **by reading the raw text directly**, not by `judge.mjs`.
No improvement is claimed. This scenario had never completed before, so there is nothing to compare against.

The subjects are the two runs that the [control response budget fix](2026-09-09-control-receipt-budget.md) let pass.
`m42-server2` (RTX 3090 ×2), commit `76d9bc3e3`, clean working tree, agent sha256 `eaa153f4231385ce…`.

Every number was recomputed from each run's preserved artifacts. Reproduction command:

```
node test/benchmarks/p4-4node/measure-run.mjs target/pressure-remote-receipt-fix-20260909/<run id>
```

This script reads only `artifact.json`, `config.json`, `report.json` and `gpu.csv`. It recounts throughput from the
responses and prints the harness value next to it, so any mismatch is visible. Both runs matched (382.15, 401.75).

## Configuration

| Item | Value |
| --- | --- |
| Model | `S:\models\unsloth\gemma-4-E2B-it-GGUF\gemma-4-E2B-it-Q8_0.gguf` |
| Cut | 4 stages `[0,5) [5,9) [9,13) [13,35)`, device `0,0,1,1` — **two nodes per card** |
| Layer split | 5 / 4 / 4 / **22** — gemma-4-E2B makes 13..34 a KV-sharing region, so no boundary can be placed inside it |
| Offloading | **None (VRAM-only).** Each stage's `--override-tensor` sends **layers it does not own** to CPU as a staging technique (node-3 uses `blk.(0..12)`); it is not RAM offloading |
| resident | `n-seq-max` 256, per-sequence context 512, `batch/ubatch` 512 |
| Workload | 512 requests, 16 waves of 32 every 1 s, `max_tokens` 200 |

## 1. Responses — read and judged directly

The prompt is identical for every request.

```
<|turn>user\n타입스크립트에 대해 한국어로 설명하라<turn|>\n<|turn>model\n
// English: <|turn>user\nExplain TypeScript in Korean<turn|>\n<|turn>model\n
```

**This format is this model's own.** It was confirmed by reading the GGUF's `tokenizer.chat_template` directly.
The template uses `<|turn>ROLE\n … <turn|>\n` and opens `<|turn>model\n` as the generation prompt. The system turn
opens only when `enable_thinking`, tools or a system message is present, so for this request with a single user message
the template produces the string above. The tokenizer adds BOS.

Response sample (A's `req-001`, beginning):

> \#\# 타입스크립트(TypeScript)에 대한 한국어 설명 (English: "## A Korean explanation of TypeScript")
> **타입스크립트(TypeScript)**는 **JavaScript에 정적 타입(Static Typing) 기능을 추가한 언어**입니다. … (English: "**TypeScript** is **a language that adds static typing to JavaScript**. …")
> \#\#\# 1. 왜 타입스크립트를 사용할까요? (핵심 필요성) (English: "### 1. Why use TypeScript? (the core need)")
> * **런타임 오류 (Runtime Errors):** … (English: "* **Runtime errors (Runtime Errors):** …") (cut off here: `* **유지` (English: "* **Maint"))

| Verdict item | A `…4ce8e2b1` | B `…9014d441` | My judgement |
| --- | --- | --- | --- |
| Topical fit | 512/512 are Korean-language TypeScript explanations | same | **Pass.** I read the samples, and they answer the question |
| Hangul ratio | min 0.50 / mean 0.54 | 0.49 / 0.54 | Pass. No drift into English |
| Repetition collapse | max share of the most repeated 24-character window **0.06** | 0.06 | Pass. No single-line loops |
| Distinct responses | 177/512 | 165/512 | Expected for an identical prompt. This is not a diversity test |
| Finish reason | **512/512 `length`** | same | **Fail.** Not a single natural stop |
| Ends with punctuation | **only 5/512** | 5/512 | Another expression of the same fact |

**Verdict: the content is meaningful, but these are not complete responses.** All 512 were cut at `max_tokens` 200,
and most end mid-sentence. This scenario was built to measure batch pressure, and 200 tokens is a setting for that
purpose, but **this run cannot be used as evidence of "a normal prompt and a normal response".** A separate run with a
budget that generates to completion and a stop condition is needed.

## 2. Throughput

| | A | B |
| --- | ---: | ---: |
| Generated tokens | 102,400 | 102,400 |
| Wall clock (arrival to last release) | 267.96 s | 254.89 s |
| **Generation TPS** | **382.15** | **401.75** |
| prefill / decode rows | 9,728 / 101,888 | 9,728 / 101,888 |
| Verify / Replay rows | 0 / 0 | 0 / 0 |
| Per-request inter-token interval (p50) | 692 ms | 698.5 ms |

The speed one request sees is **about 1.45 tok/s**. The total of 382 is the result of 256 requests running at that speed
at the same time.

Latency by arrival rank splits into two groups, because resident is 256 and there are 512 requests.

| Arrival rank (A) | Arrival time | TTFT p50 | End-to-end p50 |
| --- | ---: | ---: | ---: |
| 0–32 | 0 ms | 3,165 ms | 140,959 ms |
| 128–160 | 4,013 ms | 1,256 ms | 139,719 ms |
| 256–288 | 8,007 ms | **139,317 ms** | 257,457 ms |
| 384–416 | 12,004 ms | **135,578 ms** | 253,742 ms |

The first 256 requests get a slot immediately and emit their first token in 1.3–3.2 s. The last 256 **wait in full until
the earlier wave finishes and returns its slots.** Quoting a single overall median TTFT would give a value nobody
experienced.

## 3. Batch width and UBATCH fill ratio

The fill ratio is an observation, not a target metric. The 09-09 policy experiment showed that waiting to accumulate
width raises the fill ratio but lowers throughput and kernel activity ([evidence](2026-09-09-saturation-and-utilisation.md)).

| | A | B |
| --- | ---: | ---: |
| Physical batches | 1,367 | 1,281 |
| UBATCH | 512 | 512 |
| Mean rows per batch | 81.65 | 87.13 |
| **UBATCH fill ratio** | **15.95 %** | **17.02 %** |
| decode batch rows mean / **max** | 76.21 / **160** | 81.41 / **160** |
| prefill batch rows mean / max | 324.27 / 512 | 325.71 / 512 |

**Resident is 256, yet decode width never exceeded 160.** A's decode width distribution and the idle time just before
each width:

| Width | Count | Mean idle before | Mean head stage |
| ---: | ---: | ---: | ---: |
| 32 | 546 | 17.2 ms | 46.9 ms |
| 64 | 317 | 50.1 ms | 79.4 ms |
| 96 | 89 | 93.8 ms | 90.6 ms |
| 128 | 188 | 223.8 ms | 102.0 ms |
| 160 | 197 | 380.6 ms | 122.5 ms |

**The wider the batch, the longer it waited beforehand.** And this was not left behind by the issue policy.

- `idle_gated` = **0** (both runs)
- `ready_rows_left` = mean 2.7 / 2.9, **p50 0, p90 0** — practically no rows were left behind at planning time
- `ready_sequences` p50 **64**, max **160** — that was all the eligible sequences there were

So **the width ceiling lies in "what can be issued", not in "what to issue".**
`phase_within` in [state.rs:356](../../../adapter/src/v2/node/state.rs) returns `None` during decode when
`outstanding > 0`. A request whose previous decode is still in the pipeline cannot contribute any row to the next
batch. In a 4-stage pipeline `depth_mean` is 3.18 (3.07 for B), so a large share of resident is always in flight, and
width is cut by that much.

The head's total idle time is A **158 s / 267.96 s (59 %)** and B **145.4 s / 254.89 s (57 %)**.

Per-stage occupancy:

| | node0 | node1 | node2 | node3(tail) |
| --- | ---: | ---: | ---: | ---: |
| A service | 40.0 % | 29.6 % | 29.4 % | **76.6 %** |
| A mean stage time | 78.4 ms | 57.9 ms | 57.6 ms | **149.9 ms** |
| B service | 41.8 % | 30.8 % | 30.4 % | **78.5 %** |

`any_stage_open` 95.7 % / 95.9 %, `two_or_more_open` 62.2 % / 65.8 %.

**Layer count is the first candidate for why the tail is slow.** This cut is 5 / 4 / 4 / **22** layers. Fitting
`intercept + layers × slope` to the four stage times (a line through 57.9 ms at 4 layers and 149.9 ms at 22 layers) gives
5.11 ms per layer and an intercept of about 37 ms, and node2 at 4 layers with 57.6 ms lies on that line.

**But this intercept is not a fixed cost attributed by measurement.** The four points are means that differ in batch
width, stage role and the number of processes sharing a card, and the regression intercept is an empirical value that
connects them by layer count. It must not be read as a recoverable cost that can be subtracted from wall time. All that
can be said is that **the tail's 149.9 ms must not be explained directly by the sampler share**; the 09-04 35B
decomposition is not transcribed to this configuration. The breakdown must be measured with the
parse/decode/sample/encode of `P4_STAGED_TRACE_STEP`.

This observation justifies the `pressure_2stage` experiment. That run answers what changes when the stage count is reduced.

## 4. GPU utilization

Window definition: from the first sample where any card exceeds 5 % to the last. The load head and cleanup tail are excluded.

| | A gpu0 | A gpu1 | B gpu0 | B gpu1 |
| --- | ---: | ---: | ---: | ---: |
| Full-capture mean | 16.8 % | 25.3 % | 17.4 % | 25.0 % |
| **Window mean** | **18.8 %** | **28.2 %** | **19.5 %** | **28.1 %** |
| Window p50 / p90 / max | 14 / 46 / 100 % | 25 / 66 / 100 % | 18 / 46 / 100 % | 25 / 63 / 100 % |
| Samples at 0 % in window | **430/1,205** | 231/1,205 | 376/1,155 | 196/1,155 |
| Power mean / max | 93.0 / 203.1 W | 177.1 / 298.8 W | 93.2 / 206.5 W | 178.7 / 311.2 W |
| Memory max | 9,754 MiB | 9,754 MiB | 9,754 MiB | 9,754 MiB |

gpu1 is higher because of placement. The device list is `0,0,1,1`, so **the tail (node3) is on gpu1.**
gpu0 spends **36 % of the window at 0 %**. Power is at 27 %/51 % of the 3090 rating, and memory use is only
9.5 GiB of 24 GiB. `utilization.gpu` is the fraction of time a kernel was running, not SM occupancy, so
**this value alone cannot say whether compute resources are saturated.** That memory has headroom stands on its own as a
separate observation.

## What this document does not claim

- An improvement. There is no comparison baseline.
- Placing this configuration's values in the same column as 35B VRAM-only or other scenarios. Model, cut and offloading differ.
- The breakdown of the tail cost. That requires splitting parse, decode, sample and encode with `P4_STAGED_TRACE_STEP`.
- Multi-physical-computer acceptance. The two 3090 cards are in one host.

## Next actions backed by measurement

1. **Complete-response baseline.** Set `max_tokens` and the stop condition for this model, and build a separate run that
   judges quality on uncut responses. Right now 512/512 are cut, so there is no evidence of normal responses.
2. **Breakdown of the width ceiling.** Count sequences by reason they could not be issued, and record directly how many
   were excluded because of `outstanding > 0`. Right now there is only the result that `ready_sequences` peaks at 160,
   with no causal breakdown.
3. **Tail decomposition.** Split node3's 149.9 ms with the existing STEP instrumentation. No new instrumentation is needed.

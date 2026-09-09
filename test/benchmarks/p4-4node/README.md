# Four-node acceptance harness

> 문서 지위 (2026-09-06): **개발 하네스 안내**. 현재 하네스 사용법이다. 한 원격 호스트 시험을 다중 컴퓨터 최종 증명으로 세지 않는다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../docs/document-map.md)를 따른다.

This is the existing development/regression harness, not the final multi-host
large-model acceptance runner. Current work is owned by the
[roadmap](../../../docs/distributed-batching-roadmap.md) and
[verification contract](../../../docs/distributed-batching-verification.md).
It asks a meaningful Korean TypeScript question, but the keyword/format checks
in [`judge.mjs`](judge.mjs) are only one part of quality evidence. The final gate
requires varied normal prompts, full answers, strong overlapping waves and actual
model execution on multiple physical computers. Existing remote mode runs the
stage servers together on one remote host; SSH alone does not prove distribution.

```bash
node test/benchmarks/p4-4node/run.mjs smoke      # one request, whole path
node test/benchmarks/p4-4node/run.mjs service    # 40 requests, arrival waves
node test/benchmarks/p4-4node/run.mjs mixed      # continuous arrivals
node --test test/benchmarks/p4-4node/judge.test.mjs   # no GPU required
```

Artifacts land in `target/p4-4node/runs/<run-id>/`: `config.json` (the resolved
OUTER plan), `artifact.json` (per-request rows, timings, outcomes),
`report.json` (structural + meaning verdict and batch metrics), `gpu.csv`,
and the stage logs.

## Report metrics version 2

`report.json.metrics.metrics_version` distinguishes the current calculation
from archived version-1 reports. `generation_tps` now counts preserved approved
OUTPUTs, subtracting only one final empty EOS per request, rather than counting
decode evaluation rows. The first generated token and accepted speculative
tokens therefore count even when they do not correspond to a Decode row.

- `sampled_tokens` / `sampled_output_tps` include the empty terminal EOS.
- `generated_tokens` / `generation_tps` use the same empty-EOS exclusion as
  the Rust acceptance counter. Empty nonterminal, length and stop outputs are
  not removed. Nonempty EOS text is not removed either.
- `text_bearing_output_events` counts events with nonempty text, not visible
  tokens or a retokenized response.
- `legacy_row_rates.generation_tps` and `.total_tps` preserve the previous
  Decode rows/s and (Prefill+Decode) rows/s calculations. They exclude Verify
  and Replay, and are not generated-token throughput. The old top-level
  `total_tps` is no longer emitted. Consumers must check `metrics_version`.
- Mixed physical work means Prefill together with Decode, Verify or Replay.
  Physical row width, ubatch fill and pacing still use the whole observed
  physical batch, not this OUTER's output count or owned rows.

The denominator remains the current Rust drive's `elapsed_ms`, labelled
`drive_elapsed_through_release`; it includes the release wait. This is **not**
the verification contract's first scheduled submit→last terminal useful-TPS
window, and these rates do not approve answer quality. The existing per-request
Rust `logical_generation_tps` remains Decode rows / generation interval and has
not been migrated. If the drive's completion barrier changes, this denominator
description and the report version must be reviewed together.

`report-metrics.test.mjs` calls the same `buildReport` artifact consumer used by
the live runner. Its literals are model-free report tests, not model/worker/GPU
evidence. Span ownership, full telemetry completeness, cross-host clock proof,
RAM-offload layout validation and the final varied-corpus judge remain unfinished
work described by the roadmap. A successful report-unit test does not open an
offload or multi-host acceptance gate.

## Memory records

`node test/benchmarks/p4-4node/plan-lines.mjs <run-directory>` prints the log's
`MEMORY_PLAN` and `MEMORY_ACTUAL` records separately, including preceding RS/KV
allocation lines. Record numbers are log order, not stage identity. Compare a
stage's planned and actual model/context/compute sizes; a following stage's
PLAN is not the preceding stage's ACTUAL. Allocated memory reduces current
free memory, so an ACTUAL `fits_current_free=false` alone is not a load refusal.

## What the judge checks

| Check | Failure it catches |
| --- | --- |
| length, distinct domain terms | empty or off-topic output |
| hangul ratio | wrong-language output from a mis-sliced cut-set |
| repeat ratio over 12-char shingles | degenerate loops from stale KV |
| absence of U+FFFD | one detectable text-corruption symptom, not proof of UTF-8 or semantic correctness |
| terminal stop in {eos, length} | a request that never terminated |

## Layering

This directory is an OUTER implementation, not part of P4. It chooses
placement, capacity and the llama.cpp plan text, and it applies the gemma-4
chat template — the staged server tokenizes prompt text verbatim and owns no
template. P4 carries the plan as an opaque string.

## Four stages is a name, not a recommendation

Measured 2026-09-04, interleaved, non-overlapping: on two cards, one stage a
card beats two stages a card by **45% on a 35B** (176.31 against 121.27 total
rows/s) with 20 layers a card in both arms, and by **22% on gemma-4-E2B**
(639.66 against 522.87) with 13 and 22 layers a card in both. Rebalancing the
cards is a separate **19%** on top of that, which is why the first figure
quoted for the 2B - 42% against the shipped 9/26 placement - was two effects
in one number. Every 2B arm passed 192/192; the 35B arms passed structurally
and scored 55-64/64 on meaning, varying by run. Two
cards and these specific placements are the scope of that observation.
They do not impose a node-count limit: model/KV capacity and multi-host deployment
may require more nodes, including multiple nodes on a device. Keep topology fixed
when judging batching policy. `prefill_mix_2stage` and `prefill_mix_35b_2stage`
remain separate diagnostic placement arms, not a product topology recommendation.

The split below is fixed at 5/4/4/22 because gemma-4-E2B shares KV across
layers 13..34, so no stage boundary may fall inside that region. See
[docs/llamacpp-stage-memory.md](../../../docs/llamacpp-stage-memory.md).

## Larger models and RAM offload (2026-09-07)

Three scenarios extend the ladder past the 2B/35B VRAM-only arms. They are
development arms on the approved 3090x2 host, not the roadmap's final
multi-computer proof, and the harness still does not enforce H0~H7.

| Scenario | Model (from `S:\models`) | Placement | What it isolates |
| --- | --- | --- | --- |
| `vram_31b_2stage` | gemma-4-31B-it Q5_K_S, dense, 60 layers | 36/24 layers on two cards | a dense model that only fits by splitting |
| `offload_35b_moe_2stage` | Ornith-1.0-35B Q5_K_S (qwen35moe, 256 experts) | 20/20 layers, routed experts on CPU | expert offload against `prefill_mix_35b_2stage`, same cut |
| `offload_122b_moe_2stage` | Qwen3.5-122B-A10B Q5_K_S (75.9 GiB of experts) | 25/24 layers, experts on CPU | a model that only fits with RAM offload |

A one-stage baseline of the offload layout was tried and cannot run:
`p4-event-drive` refuses a config with fewer than two nodes. On 2026-09-07
the 122B two-stage arm loaded both stages (about 39 GiB of experts in host
RAM and 3 GiB on each card) and then the tail stage exited: its planned host
compute buffer did not equal the reserved one
(`stage_memory_plan.cpp`, "planned and actual memory differ at entry 1"), so
that arm has no throughput figure yet.

Two spec fields carry the offload into the plan. `mmap: true` drops
`--no-mmap`; the offload arms keep it `false`, because with the GGUF on the
S: SMB share the mmapped experts faulted in page by page during the first
prefill and the run stalled (2026-09-07, one stage execution in 30 minutes).
`--no-mmap` reads only the owned experts, sequentially, into host RAM at
load, and the offload arms allow 60 minutes for that. `overrideTensors` appends
`"<regex>=<buffer>"` patterns to the existing unowned-layer
`--override-tensor`; `EXPERTS_TO_CPU` keeps every `ffn_{up,down,gate}_exps`
tensor on CPU and computes it there, which is what llama.cpp's `--cpu-moe`
does. Routers, attention, norms, embeddings and the KV cache stay on the
GPU. Classification of a run as `vram_only` or `ram_offload` follows the
actual placement the stage logs report, not these option names.

The Ornith-1.0-35B GGUF declares a ChatML template with `<|im_end|>` as EOS
(read from `tokenizer.chat_template` and `tokenizer.ggml.eos_token_id` on
2026-09-07). `prefill_mix_35b_2stage` still sends it gemma-4 turns and gemma
stops, which is the arm every earlier 35B figure used; the offload arms use
`chatmlTurn` and `STOPS_CHATML`, so their meaning verdicts are not comparable
with that arm's 55-64/64 history. `gemma-4-31B-it` uses the same turn format
as the 2B.

A run that fails before UNLOAD leaves its stage servers registered in a
remote agent, and the next run then fails with `node already exists`. Restart
the agent with `remote-agent.mjs stop` and `start` between such runs.

## Requirements

Build the staged server for the compute capabilities actually present, not
the script default:

```bash
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --cuda --cuda-architectures '86;89'
```

The default `75;89` leaves an sm_86 card (RTX 3090) without native code, which
this harness measured as 19.9 -> 6.15 tok/s while still producing a meaningful
answer - the reason meaning and throughput are judged separately.

A CUDA build of the staged server at `target/p4-staged-cuda/`, release builds
of `p4-agent` and `p4-event-drive`, the model at the path in
[`scenarios.mjs`](scenarios.mjs), and two NVIDIA devices. Ports start at 42003,
below the Windows dynamic range (49152+), so an outbound ephemeral connection
cannot take the port from under the agent.

# Four-node acceptance harness

The acceptance bar for every implementation step in
[docs/adapter-restructure-plan.md](../../../docs/adapter-restructure-plan.md):
four stages must answer `타입스크립트에 대해 한국어로 설명하라` with a
meaningful Korean explanation. Delivering 40/40 responses is not acceptance —
[`judge.mjs`](judge.mjs) decides whether the answers mean anything, and the
run fails when they do not.

```bash
node test/benchmarks/p4-4node/run.mjs smoke      # one request, whole path
node test/benchmarks/p4-4node/run.mjs service    # 40 requests, arrival waves
node test/benchmarks/p4-4node/run.mjs mixed      # continuous arrivals
node --test test/benchmarks/p4-4node/judge.test.mjs   # no GPU required
```

Artifacts land in `target/p4-4node/<scenario>/`: `config.json` (the resolved
OUTER plan), `artifact.json` (per-request rows, timings, outcomes),
`report.json` (structural + meaning verdict and batch metrics), `gpu.csv`,
and the stage logs.

## What the judge checks

| Check | Failure it catches |
| --- | --- |
| length, distinct domain terms | empty or off-topic output |
| hangul ratio | wrong-language output from a mis-sliced cut-set |
| repeat ratio over 12-char shingles | degenerate loops from stale KV |
| absence of U+FFFD | a multi-byte token split across a stage boundary |
| terminal stop in {eos, length} | a request that never terminated |

## Layering

This directory is an OUTER implementation, not part of P4. It chooses
placement, capacity and the llama.cpp plan text, and it applies the gemma-4
chat template — the staged server tokenizes prompt text verbatim and owns no
template. P4 carries the plan as an opaque string.

The four-node split is fixed at 5/4/4/22 because gemma-4-E2B shares KV across
layers 13..34, so no stage boundary may fall inside that region. See
[docs/llamacpp-stage-memory.md](../../../docs/llamacpp-stage-memory.md).

## Requirements

Build the staged server for the compute capabilities actually present, not
the script default:

```bash
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --cuda --cuda-architectures 86;89
```

The default `75;89` leaves an sm_86 card (RTX 3090) without native code, which
this harness measured as 19.9 -> 6.15 tok/s while still producing a meaningful
answer - the reason meaning and throughput are judged separately.

A CUDA build of the staged server at `target/p4-staged-cuda/`, release builds
of `p4-agent` and `p4-event-drive`, the model at the path in
[`scenarios.mjs`](scenarios.mjs), and two NVIDIA devices. Port 52203 is used
because the default 52003 falls inside a Windows excluded port range on this
machine.

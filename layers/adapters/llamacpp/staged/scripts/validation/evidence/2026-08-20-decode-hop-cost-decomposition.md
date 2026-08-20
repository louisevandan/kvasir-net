# What a decode hop actually costs

Observed 2026-08-20 KST on this machine's two cards — RTX 4080 holding
layers `[0,14)` and RTX 3090 holding `[14,28)` — with
`Qwen2.5-1.5B-Instruct-Q8_0.gguf`, 64 requests at parallel 64, a 32-token
prompt and 512 generated tokens so that generation dominates the run.

Every run below completed 64 of 64 with all four verdicts and no failure.

## Why measure this at all

[2026-08-20-batched-decode-throughput.md](2026-08-20-batched-decode-throughput.md)
ends by observing that the mean hop width is about a sixth of the declared
concurrency and guessing that whatever gathers laps is where the rest of the
throughput is. An attempt to gather them — holding a narrow decode window open
for a quarter of the node's own last hop — nearly doubled the mean width, from
10.89 to 20.48, and **cost 41% of the throughput**. Widening the hop was not
the lever. So the hop itself was instrumented instead of guessed at.

## The phases of a hop

`P4_STAGED_TRACE_HOP=1` now reports, per batched decode, the time in
`llama_decode`, in the wait for its outputs, in binding and splitting the
cut-set, and in sampling. Nothing is left over: the residual is 0.05 ms on one
stage and 0.09 ms on the other.

| | stage 0 — `[0,14)` | tail — `[14,28)` |
| --- | ---: | ---: |
| `llama_decode` submit | 11.25 ms | 12.57 ms |
| wait for outputs | 0.01 ms | 9.50 ms |
| bind the cut-set | 0.00 ms | 0.03 ms |
| split the cut-set | 4.45 ms | 0.00 ms |
| sample | 0.00 ms | **21.40 ms** |
| **hop** | **15.76 ms** | **43.58 ms** |

A lap is one hop of each, so the ring turns in about 59 ms and carries about 26
tokens — which is the 450 to 700 tokens per second the driver reports.

## A hop is nearly free in its width

The same trace, bucketed by how many sequences the hop carried:

| rows | stage 0 submit | tail submit |
| ---: | ---: | ---: |
| 2 | 8.55 ms | 10.74 ms |
| 16 | 9.51 ms | 12.58 ms |
| 27 | 11.77 ms | 12.78 ms |
| 40 | 12.09 ms | 12.86 ms |
| 62 | 13.57 ms | 13.23 ms |

Thirty-one times the work costs between 1.2 and 1.6 times the time: the
marginal cost of one more sequence is 0.04 to 0.08 ms. **Batching a lap already
works.** What is expensive is having a lap at all.

## What the fixed cost is made of

Running the same model split 7/21 instead of 14/14 gives a second point on the
same line:

| layers in the stage | submit |
| ---: | ---: |
| 7 | 7.98 ms |
| 14 | 11.25 / 12.57 ms |
| 21 | 17.40 ms |

which is about **3 to 5 ms per call plus 0.5 to 0.7 ms per layer**. A layer of
this model is about 53 MiB of Q8 weights, so a card reading them at its rated
bandwidth would spend 0.06 to 0.08 ms — the layer term is an order of magnitude
above what the memory can explain, which is what a graph relaunched kernel by
kernel looks like.

## The CUDA graph is never armed, and arming it does not pay

`ggml-cuda.cu` resets its warmup whenever a node property changes and needs two
consecutive identical calls to arm again, and `llm_graph_params::allow_reuse`
requires the same `n_tokens`, `n_seqs_unq` and `n_outputs`, with
`can_reuse_kq_mask` adding the same `n_kv`. A lap whose width is whatever
happened to arrive matches the lap before it 4 times in 1,010.

So the width was pinned: padding rows on a reserved sequence slot, emptied
again as soon as the lap was decoded, so that every submission has one shape.
It worked as designed and did not help.

| | reuse / builds, tail | tail submit | tail hop | aggregate, untraced |
| --- | ---: | ---: | ---: | ---: |
| no padding | 83 / 2,156 | 12.57 ms | 43.58 ms | **712.9 tok/s** |
| padded to a power of two | 280 / 2,024 | 9.40 ms | 42.55 ms | — |
| padded to the slot budget | 113 / 2,094 | **7.07 ms** | 44.00 ms | **671.0 tok/s** |

Submit falls by 44% and the hop does not move: `llama_decode` is asynchronous,
so the time leaves the submit and reappears in the wait. The arithmetic was
never the thing being paid for, and making the graph reusable does not change
how much arithmetic there is. The change was reverted; only the instrumentation
that found this is kept.

## Where the tail's time is

Splitting the tail's sampling further, per row:

| | per row |
| --- | ---: |
| `common_sampler_sample` | 0.396 ms |
| `common_detokenize` of the whole history | 0.024 ms |

Detokenising the entire generated text on every token is quadratic in the
length of the answer and it does not matter: at 512 tokens it is 6% of the
per-row cost, and the per-row cost is flat across the run (0.803 ms at the
start, 0.855 ms at the end). The cost is the sampler chain over a
151,936-entry vocabulary, and it runs one row after another on one thread
while 47 other cores and the card both wait.

That is the largest single item left — about a third of the ring — and it is
the adapter's, not P4's. Parallelising it is not free to do safely:
`common_sampler_sample` reaches the logits through `llama_get_logits_ith`,
which calls `output_reorder()`, which clears `output_swaps` on every call. The
rows are independent and each has its own sampler, so the work parallelises;
the accessor does not. llama.cpp also offers backend sampler chains
(`llama_context_params::samplers`), which would move the chain onto the card
and hand the host a short candidate list instead of the whole vocabulary —
marked experimental upstream, and incompatible with grammar and with the
reasoning budget.

## Batching a lap is no longer behind a switch

It was, because llama.cpp splits a batch into ubatches of its own choosing and
the staged cut-set is bound once per decode, so a split hands the graph a
narrower input than the lap it was given. That is not a risk to be accepted or
hoped away — it is a precondition the stage can check about itself:

* `llama_kv_cache` picks `split_simple` when there is a single stream, which
  is what a unified cache means, and
* `split_simple` takes consecutive tokens until `n_ubatch` and stops.

So a lap that fits one ubatch on a unified cache cannot be split, and both
halves are the stage's own configuration. `execute_decode_batch` now refuses
the batch when either fails, and a refusal computes nothing, so the
per-sequence path runs the same lap unchanged.

Running the harness with `--kv-unified` removed exercises exactly that refusal:

| | aggregate |
| --- | ---: |
| per-sequence, the refusal path | **69.2 tok/s** |
| batched, the default path | **694 to 713 tok/s** |

Both completed every request with all four verdicts. The refusal is correct and
the difference is an order of magnitude, which is why this belongs on by
default rather than behind an environment variable.

## Reproduction

```powershell
apps\p4\tools\scripts\e2e\run-local-real-two-stage.ps1 `
  -Model 'S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf' `
  -PromptFile 'apps\p4\tools\scripts\e2e\fixtures\prompt-tiny.txt' `
  -Requests 64 -Tokens 512 -PromptTokens 32 -Parallel 64 `
  -LayerBoundary 14 -LayerCount 28 -BatchSize 2048 -UBatchSize 512 `
  -MaxSecondaryVramMiB 13000 -ArriveMilliseconds 0 -VaryPrompts
```

with `P4_STAGED_DECODE_BATCH=1`, and `P4_STAGED_TRACE_HOP=1` for the phase
lines. The trace writes a line per hop and a line per sampled row, which costs
about 30% of the throughput — take aggregate numbers with it off.

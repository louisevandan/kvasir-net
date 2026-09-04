# The small model was distorting it: a 35B, and a scheduler that threw work away

Host M42-SERVER2, two RTX 3090. `Ornith-1.0-35B-UD-Q5_K_S` (qwen35moe, 40
layers, 23.2 GiB) cut 10/10/10/10 over four stages, two stages a card, flash
attention on, K and V cache `q8_0`, `--batch-size 512 --ubatch-size 512`.
Every number is copied from the run's own `report.json`, `artifact.json` and
`gpu.csv`; run ids are given so they can be re-read.

## What the 2B model was hiding

The parallel-sampler work rested on a ratio measured on gemma-4-E2B: 0.29 ms a
row to choose a token against 0.11 ms a row to run the stage's 22 layers, so
the sampler cost more than the transformer. **That is a property of a 2B model
with a 249k vocabulary, not of the pipeline.** On the 35B, binned by width:

| width | decode ms/row | sample ms/row |
| ---: | ---: | ---: |
| 5.1 | 3.967 | 2.477 |
| 12.0 | 3.219 | 2.163 |
| 22.1 | 2.155 | 1.725 |

The layers cost 1.25 to 1.6 times the sampler here - the ratio inverts. The
sampler is still 40-45% of the tail's step, so parallelising it is not
pointless, but **the +10% it measured belongs to gemma-4-E2B and must be
quoted with that condition**.

## A decode row was cutting every prompt to one row

The 35B reports `equal_sequence_ubatch=1` on all four stages: its memory makes
llama.cpp split by equal per-sequence width. The scheduler read that as "if any
ready demand is a decode, the common width is one" - and a decode has exactly
one row to give, so a prompt with a thousand rows ready went one row at a time.

Measured before the fix (run 050325Z-2c302415):

| | |
| --- | ---: |
| token rows ready at plan time, mean | **934.8** |
| rows actually issued, mean | **9.85** |
| physical batches | 5,717 |

The fix decides the participants before the width: if any decode is ready the
batch is the decodes, and the prompts take the next batch, where they share a
wide equal UBATCH. Both keep moving because the round-robin cursor advances
either way.

| | before | after |
| --- | ---: | ---: |
| total rows/s | 90.13 | **127.77** |
| gen tok/s | 61.33 | **86.22** |
| mean batch width | 9.85 | **21.83** |
| prefill batch width | - | **367 mean, 512 max (the cap)** |
| rows left behind, mean | **934.8** | **32.1** |
| physical batches | 5,717 | 2,563 |

Three runs of the fixed scheduler gave 86.02, 86.22 and 90.75 gen tok/s and
126.9, 127.77 and 134.72 total rows/s - reproducible within a few percent.

**And again the two quantities this record once chased moved the other way:**
concurrent stage occupancy fell 87.9% -> 47.7% and GPU utilisation 20% -> 15%,
while throughput rose 38%. That is the third independent time.

The scheduler contract test that encoded the old behaviour was rewritten rather
than deleted, and carries the measurement that changed it.

## A data race, defaulted off

The parallel sampler gives every row its own sampler but every worker calls
`common_sampler_sample(ctx_, ...)` on one context. Upstream enters that through
`llama_synchronize()`, which updates `t_eval_us`, `n_eval` and
`n_queued_tokens` unlocked, and `get_logits_ith()`, which calls
`output_reorder()` and swaps rows of `logits.data` in place - the buffer the
answer is read from. Eight runs passing their judge is not evidence that no
race occurred. The default is back to one thread;
`P4_STAGED_SAMPLE_THREADS=N` remains for the experiment that has to come
first: synchronise once on one thread, copy the logits, then let independent
samplers run, with a fixed seed, fixed batch membership and a thread sanitiser.

## Reasoning budget: it was already there, twice

The pinned llama.cpp has `--reasoning-budget`, `--reasoning-budget-message` and
`enable_thinking`, and the budget is a *sampler* parameter -
`common_params_sampling::reasoning_budget_tokens`, consumed by
`common_reasoning_budget_init` in the sampler chain - so it acts on logits.
The adapter already accepted it per request as flat option keys
(`reasoning_budget_tokens`, `_start_tag`, `_end_tags`, `_message`) and
advertised them in HELLO. A nested `reasoning` object was written before
checking that; the option list is an allowlist, so the request was refused
rather than silently ignored, and the duplicate was removed.

It works: with `reasoning_budget_tokens: 0` every answer opened
`<think></think>` and closed it immediately, 64 of 64. **But it is not enough
on its own** - the model reopens a block, the sampler forces it closed again,
and four of sixty-four answers became `<think></think>` repeated to the token
limit. What `enable_thinking: false` does in a chat template is put the closed
block in the *prompt* so none is ever opened, and that belongs to OUTER, which
owns the template; P4 carries the prompt verbatim. Prompt suppression took the
judge from 47/64 to 62/64, and removing the budget sampler on top changed
nothing (62/64), so the sampler was not the cause of the loop either.

## Three harness faults, none of them the pipeline

The 35B scenario failed its judge for reasons that were all the harness:

- **Token budget.** A reasoning model spends its first hundreds of tokens
  thinking; at 200 a quarter of the runs never reached the answer. 47/64.
- **Stop reasons.** Scenario stop strings make requests end on `stop`, which
  neither the drive's acceptance nor `judge.mjs` allowed. The drive exited 1
  on a run that had completed 64/64.
- **A degenerate fixture, mine.** The background sentences numbered sixteen and
  were cycled to fill a 56-line prompt, so the model spent its budget
  observing in English that "many are duplicated (1-16, 17-32, 33-48, 49-56)"
  and never answered in Korean. Fifty-six distinct sentences now.

## Not a baseline

The best 35B run is 90.75 gen / 134.72 total rows/s at 62/64 meaningful. It is
**not promoted**: two answers still fail because this model reasons in English
over a long prompt, so the scenario is not yet an acceptance gate. What is
solid is the *delta*: rows-left-behind 934.8 -> 32.1 and prefill width 9.85 ->
367 are structural counts, not timings, and they reproduce across three runs.

## Still open

Two stages share each card, so there are two independent execution lanes and
four sets of per-batch fixed cost. The report's stage spans cover the whole
stage RPC - CUDA wait, sampling, serialisation - so they are service time, not
device time, and are renamed `service_pct` / `two_or_more_open_pct` /
`stages_open_peak` to stop them reading as GPU concurrency. Whether one stage
a card beats two is unmeasured, and so is the comparison against stock
llama.cpp on a single card.

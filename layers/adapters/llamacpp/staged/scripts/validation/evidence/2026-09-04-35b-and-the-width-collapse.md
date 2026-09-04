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

## Four stages on two cards is over-partitioning

Two stages share each card, so there are two independent execution lanes and
four sets of per-batch fixed cost. The stage spans cannot settle that - they
cover whole stage RPCs, so four read as open at once on two devices, and they
are renamed `service_pct` / `two_or_more_open_pct` / `stages_open_peak` for
that reason. Running the same work at the depth the cards actually provide
does settle it. Same model, same 40 layers, same context, arrivals and
prompts; only the partition changes: `[0,20),[20,40)` on one card each,
against `[0,10),[10,20),[20,30),[30,40)` at two stages a card.

Four interleaved runs:

| | one stage a card | two stages a card |
| --- | ---: | ---: |
| total rows/s | 173.79, 176.21 | 127.38, 136.54 |
| mean | **175.00** | 131.96 |
| gen tok/s | **117.58** | 88.88 |
| wall (s) | 317.6, 321.4 | 439.9, 412.8 |
| stage ms, per lap | 81 + 89 = **170** | 56+61+62+69 = **248** |
| GPU utilisation, summed | 32.9, 33.2 | 32.8, 31.3 |

**+32.6% for halving the number of stages**, with no overlap between the arms
and both passing 64/64 structurally - and 58 to 60 of 64 on the meaning
judge in all four arms, which is why this scenario is still not an
acceptance gate. The same forty layers cost 78 to 88 ms
more per lap when they are cut into four - two extra stage crossings at about
44 ms each, which is the per-batch fixed cost measured earlier from the width
fit. And GPU utilisation is identical at ~33% in both arms while throughput
differs by a third: the fourth independent time that number has failed to
track the work done.

So the partition should follow the hardware, not a node count: **stages are
worth having up to the number of independent execution lanes, and past that
each one adds a full set of per-batch fixed cost for lanes that cannot
overlap.** Two processes on one card do not pipeline; they contend.

The four-node split in this harness exists because gemma-4-E2B shares KV over
layers 13..34 and no boundary may fall inside that region - a model
constraint, never a measurement that four stages were better.

## And the same on the 2B, larger

gemma-4-E2B allows exactly one two-way cut, `[0,13)` and `[13,35)`, because of
the same KV region that forces the four-node split. Four interleaved runs of
`prefill_mix`, all four passing 192/192 on structure and meaning:

| | one stage a card | two stages a card |
| --- | ---: | ---: |
| total rows/s | 664.33, 585.63 | 446.43, 431.33 |
| mean | **624.98** | 438.88 |
| gen tok/s | **251.51** | 176.62 |
| stage ms, per lap | 200, 240 | 329, 389 |
| GPU utilisation, summed | 48.9, 45.0 | 41.0, 38.3 |

**+42.4%** - but not of the boundary count alone. The four-stage cut leaves 9
layers on one card and 26 on the other; the two-stage cut leaves 13 and 22, so
removing two crossings and rebalancing the cards were measured together, and
the first two-stage run came from a different working tree besides.

### Separating the two

`prefill_mix_tail_alone` already was the control and had been built for a
different question: it keeps four stages but places them 0,0,0,1, which puts
13 layers on one card and 22 on the other - exactly the two-stage load. Same
concurrency, context, prompts and arrivals; only the boundary count differs.
Four interleaved runs, every tree clean, all four 192/192 on structure and
meaning:

| | boundaries | layers per card | total rows/s |
| --- | ---: | --- | ---: |
| two stages | 2 | 13/22 | 653.85, 625.47 -> **639.66** |
| four stages, rebalanced | 4 | 13/22 | 490.26, 555.48 -> **522.87** |
| four stages, as shipped | 4 | 9/26 | **438.88** |

So the 42.4% is two effects of similar size:

- **removing two boundaries: +22.3%** (639.66 against 522.87, load held equal)
- **rebalancing the cards: +19.2%** (522.87 against 438.88, boundaries held equal)

Reporting them as one number was wrong, and only about half of it was the
partition. The 35B pair remains the clean measurement of the boundary effect
on its own - 20 layers a card in both arms - and it is larger there, +45.4%
from the committed source.

Both models therefore say the same
thing, and the 2B says it louder: the harness has been paying for two extra
stage crossings a lap since it was written, 129 to 149 ms of them here.

The rule to carry forward: **partition to the number of independent execution
lanes, then stop.** A second process on the same card is not a second lane -
it adds a full set of per-batch fixed cost and contends for the device it
already had. The four-node harness is named for a model constraint, and the
count was never measured until now.

## Still open

The comparison against stock llama.cpp on a single card is not run. The 35B
does not fit on one 3090 with usable context, so that baseline needs the small
model, and it answers a different question from this one: what the adapter and
the boundary cost, rather than what the partition costs.

## Re-run from the committed source

The four A/B runs above ran from a working tree that carried the harness
changes they were measuring. Repeated at `1d9185a41`, with the deployed
binaries hash-checked against the local build before the block started:

| | one stage a card | two stages a card |
| --- | ---: | ---: |
| total rows/s | 177.02, 175.60 | 119.36, 123.18 |
| mean | **176.31** | 121.27 |
| gen tok/s | 118.54, 117.41 | 80.14, 82.73 |
| layers per card | 20/20 | 20/20 |
| structural | 64/64 in all four | |
| meaning | 58/64, 55/64 | 59/64, **64/64** |

**+45.4%**, larger than the 32.6% measured before, with the same 20 layers a
card in both arms. The partition result holds and strengthens.

Two things belong in the record rather than out of it. Only the first run has
a clean tree: the other three were taken while source for the next commit was
being edited, which is a discipline failure of mine - the binaries were fixed
and hash-verified before the block, so the numbers are attributable to this
commit's build, but only one run meets strict clean-commit acceptance. And one
four-stage run scored 64/64 on meaning where its sibling scored 59/64, so the
meaning judge varies run to run rather than by configuration; it is not a
property of the partition.

# 2026-09-03: what the four-node pipeline does under load, and what it does not

> Document status (2026-09-06): **evidence limited to its date and environment**. It records observations for the date, commit, model and topology in the body. It is not completion evidence for the current implementation or for other distributed environments.
> For current goals, status and order see the [execution roadmap](../../../../../../../docs/distributed-batching-roadmap.md); for document authority and reading paths see the [document map](../../../../../../../docs/document-map.md).

Host M42-SERVER2, two RTX 3090, gemma-4-E2B-it Q8_0 split 5/4/4/22, K `q8_0`
V `f16`, flash attention off, `--batch-size 512 --ubatch-size 512`. Every
number below is copied from `target/p4-4node/runs/<run>/report.json` and
`artifact.json`; the run id is given so it can be re-read.

## The acceptance scenarios were not load

| run | scenario | `min_batch_rows` | batches | mean width | mixed | fill | gen tok/s | GPU0/GPU1 util |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 050210Z-245f6c6f | mixed | 0 | 3,046 | 4.18 | 2 | 0.82% | 93.43 | 12.5% / 21.3% |
| 064227Z-7d4969c7 | mixed | 4 | 1,187 | 10.72 | 19 | 2.09% | 97.00 | 16.9% / 17.0% |
| 065621Z-c03935d8 | mixed | 24 | 1,183 | 10.75 | 19 | 2.10% | 97.35 | 15.5% / 16.9% |
| 050649Z-d022f10a | service | 0 | 3,000 | 13.57 | 0 | 2.65% | 166.31 | 15.7% / 26.1% |
| 070038Z-4d508847 | service | 8 | 3,000 | 13.57 | 0 | 2.65% | 176.93 | 16.9% / 24.4% |
| 070612Z-a4b59d94 | service | 40 | 1,333 | 30.55 | 2 | 5.97% | 122.69 | 21.2% / 19.7% |

Three things this table settles.

Arrival coalescing was switched off in every run before this day: the
harness default is `--min-batch-rows 0` and the gate needs `> 1`. Turning it
to 4 takes `mixed` from 2 to 19 mixed batches and 4.18 to 10.72 rows without
a throughput loss; 8, 16 and 24 are identical to 4, so the axis saturates
there and the ceiling is the scenario's own concurrency (about 16 active).

The two `service` runs at 0 and 8 have byte-identical batch structure
(`10x1998, 20x999`) and differ by 6.4% in throughput. That is the run-to-run
noise floor, and it means the +3.8% seen on `mixed` at threshold 4 is inside
it. What is outside it are the structural counts.

Forcing width by waiting (threshold 40) does merge the waves - 666 batches of
exactly 40 - but costs 26% of throughput and moves GPU utilisation by under
one point. Width is not what the GPU is waiting for.

## Load

| run | scenario | seqs | arrivals | batches | mean width | max | fill | gen tok/s | total rows/s | GPU0/GPU1 |
| --- | --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 075059Z-0b146051 | pressure_128 | 128 | 16/s x16 | 1,306 | 42.73 | 320 | 8.35% | 380.26 | 416.56 | 16.2% / 20.9% |
| 081127Z-edbd05a6 | pressure | 256 | 32/s x16 | 1,331 | 83.86 | 512 | 16.38% | 397.77 | - | 17.3% / 27.2% |
| 154752Z-a84461ce | prefill_mix | 96 | 12/s x16, prompts 19/202/564/1290 rows | 1,348 | 69.9 | 512 | 13.65% | 212.09 | 523.03 | 18.2% / 25.7% |

Concurrency is what moves throughput: 166 to 436 tok/s from 40 to 256
sequences, and the batch reaches the 512-row cap. Utilisation does not follow
it. Per-session rate falls from 5.23 to 1.79 tok/s across the same range.

`prefill_mix` is the batching strategy's actual test - four prompt sizes
spanning 68x, arriving while earlier requests decode. 100 of 1,348 physical
batches carried prefill and decode together (the old `mixed` scenario managed
2 of 3,046), all 192 answers passed the judge, and the gate never fired.

## What the first node was doing

The `BatchObservation` now carries the first node's own stage time, its idle
time before planning, how often the gate refused it, and the token rows the
ready set held. From 154752Z-a84461ce:

| | mean | p50 | p90 | max |
| --- | ---: | ---: | ---: | ---: |
| `stage_ms` (node 0's own five layers) | 63.7 | 54 | 118 | 556 |
| `idle_ms` | 68.6 | 18 | 200 | 798 |
| `ready_rows_left` | 128.6 | 0 | 0 | 4,080 |

Gate refusals: 0. In 1,247 of 1,348 plans nothing was left behind; in the
101 that left something, 82 left 512 rows or more - a prefill wave larger
than `--batch-size`, which is the cap and not the scheduler.

**An earlier reading of this field was wrong.** The first version subtracted
the batch's rows from `ready_row_count()`, which counts eligible *requests*,
and reported the never-positive result as "the scheduler leaves nothing".
That was an identity, not a measurement. `available_row_count()` now counts
token rows, and the table above is from that.

Five layers at 54 to 64 ms is the number that matters: scaled to 35 layers it
is roughly 540 ms, and the measured per-session lap under load is 559 ms. The
lap is stage time. Width, coalescing and arrival shape are no longer where the
throughput is.

## Every stage timed (run 161030Z-60fa5980, `prefill_mix`, gate off)

Each node now reports `[ingress, start, end, forward]` per batch on the host
wall clock, keyed by the batch's execution ids, so the four stages can be laid
side by side. 1,159 batches, all four spans present for every one. 192/192
passed; 190.85 gen tok/s, 470.65 total rows/s; GPU 17.4% / 25.8%.

| node | layers | busy | stage ms mean / p50 / p90 | ms per layer |
| ---: | ---: | ---: | --- | ---: |
| 0 | 5 | 44.4% | 76.6 / 68 / 134 | 15.3 |
| 1 | 4 | 37.3% | 64.4 / 48 / 121 | 16.1 |
| 2 | 4 | 36.5% | 63.0 / 47 / 101 | 15.8 |
| 3 | 22 | **70.2%** | 121.2 / 94 / 185 | 5.5 |

**The pipeline does overlap.** Two or more stages were inside their stage
server at the same instant for 68.5% of the run, all four at once at the peak,
and 2.96 executions were open on average (peak 8). The wavefront the earlier
record could only infer is traced.

**Stage time is mostly not layers.** A least-squares fit of mean stage time
against layer count gives **54.7 ms per batch fixed + 3.04 ms per layer**: the
22-layer tail costs 121 ms and a 4-layer stage 63 ms. Four stages pay the fixed
part four times a lap - about 219 ms of the 325 ms a lap spends computing.

**The lap is 510 ms mean (p50 376), decomposed:** 325 ms in stage servers,
185 ms between them. Of the between-stage time, the hop into the tail is
129.8 ms mean against 18.5 and 29.9 ms for the other two hops. That is not
transfer: when a batch left node 2 while node 3 was idle the hop took 2 ms
(p50, n=427); when node 3 was busy it took 128 ms (p50, n=732), the tail's
residual service (97 ms p50) plus up to five batches already queued. 63% of
batches found the tail busy. `queue_ms` at the node reads zero because the
wait happens before the worker stamps ingress - it is the mailbox, and it
shows up as the hop.

So the bottleneck is the tail stage's per-batch cost, most of which is fixed,
and the batches reach it one at a time. Width was never the lever; batch
*count at the tail* is.

## A tail-aware issue gate, and why it is not demonstrated

The spans said the lap sits in the tail's mailbox, so the first node gained
`P4_STAGED_MAX_OPEN_BATCHES`: hold the plan while that many batches are
already in the pipeline, so rows that would queue at the tail merge at the
head instead. Unlike a row threshold it never waits for width - when the
pipeline has room the plan goes at once however thin.

Swept on `prefill_mix` in the order 0, 2, 3, 4 it looked decisive: 190.9,
204.9, 209.1, 231.6 gen tok/s, monotone, +21% at the top. **That was run
order.** Re-run interleaved (4, 0, 4, 0, 8) the ordering vanishes:

| setting | n | mean gen tok/s | samples |
| --- | ---: | ---: | --- |
| 0 (off) | 4 | 196.7 | 190.8, 170.8, 216.9, 208.1 |
| 2 | 1 | 204.9 | |
| 3 | 1 | 209.1 | |
| 4 | 3 | 216.2 | 231.6, 208.1, 208.9 |
| 8 | 1 | 193.9 | |

The control's own range is 170.8 to 216.9 - 27% - and its best beats both
later samples of the setting that had looked best. The gate is not shown to
help or hurt. It stays, defaulted off, because it is the only knob that
bounds pipeline depth and one result from it survives: capping depth at 2
(from a natural 2.5-3, peak 8) cost nothing, 204.9 against a control mean of
196.7. Depth is not what the throughput is made of.

## What throughput is made of

Correlation of gen tok/s against every pipeline metric, over those ten runs:

| metric | r |
| --- | ---: |
| **share of run with two or more stages computing** | **+0.923** |
| tail node busy share | +0.684 |
| physical batches | +0.213 |
| open-execution depth | +0.121 |
| **batch width** | **-0.211** |

Width is mildly *negative*. Every earlier result now reads the same way: six
times the width moved utilisation by nothing, cutting depth to a quarter cost
nothing, and the loosest gate was the worst run. The quantity to maximise is
concurrent stage occupancy, and no policy tried so far moves it much - it
ranged 53.6% to 83.8% across runs of identical configuration.

That points at the stage balance rather than the schedule. At 54.7 ms fixed
plus 3.04 ms per layer the 22-layer tail costs 121 ms against 63 for a
4-layer stage, and under the default placement that tail shares GPU1 with
stage 2 - the two contend exactly when they should overlap. The cut cannot
move (gemma-4-E2B shares KV across layers 13..34) but the placement can, so
`prefill_mix_tail_alone` puts the three light stages on one card and the tail
on the other.

## Giving the tail its own card

Eight runs, interleaved and with the block order reversed halfway so a
session trend cannot masquerade as a result. Same binary, gate off.

| | tail alone (n=4) | default (n=4) | difference |
| --- | ---: | ---: | ---: |
| gen tok/s | 212.84 +/- 8.07 | 207.23 +/- 12.15 | +2.7% |
| total rows/s | 524.89 +/- 19.91 | 511.05 +/- 29.97 | +2.7% |
| tail busy | 85.92 +/- 4.86 | 77.88 +/- 6.43 | +10.3% |
| tail stage ms | 95.5 +/- 10.92 | 109.45 +/- 11.14 | **-12.7%** |
| batch width | 58.68 +/- 9.46 | 72.2 +/- 10.12 | -18.7% |

tail alone: 204.75, 212.46, 223.93, 210.22. default: 199.63, 198.27, 224.67,
206.36.

**The placement makes the tail faster and busier and does not demonstrably
make the pipeline faster.** Tail stage time falls 12.7% and tail occupancy
rises 10.3% in every pairing, but the throughput distributions overlap - the
best default run (224.67) beats three of the four tail-alone runs. On the
first two pairs it read as +4.8%; four pairs put it at +2.7% inside the
spread. Not shown.

Batch width fell 19% while throughput did not fall, which is the third
independent time width has moved one way and throughput the other.

## The session drifts, and that is why the first sweep lied

Across all eighteen `prefill_mix` runs of the day, run order correlates with
tail busy share at r=0.696 and with batch width at r=-0.700: later runs have
a busier tail and narrower batches whatever policy they were testing. Any
comparison run in sequence inherits that slope, which is exactly the +21% the
first gate sweep reported and the interleaved re-run erased. Every policy
claim from here needs interleaving; a sequential sweep is not evidence.

## The one durable finding

Over those eighteen runs - four issue policies, two placements, throughput
from 170.8 to 231.6 tok/s:

| metric | r with gen tok/s |
| --- | ---: |
| **share of run with two or more stages computing** | **+0.891** |
| tail busy share | +0.667 |
| first-node busy share | +0.521 |
| physical batches | +0.347 |
| open-execution depth | +0.330 |
| **batch width** | **-0.357** |
| **UBATCH fill** | **-0.357** |

Concurrent stage occupancy explains the throughput; width and fill are
mildly against it. That holds across policies rather than being produced by
one, which is what makes it worth building on. What is not yet known is what
*sets* that occupancy: neither issue policy nor placement moved it reliably,
and it varied 53.6% to 85.3% between runs of identical configuration.

**Everything in the paragraph above is backwards. See the next section.**

## The correlation was reverse causation (2026-09-04)

In all eighteen runs above, batch width was an *output*: the first node
plans from whatever is ready, so a run that happens to be going fast drains
its ready set faster and forms narrower batches. Reading width as a cause
and concurrent occupancy as the lever inverted both.

`P4_STAGED_MAX_ISSUE_ROWS` caps the rows one issued batch may carry, which
makes width an input. Eight interleaved runs of `prefill_mix`, caps 0, 24,
0, 24, 0, 12, 0, 48:

| cap | total rows/s | width | two-or-more busy | GPU0/GPU1 | mixed batches |
| ---: | ---: | ---: | ---: | --- | ---: |
| 0 | 544.25 | 79.7 | 81.1% | 18.6/27.0 | 98 |
| 24 | **290.88** | 19.8 | 89.0% | 23.8/29.1 | 2,490 |
| 0 | 494.07 | 77.0 | 74.5% | 18.3/25.0 | 100 |
| 24 | **277.37** | 16.2 | 94.6% | 25.7/31.0 | 2,508 |
| 0 | 545.54 | 57.4 | 84.2% | 20.2/30.4 | 102 |
| 12 | **198.10** | 8.2 | 95.4% | 26.7/32.1 | 3,698 |
| 0 | 473.74 | 97.7 | 67.3% | 17.3/26.0 | 92 |
| 48 | **253.55** | 11.3 | 93.7% | 26.4/39.7 | 1,057 |

Every metric this record had been treating as the goal improved, and
throughput fell by half. Concurrent occupancy reached 95.4%, GPU utilisation
its highest ever measured, mixed batches 3,698 against 98 - while total row
throughput went from 544 to 198. With width controlled the correlations
invert: width **+0.898**, concurrent occupancy **-0.060**.

The cause is a per-batch cost that narrow batches pay over and over. Fitting
the tail's step time against width across these eight runs:

**tail step = 34.2 ms per batch + 1.051 ms per row**

| width | tail step | rows per ms |
| ---: | ---: | ---: |
| 8.2 | 39.3 ms | 0.208 |
| 16.2 | 55.2 ms | 0.293 |
| 19.8 | 60.4 ms | 0.327 |
| 57.4 | 91.4 ms | 0.628 |
| 79.7 | 115.3 ms | 0.691 |
| 97.7 | 138.7 ms | 0.705 |

A wide batch is 3.4x more efficient per row than a narrow one, and the
curve is still climbing at width 98 - it has not reached the point where
the 1.05 ms per row dominates the 34 ms per batch. Busy stages were busy
paying that fixed cost repeatedly, which is why occupancy rose while work
fell.

So the lever is the 34 ms, twice over: amortise it with width, or remove
it. Width is bounded by what has arrived - the earlier coalescing
experiments show waiting for it costs depth - so the 34 ms itself is the
target, and it has not been decomposed. `stage_ms` covers frame receive,
`llama_decode`, cut-set extraction from the device and the response, and
nothing measured so far says which of the four it is.

The order of these three sections is the record of the mistake: a
correlation over runs where the suspected cause was actually an effect, a
conclusion drawn from it, and the experiment that inverted both signs. The
first two are left standing rather than rewritten.

## Where the per-batch cost actually is (2026-09-04)

`P4_STAGED_TRACE_STEP` times a stage step in four parts on the stderr the
harness already collects. One run of `prefill_mix`, 6,716 steps, widths 2 to
512:

| part | first node | middle | tail |
| --- | ---: | ---: | ---: |
| frame parse | 0.04 ms | 0.54 ms | 0.54 ms |
| `llama_decode` | 49.2 | 37.8 | 40.1 |
| owner matching | 0.07 | - | - |
| sampling | - | 0 | **46.9** |
| response encode | 2.13 | 1.18 | 1.18 |

Parsing, owner matching and encoding are together under 3 ms and are not the
34 ms. The cost is `llama_decode` and, on the tail, sampling - which is the
larger of the two. Binned by width, the tail costs 0.11 ms per row to run its
22 layers and **0.29 ms per row to choose the token**: the sampler is 2.7x
the transformer. The vocabulary is at least 249,157 (largest token id
observed in the run's own outcomes) and `common_sampler_sample` builds a
candidate array that size per row, one row after another on one thread.

The 2026-08-20 record called this "about a third of the ring, and the
adapter's problem rather than P4's". It is now half.

## Sampling the rows in parallel

Each output row has its own sampler keyed by sequence, so the loop
parallelises. The pass refuses rather than assumes: Verify and Replay rows go
through the MTP path and stay sequential, a repeated sequence key inside one
batch abandons the parallel pass because one row's `accept` is the next row's
state, and fewer than eight output rows stays serial. Threads default to a
quarter of the host's cores because four stage servers share the machine.
`P4_STAGED_SAMPLE_THREADS=1` restores the previous behaviour exactly and is
the control.

Eight runs, interleaved, block order reversed halfway:

| | parallel (n=4) | serial (n=4) | |
| --- | ---: | ---: | ---: |
| gen tok/s | 213.45 +/- 8.09 | 194.06 +/- 6.54 | **+10.0%** |
| total rows/s | 526.39 +/- 19.95 | 478.56 +/- 16.12 | +10.0% |
| per-session tok/s | 2.46 +/- 0.12 | 2.23 +/- 0.08 | +10.3% |
| tail stage ms | 105.63 +/- 2.0 | 118.3 +/- 13.28 | -10.7% |

parallel: 204.7, 208.9, 217.8, 222.4. serial: 188.1, 189.6, 196.2, 202.3.
**The distributions do not overlap** - the slowest parallel run beats the
fastest serial one - and all eight passed 192/192 on structure and meaning.
This is the first change in this record whose effect survives interleaving.

Parallel runs happened to form wider batches, and a wider batch amortises
cost, so the comparison is repeated at matched widths:

| width band | serial sample ms | parallel sample ms | |
| --- | ---: | ---: | ---: |
| 9-16 | 23.4 | 16.8 | -28.3% |
| 17-32 | 42.3 | 24.7 | -41.7% |
| 33-64 | 74.8 | 38.4 | -48.7% |
| 65-128 | 102.0 | 58.7 | -42.4% |
| 129-512 | 161.5 | 145.3 | -10.0% |

Two caveats belong with that table. `llama_decode` is not parallelised and
should be unchanged between the arms, but moves -1% to -30% band by band,
so a per-band figure carries run-to-run noise and the throughput comparison
above is the reliable one. And the 129-512 band barely improves because its
`sample_us` is mostly not sampling: those are prefill batches, where only the
last row of each prompt has an output, and the time goes to constructing a
sampler for each newly seen sequence - a serial cost by necessity, since it
writes the sampler table. That construction is a separate target.

## A correctness check that could not be run

The intended proof was that parallel sampling changes no token: each row uses
its own sampler, so the output should be identical. It is not testable here -
**the two serial runs disagree with each other**. Batch composition varies
between runs, which varies the ubatch split, which varies the order of
floating-point reduction, which moves logits enough to change a sampled
token. The pipeline is non-deterministic run to run independently of this
change. The acceptance bar that did apply is the judge: 192/192 structure and
meaning in all eight runs. A real identity test needs a fixture that pins
batch composition, and there is not one.

# 2026-09-03: what the four-node pipeline does under load, and what it does not

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

## Not yet shown

Only the first node is timed. Overlap between stages - the same execution
open on two nodes at once, and the peak number of executions in flight - is
inferred from lap time over submission interval (about 3.6) and not traced.
That needs a per-stage span with a shared dispatch identity, which is the
next instrument.

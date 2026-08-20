# The plan

What is going to be built, in what order, and how each step is known to be
done. Written 2026-08-21.

Why it looks the way it does is in
[adapter-boundary.md](adapter-boundary.md); the numbers are in
[runtime-evidence.md](runtime-evidence.md). This document is only the work.

## 0. Where things stand

Working, and verified end to end on two cards with a real question and a real
answer:

- The adapter boundary carries a request, opaque state, a bound and options,
  and nothing that names a backend.
- A decode lap is batched by default, worth an order of magnitude:
  **69.2 tok/s** on the per-sequence path against **694–713** batched.
- The staged cut-set holds no opinion about the model. Gemma 4 E2B, whose
  boundary carries 55 tensors, answers correctly across a 17/35 layer split;
  Qwen2.5-1.5B, whose boundary carries 1, answers across 14/28.
- All four driver verdicts pass at parallel 1 and 4.

Not working, and known:

- Gemma cannot use the batched decode path: merging along the token axis is a
  concatenation, and concatenating a rank-3 tensor interleaves.
- **A stage executes every chunk of its work before emitting anything.**
  `llama_stage_runtime_hop.cpp:152` loops the chunks and appends each one's
  boundary tensors to a single payload that crosses the wire once, at the end.
  A 5,000-token prefill runs ten ubatches on stage 0 while every other stage
  holds nothing, and arrives as one payload of all ten fragments. This is a
  policy that empties a pipeline, and it is the origin of the 40 MB prefill
  payload rather than an intrinsic cost of prefill.
- **A stage can silently mis-slice a cut-set from a peer with a different
  `n_ubatch`.** The receiver recomputes the chunk count from its own
  `n_ubatch` and then checks only that the descriptor count divides by it
  (`llama_stage_runtime_hop.cpp:102`). A producer sending one bundle of 55
  descriptors to a receiver that computes five chunks passes `55 % 5 == 0` and
  binds eleven descriptors per chunk, with no error anywhere. Only a mismatch
  that fails to divide is refused, and that is luck rather than design. The
  producer's bundle count is not on the wire at all.
- `tools/drive/src/session/replies.rs` is 703 lines against a 400-line rule.
- The four-node 35B numbers — **37.4 tok/s combined, 18.2 generation**, cards
  at 18–45% — predate every fix above and were taken at a parallelism too low
  for four stages.

## 1. Make a hop one physical ubatch, and let fragments flow

**The point.** A stage does not emit until it has executed every chunk of the
work it was handed. `llama_stage_runtime_hop.cpp:152` loops over the chunks
and appends each chunk's boundary tensors to one `result.descriptors`, which
crosses the wire once at the end. A 5,000-token prefill therefore runs ten
ubatches on stage 0 while stages 1 to N hold nothing, and arrives as one
payload of every fragment at once — the 40 MB figure in
[adapter-boundary.md](adapter-boundary.md) §8.7 is this, not an intrinsic cost.

In a single process that is a reasonable policy; upstream `llama-server` fills
`n_batch` and lets llama.cpp split it, and the only cost is latency to the
next scheduling decision. **Across a stage boundary it is a policy that
empties the pipeline**, because the boundary is where work becomes visible to
the next card.

So the unit of a scheduler turn must be the unit that crosses the wire:

```
rows in one execution
  = Σ decode rows  +  Σ prefill fragment rows   ≤  boundary_ubatch
```

`boundary_ubatch` is not a GPU tuning number. It is how much a stage may
compute before the result becomes the next stage's input, and therefore it is
also the flow-control unit and the bound on how much boundary memory is in
flight at once.

**What must be built.** One execution per hop, and the fragment emitted as
soon as it exists, so that a long prefill streams:

```
stage 0: prefill fragment 2
stage 1: prefill fragment 1
stage 2: decode rows
```

**Done when.** A 5,000-token prefill crosses the boundary as fragments rather
than as one payload; a trace shows stage 0 working on fragment N+1 while stage
1 works on fragment N; boundary bytes in flight per sequence are bounded by
`boundary_ubatch` rather than by the prompt length; and the answers are still
answers.

## 2. Put the fragment layout on the staged wire

**The point.** The receiver reconstructs the producer's grouping by dividing.
`SequencePayload` carries `n_tokens` and nothing about layout
(`staged/server/src/protocol/protocol.hpp:132`), so the consumer derives its
own chunk count from its own `n_ubatch` and checks only that the descriptor
count divides by it. §0 records what that costs.

A bundle **count** is not enough. It cannot express a final fragment shorter
than the others, and it cannot express two stages with different ubatch
limits. The layout has to be stated: the rows in each bundle, in order —
`bundle_rows[]` or an equivalent — so the consumer reads the producer's
grouping instead of guessing at it.

**What must be built.** The layout field, written by the producer and checked
by the consumer against what it can execute; a refusal, naming both sides,
when it cannot.

**And a rule for mixed versions.** A four-node deployment starts four binaries
and nothing makes them the same build. Both directions have to be decided
rather than discovered — an old producer to a new consumer, and a new producer
to an old consumer. Refusing both is defensible and probably right, since the
alternative is the silent mis-slice of §0 and a deployment that will not start
beats one that answers wrongly. Whichever is chosen, the refusal names the
version it saw and the version it wanted, and the staged capability report is
where that belongs.

**Done when.** A stage whose peer used a different fragment layout is refused
with a message naming both; a stage meeting the other protocol version is
refused with a message naming both; both covered by tests that need no GPU.

## 3. Define fragment credit and ordering

**The point.** Streaming fragments breaks an invariant P4 currently relies on:
one lap per sequence in flight. With fragment N+1 at stage 0 while fragment N
is at stage 1, a sequence is in two places at once — which is correct for
prefill fragments and **not** correct for decode, where token T+1 must not
start before T has landed everywhere.

The invariant therefore has to be restated rather than dropped. Ordering is
per `(sequence_id, epoch, fragment_index)`: a stage takes a sequence's
fragments in index order, and a decode lap is still one at a time.

**And credit is not counted in sessions.** A decode row and a 512-row prefill
fragment are the same "one request" and cost entirely different amounts of
boundary buffer. Flow control has to be in **rows and boundary bytes**. The
`ceiling` that P4 keeps admits sessions against the KV pool — that is a
different budget and it does not bound what is in flight.

**What must be built.** The ordering rule and the credit unit, written down;
per-stage credit accounting; and what P4 does when credit is exhausted.

**Done when.** Out-of-order fragments are refused rather than executed; a run
whose prompts are long shows bounded boundary memory rather than growth with
prompt length; and the credit exhaustion path is exercised by a test.

## 4. Compose one execution in the adapter

**The point.** Only now is mixing worth attempting, and it is a new execution
path rather than a widening of the existing one. `execute_decode_batch`
refuses anything but one row per sequence
(`llama_stage_runtime_hop_decode.cpp:135`), so decode, prefill fragments and
later MTP verification rows sharing an execution is code that does not exist.

The compaction that would make a mixed batch unsplittable is
`ggml_get_rows(cur, inp_out_ids)`, guarded by `il == n_layer - 1` in 100 of
the 117 model implementations that use it, and a stage forwarding a cut-set
ends before that layer — measured at one row per token. Seventeen guard it
otherwise, so the row count that came back is checked against the tokens that
went in, and a mismatch refuses the batch. That is a statement about the
transfer, not about the model.

**What must be built.** An adapter-internal scheduler that fills one
`boundary_ubatch` from what it holds: decode rows first, then prefill fragment
rows within a per-session quantum so one long prompt cannot take the whole
budget. Remove the lane separation from the P4 composer, which no longer
decides anything.

**Done when.** A trace shows decode rows and prefill fragment rows in the same
`llama_decode`; the row-count check is exercised by a test; the four verdicts
pass; a Korean answer is still a Korean answer; and stage overlap is visible
rather than asserted.

**Risk.** The prior runtime limited a mixed window to one prefill token per
session and recorded that dropping the single-session term crashed the
terminal stage on 2026-08-13. Compaction does not explain that and it has not
been reproduced here. If it returns, stop and find out why.

## 5. Move the rest of batching into the adapter

**The point.** Width against latency is a backend judgement, and P4 currently
makes the call with none of the information.

**The contract this replaces, which must be written before any of it is
built.** `Adapter::start()` today means "this hop, whole, and not before the
last one finished" (`adapters/adapter/src/lib.rs:49`). Handing queueing to the
adapter changes that contract, so these are deliverables:

- **Ownership.** From which moment does a queued session belong to the
  adapter, and what may P4 assume about one it has handed over?
- **Cancellation and deadlines.** A session cancelled or expired after it was
  handed over: who drops it, what happens to the KV slot it holds, and what
  happens to a result produced for it afterwards.
- **Terminal attribution.** `HopComplete` reports `expected` against
  `outcomes`. With the adapter choosing the set, what does `expected` mean,
  and what still rejects a completion for a session P4 did not hand over?
- **Backpressure.** Who owns the queue bound, how it is reported, and what P4
  does when it is reached — in the credit unit of item 3, not in sessions.
- **Compatibility.** `served` and `mock` implement the current contract.
  Either they move too, or the trait keeps a non-queueing path, and which it
  is decides how much of the mock's test surface survives.

**Done when.** `compose()` no longer exists in the node; each point above is
covered by a test rather than by intent; a fleet run shows widths chosen by
the adapter with the queue wait and the cancelled and expired counts in the
trace; and throughput at parallel 64 on two cards is no worse than the 694–713
already measured.

**Watch for.** The one-hop-at-a-time gate goes with this, but it is not the
bottleneck and removing it will not by itself raise utilisation. Cohort
structure does that. Do not claim otherwise from a run that also changed the
parallelism.

## 6. Re-measure the 35B on four cards

**The point.** Every figure in §0 for the four-node run predates the work
above.

**What must change about the measurement.** Filling an S-stage chain needs at
least S cohorts in flight, so

```
parallel  ≳  stages × useful batch width
```

Ten sequences across four stages is about one and a half cohorts, which is the
18–45% utilisation that was measured. **Do not repeat the run at parallel 10.**
Sweep the parallelism instead, and report utilisation alongside throughput so
the cohort count is visible rather than inferred.

**What must be fixed before a number is taken.** A table and a utilisation
figure do not make a measurement reproducible, and the run this replaces is
already hard to compare against: its evidence records `parallel=10` and hops
carrying one sequence
(`staged/scripts/validation/evidence/2026-08-20-four-node-35b-service-reference.md`).
Fix and record, per run:

- the model file and its hash, the stage layer split, and the per-card
  assignment;
- `n_ctx`, `n_batch`, `n_ubatch`, KV cache type, flash attention, and the
  declared `ceiling`;
- the sampler settings, in full, since a run that ends early on EOS is not
  the same measurement as one that does not;
- the arrival schedule, the warm-up discarded, and the number of repetitions;
- the sampling period for card utilisation, because a mean over a period
  longer than a lap hides exactly the alternation being investigated;
- what counts as a completed request, judged on the answer and not only on
  the four verdicts.

**Done when.** A table of aggregate and per-session throughput against
parallelism, with card utilisation and hop widths beside it, repeated enough
to show the spread rather than one figure; every fixture above recorded with
it; and an explanation of where the knee is that follows from the cohort
arithmetic rather than from a guess.

## 7. Split `replies.rs`

703 lines against a 400-line rule, holding four things with different reasons
to change and about 160 lines of tests. Mechanical. It waits behind the items
above because a long file misleads nobody, and the documents that did have
been fixed.

## Open, unscheduled

**The tail's sampler chain.** 0.396 ms per row over a 151,936-entry
vocabulary, run one row after another on one thread — about a third of the
ring. The rows are independent and each has its own sampler, so the work
parallelises; the accessor does not, because `llama_get_logits_ith` calls
`output_reorder()`, which writes. Either a minimal patch in the compatibility
series making that a no-op when there is nothing to reorder, or llama.cpp's
backend sampler chains, which are marked experimental and incompatible with
grammar.

**Gemma's batched path.** Needs a merge that can express an interleaved token
axis rather than a concatenation.

**The 2026-08-13 terminal-stage crash** recorded by the prior runtime, which
compaction does not explain and which nothing here has reproduced.

**MiniMax M3.** Pure attention, 60 layers, 278 GiB — necessarily offloading.
The loading strategy asked for is KV cache into VRAM first, then layer bodies
in what remains, then FFN layers with whatever is left. Nothing has been built
for it and none of the work above assumes it.

## How this gets done

One task at a time, given to a subagent with instructions detailed enough that
the model does not have to supply the missing context. Each task states what to
build, how to verify it, and what not to do. Nothing is committed by a
subagent; results are checked against the gates and the evidence text before
they land.

The gates are `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets`, `cargo test --workspace`, and the staged server's own ctest.
None of them is sufficient. Four of the six defects behind the last six
commits passed every driver verdict, and one of them answered every request
with punctuation, so a run is judged by reading what it actually produced —
and by asking a question whose right answer is recognisable.

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
- Chunk grouping assumes both stages run the same `n_ubatch`; a mismatch is
  refused cleanly rather than corrupting, but nothing states the bundle count
  on the wire.
- `tools/drive/src/session/replies.rs` is 703 lines against a 400-line rule.
- Eight passages in `protocol-mtp.md` still name removed P4 fields.
- The four-node 35B numbers — **37.4 tok/s combined, 18.2 generation**, cards
  at 18–45% — predate every fix above and were taken at a parallelism too low
  for four stages.

## 1. Mix a prefill with decodes in one execution

**The point.** Accepting new work while existing work continues is a mixed
execution. P4 forbids it today — "a window never mixes lanes" — so a arriving
prompt waits for a decode window to drain and a decode waits for a prompt.
upstream `llama-server` has never worked that way: it puts every generating
slot's sampled token in first and fills the rest of `n_batch` with pending
prompt tokens.

**Why it is safe now.** The output compaction that would make a mixed batch
unsplittable is `ggml_get_rows(cur, inp_out_ids)`, guarded by
`il == n_layer - 1` in 100 of the 117 model implementations that use it. A
stage that forwards a cut-set ends before that layer, so its boundary tensor
is shaped by the ubatch's token count: one row per token, measured at 49.97
rows per tensor for a 50-token prefill.

**What must be built.**

- Remove the lane separation from the window composer.
- Add the row-count check: the rows that came back must equal the tokens that
  went in, and a mismatch refuses the batch rather than splitting it wrongly.
  Seventeen implementations guard the compaction differently, so this is not
  optional. It is a statement about the transfer, not about the model.
- Decide the mix in the adapter, not above it.

**Done when.** A run whose requests arrive staggered shows prefill rows and
decode rows in the same `llama_decode`, verified from the hop trace; the four
verdicts still pass; a Korean answer is still a Korean answer; and the
row-count check is exercised by a test rather than only by hope.

**Risk.** The prior runtime limited a mixed window to one prefill token per
session and recorded that dropping the single-session term crashed the
terminal stage on 2026-08-13. Compaction does not explain that, and it has not
been reproduced here. If the crash returns, stop and find out why before
working around it.

## 2. Move the rest of batching into the adapter

**The point.** Width against latency is a backend judgement. upstream spends
`n_batch` on it; the prior runtime spends a per-session quantum. Neither
number means anything above an adapter, and P4 currently makes the call with
none of the information.

**What must be built.**

- P4 forwards sessions as they become ready. The adapter queues them and
  decides what one physical execution is.
- Row identity travels inside the adapter's own bytes, so a node can compose
  an execution from several arrived blocks. Merging is a concatenation;
  splitting costs 4.45 ms, 28% of stage 0's hop, and stops being necessary.
- P4 keeps identity and routing, one lap per sequence in flight, the request's
  bound and `emitted`, cancellation and deadlines, and one number: `ceiling`,
  which is the KV budget expressed as a session count.
- Delete the window composer's ceiling handling, lane policy and preference
  ordering. Their tests move to the adapter.

**Done when.** `compose()` no longer exists in the node; a fleet run at
parallel 32 or more shows execution widths chosen by the adapter and varying
with arrival, not with a P4 policy; throughput at parallel 64 on two cards is
no worse than the 694–713 already measured.

**Watch for.** The one-hop-at-a-time gate goes with this, but it is not the
bottleneck and removing it will not by itself raise utilisation. Cohort
structure does that. Do not claim otherwise from a run that also changed the
parallelism.

## 3. State the bundle count on the wire

**The point.** Chunk grouping is `descriptors.len() / chunks.len()`, correct
only while both stages run the same `n_ubatch`. Nothing enforces that across a
boundary.

**What must be built.** A field on the staged `SequencePayload` carrying how
many bundles a payload holds, written by the producer and checked by the
consumer. A protocol revision, natural to take with item 2 once the adapter
owns what a bundle is.

**Done when.** A stage configured with a different `n_ubatch` than its peer is
refused with a message naming the mismatch, and a test proves it.

## 4. Re-measure the 35B on four cards

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

**Done when.** There is a table of aggregate and per-session throughput against
parallelism, with card utilisation, and an explanation of where the knee is
that follows from the cohort arithmetic rather than from a guess.

## 5. Split `replies.rs`

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

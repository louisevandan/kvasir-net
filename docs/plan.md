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

## 1. The adapter handoff contract

**First, because everything after it invents a policy otherwise.** Items 2 to
6 all hand work to an adapter and get results back on the adapter's schedule.
Building any of them before the handoff is defined means whoever builds it
decides ownership, cancellation and completion by themselves, in code, once
per item.

`Adapter::start()` today means "this hop, whole, and not before the last one
finished" (`adapters/adapter/src/lib.rs:49`). That is not a detail; it is
recorded as an invariant with its reason — "A node runs one hop at a time: it
starts the next only on seeing the previous end; a timer instead would overlap
them" ([constraints.md](constraints.md)). **This work removes it**, so that
entry has to be rewritten rather than quietly contradicted, and the reason it
gives has to be answered: what now prevents two executions overlapping in a
way nobody observes.

**What must be written down and covered by tests.**

- **Ownership.** From which moment a queued session belongs to the adapter,
  and what P4 may assume about one it has handed over.
- **Cancellation and deadlines.** A session cancelled or expired after
  handover: who drops it, what happens to the KV slot it holds, and what
  happens to a result the backend produces for it afterwards.
- **Completion is all-or-nothing and must stop being so.** The node accepts a
  `HopComplete` only when `expected_set == outcome_set == in_flight_set`
  (`agent/src/node/runner/events.rs:196`): every session in flight completes
  in the same event or the event is invalid. A fragment finishing while its
  peers continue is not merely unhandled — it **cannot be expressed**. This is
  the actual change behind "the adapter owns its queue": in-flight becomes a
  set the adapter draws from and reports against item by item, and what
  replaces the three-way equality has to reject a completion for a session P4
  never handed over, a duplicate, and a late one, without requiring the rest
  of the set to arrive with it.
- **`Hop.phase` goes, or means nothing.** A hop carries one
  `Prefill`/`Decode` for every sequence in it
  (`adapters/adapter/src/work/hop/mod.rs:18`), which makes a mixed execution
  unrepresentable at the boundary and makes P4 the author of a llama policy.
  P4 uses it in exactly one place — `reserves_sequence_slots() && phase ==
  Prefill` (`agent/src/node/runner/mod.rs:516`), an admission question — and
  that question is per sequence, not per window. The per-sequence answer is
  already on the wire: `prompt` present means this node begins the work,
  `state` present means it continues. Nothing else in P4 needs the field.
- **Backpressure.** Who owns the queue bound, how it is reported, what P4 does
  when it is reached — in rows and bytes, per item 3, not in sessions.
- **Compatibility.** `served` and `mock` implement the current contract.
  Either they move too, or the trait keeps a non-queueing path; that choice
  decides how much of the mock's test surface survives.

**Done when.** Each point above exists as prose and as a test, `constraints.md`
agrees with the code, and no later item has to decide any of it.

## 2. Fragment input state

**The point.** Stage 0 tokenizes the whole prompt in one hop
(`llama_stage_runtime_hop.cpp:47`) and then walks it with a local offset. Once
a hop is one ubatch, that offset has to survive between hops, and
`fragment_index` with `bundle_rows[]` does not carry it: they describe what
came out, not where the next execution resumes.

The adapter's opaque continuation state has to hold, at minimum:

- the tokenized prompt itself, as a canonical token vector, or a stable
  handle to one that no path can re-derive;
- `input_token_offset` and the row count of this fragment;
- the stage-local KV position;
- `(epoch, fragment_index)`.

**Two things it must not do.** It must not re-tokenize the prompt on each hop,
and it must not slice the prompt as a string. Keeping the source text and
promising not to re-tokenize is not enough: a handoff, a restore or a retry
will re-derive it, and a tokenizer is not guaranteed to agree with itself
across versions. The token vector is the contract. A tokenizer result is fixed once
and a token range is what travels; anything else makes the fragment boundary a
different boundary on each stage, and the failure is a wrong answer rather
than an error.

**Done when.** A prompt is tokenized exactly once per request across the whole
chain, provable from a trace; a fragment resumes at the token the previous one
ended on; and a test covers a resume across a hop.

## 3. Edge credit

**The point.** Flow control is in rows and boundary bytes, and the reviewable
mistake is returning credit too early. Credit is not released when a fragment
is handed to the transport. It is released when the peer has taken it — copied
or accepted or discarded — and said so. Failure, cancellation, timeout and
duplicate delivery all have to release the same lease, or the deployment
leaks credit and stalls without an error.

**The bounds are per edge, not per sequence.** The target state deliberately
puts one sequence's fragments on several stages at once, so a per-sequence
byte bound is the wrong shape:

```
per edge      rows ≤ U ,  bytes ≤ B
per sequence  at most (number of stage edges) × B
deployment    the sum of every edge's credit
```

`boundary_ubatch` is the row bound `U`. The byte bound `B` is not derivable
from it: Gemma 4 carries 55 tensors across its boundary and Qwen2.5 carries
one, so `B` is measured or conservatively estimated per model rather than
assumed.

**And both are per edge, negotiated, not per stage and assumed.** Each stage
takes `n_ubatch` from its own plan and pins `n_batch` to it
(`staged/server/src/server/plan.cpp:264`); nothing reconciles two stages, so
today a producer and a consumer can disagree and neither finds out. The edge
values are `U_edge = min(producer, consumer)` and a `B_edge` for the model and
cut-set on that edge, agreed when the chain is composed and carried the way
other stage facts already are — the capability report and `context_identity`
are where `n_batch`, `n_ubatch`, `n_seq_max` and `kv_unified` already travel.
A chain that cannot agree does not load.

**A lease needs an identity.** "The same lease" is not a rule until it names
one. A lease is `(deployment generation, edge, sequence, epoch, fragment)`,
and the receipt that releases it must be idempotent against that identity, so
that a retransmission after a timeout and a receipt that arrives after the
lease was already reclaimed are both safe and neither double-releases. The
deployment generation is in it because a reconnect must not let a receipt from
a previous generation release credit in this one.

**Done when.** `U_edge` and `B_edge` are agreed when the chain loads and a
disagreement refuses the load; credit returns on a peer receipt and on every
failure path, idempotently against the lease identity, with a test for a
receipt that arrives twice and one that arrives after a timeout reclaimed the
lease; each edge’s rows and bytes in flight are bounded and observable; a run with
long prompts shows bounded boundary memory rather than growth with prompt
length; and the exhaustion path is exercised by a test.

## 4. Fragment layout on the staged wire

**The point.** The receiver reconstructs the producer's grouping by dividing:
`SequencePayload` carries `n_tokens` and nothing about layout
(`staged/server/src/protocol/protocol.hpp:132`), so the consumer derives a
chunk count from its own `n_ubatch` and checks only that the descriptor count
divides by it. §0 records what that costs.

A bundle **count** is not enough either: it cannot express a final fragment
shorter than the others, nor two stages with different limits. The rows in
each bundle, in order — `bundle_rows[]` or an equivalent — is what the
consumer must read instead of guessing.

**And a rule for mixed versions.** A four-node deployment starts four binaries
and nothing makes them the same build. Both directions have to be decided
rather than discovered — an old producer to a new consumer, and the reverse.
Refusing both is defensible and probably right, since the alternative is the
silent mis-slice of §0, and a deployment that will not start beats one that
answers wrongly. Whichever is chosen, the refusal names the version it saw and
the version it wanted; the staged capability report is where that belongs.

**Done when.** A peer with a different layout is refused with a message naming
both; a peer on the other protocol version is refused with a message naming
both; both covered by tests that need no GPU.

## 5. Stream fragments, one physical ubatch per hop

**The point.** This is the defect in §0. `llama_stage_runtime_hop.cpp:152`
loops every chunk and appends each one's boundary tensors to a single payload
that crosses the wire once, at the end, so a long prefill runs all its ubatches
on one stage while the rest of the chain holds nothing.

The unit of a scheduler turn becomes the unit that crosses the wire:

```
rows in one execution  =  Σ decode rows + Σ prefill fragment rows  ≤  U
```

so that a long prefill flows instead of accumulating:

```
stage 0: prefill fragment 2
stage 1: prefill fragment 1
stage 2: decode rows
```

**Done when.** A 5,000-token prefill crosses as fragments rather than as one
payload; a trace shows stage 0 on fragment N+1 while stage 1 is on fragment N;
and the answers are still answers.

## 6. A scheduler that does not starve either side

**The point.** With items 1 to 5 in place, one execution can hold decode rows
and prefill fragment rows together. The compaction that would make that
unsplittable is `ggml_get_rows(cur, inp_out_ids)`, guarded by
`il == n_layer - 1` in 100 of the 117 model implementations that use it, and a
stage forwarding a cut-set ends before that layer — measured at one row per
token. Seventeen guard it otherwise, so the rows that came back are checked
against the tokens that went in and a mismatch refuses the batch. That is a
statement about the transfer, not about the model.

`execute_decode_batch` refuses anything but one row per sequence
(`llama_stage_runtime_hop_decode.cpp:135`), so this is a new execution path
rather than a widening of the existing one.

**"Decode first" is not the rule.** It is a latency preference, and as an
unconditional rule it starves prefill — which this repository already records:
"Lane preference is bounded: strict priority is a veto, a lap never dispatched
is a request that never finishes" ([constraints.md](constraints.md)). "At least one of" is not an implementation plan either, so the policy is
chosen here and may be argued with rather than left open:

- **A prefill reserve.** Whenever any prefill fragment is waiting, at least
  `R` rows of every execution go to prefill, `R` declared as a fraction of
  `U_edge`. Decode fills the rest.
- **A bounded wait, measured in wall clock and including GPU execution.** No
  waiting item exceeds `W`. A bound counted in scheduler turns is not a bound,
  because a turn is as long as the execution it contains.

`R` and `W` are declared with the plan, reported in the trace, and chosen from
the measured hop cost rather than from taste — a hop is 15.8 ms at stage 0 and
43.6 at the tail, so a `W` below either is not a policy but a promise that
cannot be kept. Anything more elaborate — deficit round-robin, age ordering —
is a later change with a measurement behind it, not a starting point.

**MTP rows are pinned at zero here.** See item 7.

**Done when.** A trace shows decode rows and prefill fragment rows in one
`llama_decode`; the row-count check is exercised by a test; under continuous
decode load a waiting prefill gets its `R` rows and no item waits longer than
`W`, both measured rather than argued; the four verdicts
pass; and a Korean answer is still a Korean answer.

**Risk.** The prior runtime limited a mixed window to one prefill token per
session and recorded that dropping the single-session term crashed the
terminal stage on 2026-08-13. Compaction does not explain that and it has not
been reproduced here. If it returns, stop and find out why.

## 7. MTP candidate blocks

**Why it is not part of item 6.** An MTP row is not an independent row. A
sequence's candidate block is ordered, has target and draft KV, and is the
unit of rollback; scheduling its rows as if they were decode rows would make a
partial block possible, and a partial block is unrecoverable state rather than
a slow answer.

Until this item is built, the scheduler pins MTP rows at zero. When it is, a
candidate block is admitted **whole or not at all**, against the remaining row
and byte credit of item 3.

`protocol-mtp.md` is the specification and now describes the current contract;
its gates A and C are the entry conditions for this item.

## 8. Move the rest of batching into the adapter

With items 1 to 6 done, what remains is deleting P4's window composer — its
ceiling handling, lane policy and preference ordering — and moving the tests
that guard them to the adapter.

**Done when.** `compose()` no longer exists in the node; a fleet run shows
widths chosen by the adapter with the queue wait and the cancelled and expired
counts in the trace; and throughput at parallel 64 on two cards is no worse
than the 694–713 already measured.

**Watch for.** The one-hop-at-a-time gate goes with item 1, but it is not the
bottleneck and removing it will not by itself raise utilisation. Cohort
structure does that. Do not claim otherwise from a run that also changed the
parallelism.

## 9. Re-measure the 35B on four cards

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

## 10. Split `replies.rs`

703 lines against a 400-line rule, holding four things with different reasons
to change and about 160 lines of tests. Mechanical. It waits behind the items above
because a long file misleads nobody, and the documents that did have been
fixed.

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

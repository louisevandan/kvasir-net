# The adapter boundary

What P4 hands a backend, what a backend hands back, and why almost nothing
crosses that line. Written 2026-08-21, after the boundary was rebuilt.

This document is the reasoning. The numbers behind it are in
[runtime-evidence.md](runtime-evidence.md) and in
`layers/adapters/llamacpp/staged/scripts/validation/evidence/`; the invariants
that must not move are in [constraints.md](constraints.md).

## 1. The principle

P4 moves a node's output to the next node. That is the whole of it.

Everything a backend needs in order to take one more step is the backend's,
and it travels as bytes P4 does not read. Model architecture is llama.cpp's
problem, batching policy is the adapter's, and even the adapter's own concrete
knowledge should reach no further than the transfer requires. ggml's RPC
backend and its device scheduler move tensors and graphs without knowing which
architecture produced them; this layer behaves the same way.

The test of a boundary is not whether it compiles. It is whether a second
backend can be attached by writing one `Adapter` implementation and changing
nothing else. Until 2026-08-20 that test failed, and the boundary itself said
so in its own documentation — see §3.

## 2. What crosses

What a node drives is the [`Adapter`](../layers/adapters/adapter/src/lib.rs)
trait (`apps/p4/layers/adapters/adapter/src/lib.rs:28`); everything below is
what its `start()` and `EventSink` carry.

Toward the backend, one sequence at a time — the real definition is
[`Sequence`](../layers/adapters/adapter/src/work/hop/mod.rs)
(`apps/p4/layers/adapters/adapter/src/work/hop/mod.rs:39`):

```rust
pub struct Sequence {
    pub sequence: SequenceId,
    /// The request as OUTER stated it. Present only where work begins.
    pub prompt: Option<String>,
    /// Everything a backend needs to carry this session one step further,
    /// exactly as an adapter wrote it on the step before. P4 never reads it.
    pub state: Option<Vec<u8>>,
    /// How many tokens this sequence still wants.
    pub remaining: u32,
    /// Opaque sampling and generation options, passed through whole.
    pub options: String,
}
```

Back from the backend, one per sequence in the completed hop — the real
definition is [`Outcome`](../layers/adapters/adapter/src/event/report/mod.rs)
(`apps/p4/layers/adapters/adapter/src/event/report/mod.rs:105`):

```rust
pub struct Outcome {
    pub sequence: SequenceId,
    /// What carries this session one step further, as this adapter wrote it.
    /// Handed back untouched as the next `Sequence::state`.
    pub forward: Option<Vec<u8>>,
    /// What the requester should see. Empty where this node produces nothing.
    pub text: String,
    /// Set when the sequence is finished and must not be scheduled again.
    pub stop: Option<String>,
    /// A verified request-level count for a terminal length stop, if any.
    pub terminal_generated: Option<u32>,
}
```

On the wire the same shape appears as
[`ToNode::Continue`](../layers/service/src/message/mod.rs)
(`apps/p4/layers/service/src/message/mod.rs:101`) `{ remaining, emitted,
options, state }`. `emitted` is P4's own tally of the tokens it has streamed
against the request's bound — P4 counts its own output rather than asking a
backend how far along it is.

### Terminal length accounting

[`Outcome::terminal_generated`](../layers/adapters/adapter/src/event/report/mod.rs)
is absent for ordinary output and every non-length stop. The staged adapter
sets it only when native `position >= Sequence::remaining` proves its own
`length` terminal; [`node::outcome::terminal_generated`](../layers/agent/src/node/outcome/mod.rs)
caps it at that same bound and never applies it to EOS. This preserves visible
stream text accounting while making `Done.generated` agree with a native length
terminal that ended on a textless/coalesced decode.

## 3. What used to cross, and why it was wrong

Four fields documented themselves as optional, and every one of them named the
same backend:

| Field | What its doc comment said |
| --- | --- |
| `Sequence::inbound_cut_set` | "`None` is the compatibility path for Internal and served adapters" |
| `Sequence::initial_tokens` | "Other adapters leave this absent because they own their autoregressive loop internally" |
| `Outcome::token` | "Internal and served adapters leave this absent because they own their decode loop" |
| `Outcome::position` | carried a backend's idea of how far a session had come |

A boundary most of its implementers opt out of is not a boundary. The wire
carried the same shape: `ToNode::Continue` named a position and a sampled
token, so a P4 frame could not be understood without knowing what a token is.

There was also a wrapper, `P4CUT01`, that packed an opaque cut-set around an
otherwise ordinary body, plus a rule in the node that the tail's cut-set is
dropped before stage zero. That rule was P4 knowing something about llama.cpp
staging — the sort of thing a boundary should not be able to do.

All of it is gone. Position, sampled token, cut-set and cut-set layout live
inside `state`, written by an adapter and read by one. The wrapper is gone
because state is a field of the continuation rather than an envelope around
it.

## 4. What the staged adapter does with it

`state` is the staged adapter's own `SequencePayload`, encoded in the same
envelope form the stage server is handed anyway. Nothing is left out — and
that mattered, because `SequencePayload::encode` writes only the cut-set and
stops. The position, the sampled token and the options were never in it; P4
used to carry them alongside. Encoding a state with the reduced form produced
a session that decoded token zero at position zero for ever, answered every
request, kept every stream in order, and emitted `!!!!!!`. See §6.3.

Two numbers a stage no longer takes from its plan, because neither is tuning —
both normalised in
[`plan.cpp`](../layers/adapters/llamacpp/staged/server/src/server/plan.cpp):

- **`n_batch` follows `n_ubatch`**
  (`apps/p4/layers/adapters/llamacpp/staged/server/src/server/plan.cpp:264`).
  A lap crosses the wire one ubatch at a time, so a wider batch describes a
  submission no stage makes — and it is not merely useless. The staged
  cut-set is bound once per decode, so a batch llama.cpp chooses to split
  hands the graph a narrower input than the lap it was given. Making the two
  one number removes that as a possibility rather than leaving it as a case
  to check.
- **The cache is unified**
  (`apps/p4/layers/adapters/llamacpp/staged/server/src/server/plan.cpp:265`).
  That is what keeps the ubatch meaningful past the prefill:
  `llama_kv_cache::init_batch` takes `split_simple` for a single stream, so a
  decode lap that fits the ubatch is one ubatch whichever slots its sequences
  hold.

The prior runtime reached the same place from the other direction: its window
is `min(batch_size, ubatch_size)` (`apps/llama/native/linker-node/inference/
session.inc:153`). Two implementations, one constraint.

## 5. What the measurements say

All on this machine, RTX 4080 holding layers `[0,14)` and RTX 3090 holding
`[14,28)`, `Qwen2.5-1.5B-Instruct-Q8_0`, generation-dominant (32-token prompt,
512 generated).

### 5.1 A hop, taken apart

Nothing is left over; the residual is under 0.1 ms on either stage.

| | stage 0 | tail |
| --- | ---: | ---: |
| `llama_decode` submit | 11.25 ms | 12.57 ms |
| wait for outputs | 0.01 ms | 9.50 ms |
| split the cut-set | 4.45 ms | 0.00 ms |
| sample | 0.00 ms | 21.40 ms |
| **hop** | **15.76 ms** | **43.58 ms** |

A lap is one hop of each, so the ring turns in about 59 ms carrying about 26
tokens.

### 5.2 A hop is nearly free in its width

| rows | stage 0 submit | tail submit |
| ---: | ---: | ---: |
| 2 | 8.55 ms | 10.74 ms |
| 27 | 11.77 ms | 12.78 ms |
| 62 | 13.57 ms | 13.23 ms |

Thirty-one times the work costs 1.2 to 1.6 times the time. **The marginal cost
of one more sequence in a hop is 0.04 to 0.08 ms.** Batching a lap already
works; the width is not what is expensive. Having a lap at all is.

Splitting the same model 7/21 instead of 14/14 puts the rest at about **3 to
5 ms per call plus 0.5 to 0.7 ms per layer** — an order of magnitude above what
these cards' bandwidth explains, because the CUDA graph is never armed.

### 5.3 What batching a lap is worth

Running the harness with the unified cache removed exercises the refusal path
end to end:

| | aggregate |
| --- | ---: |
| per-sequence, the refusal path | **69.2 tok/s** |
| batched, the default path | **694 to 713 tok/s** |

An order of magnitude. This is why batching is not behind a switch.

### 5.4 Two things that were tried and rejected

**Pinning the decode width.** `ggml-cuda.cu` resets its warmup on any node
property change and needs two consecutive identical calls to arm again;
`llm_graph_params::allow_reuse` needs the same `n_tokens`, `n_seqs_unq` and
`n_outputs`, and `can_reuse_kq_mask` adds the same `n_kv`. A lap whose width is
whatever arrived matches the lap before it 4 times in 1,010. Padding to a fixed
width raised reuse and **cut the tail's submit by 44%** — and moved the
aggregate from 712.9 to 671.0 tok/s. `llama_decode` is asynchronous, so the
time left the submit and reappeared in the wait. Reverted.

**Holding a narrow window open to gather laps.** Mean width rose 10.89 → 20.48
and throughput fell 768 → 450 tok/s. Graph builds per token improved by a third
— and graph builds per *second* collapsed from 132 to 51. In a closed ring
there is nothing else for the other stage to do during the hold, so the wait is
not overlapped; it is added. Reverted.

Both failures share a shape: the arithmetic was never what was being paid for.

### 5.5 Where the tail's time goes

| | per row |
| --- | ---: |
| `common_sampler_sample` | 0.396 ms |
| `common_detokenize` of the whole history | 0.024 ms |

Detokenising the entire answer on every token is quadratic and does not matter
— 6% of the per-row cost at 512 tokens. The cost is the sampler chain over a
151,936-entry vocabulary, run one row after another on one thread while the
card and 47 other cores wait. About a third of the ring, and unaddressed.

### 5.6 Four cards, thirty-five billion parameters

`Ornith-1.0-35B-UD-Q5_K_S` across two machines, 60 requests of a 5,000-token
prompt against 5,000 tokens of answer, ten admitted at once. Sixty of sixty
completed. **37.4 tok/s combined, 18.2 generation.** Card utilisation sat
between 18% and 45% and never held; hop windows averaged 6 to 7 of the 10
admitted. That measurement predates everything in §6 and must be repeated.

## 6. Defects found, and their mechanisms

Recorded because each one is a class, not an incident.

### 6.1 A boundary that re-derived shapes

`ggml_rank()` and `make_contiguous()` rebuilt a descriptor into a canonical
form. llama.cpp's input matcher compares `ne` and `nb` against the graph tensor
exactly, so a rebuilt descriptor has to be right about a model this layer does
not read. Deleted.

### 6.2 A cut-set counted by its descriptors

`execute_hop` treated one descriptor as one chunk and read a token count out of
`descriptor.dimensions[1]`. That identity holds only where the boundary carries
one tensor:

| model | boundary | tensors in the cut-set |
| --- | --- | ---: |
| Qwen2.5-1.5B | layer 14/28 | **1** |
| Gemma 4 E2B | layer 17/35 | **55** |

Gemma's `build_inp_per_layer` projects `[n_embd_per_layer, n_tokens, n_layer]`
before the layer loop and every layer takes a slice, so the whole thing crosses
whatever boundary is drawn. A 40-token prefill arrived as 55 descriptors and
was refused. Chunking is now `ceil(n_tokens / n_ubatch)` on the count the
payload *states* — which its own doc comment always asked for.

### 6.3 A state that was not a state

See §4. The reduced encoding lost position, token and options. **Every verdict
passed and the answer was punctuation.**

### 6.4 A sampling stage that forwarded its cut-set

The tail's hidden state describes the layers behind it and belongs to nobody
ahead; the next lap starts at stage 0 from the token. P4 used to drop it on the
adapter's behalf. The adapter drops it itself now.

### 6.5 Streaming that split characters

Token boundaries are not character boundaries. An incomplete trailing UTF-8
sequence was replaced with U+FFFD rather than held for the next token, so a
Korean answer came out as `러스트 프로그래밍 언어는???시아어로`. It is held now.
**This was invisible while every test prompt was ASCII.**

### 6.6 Event numbers spent on silence

`event_seq` was advanced once per lap of the chain rather than once per reply,
including on laps that emit nothing. Every silent lap burned a number no
subscriber saw, and a producer-made hole is indistinguishable from a frame the
transport lost — which is the one question `event_seq` exists to answer. Silent
laps are ordinary: the first decode primes state, and a backend holding a
partial multi-byte character makes more. A Korean 400-token answer had ten
holes; an English one had one.

Nothing below the fleet could catch it, because the mock emitted text on every
non-finishing lap and a silent lap did not exist there. `Profile::mute_every`
makes one, and the defect then reproduces in milliseconds.

### The pattern

Four of these six passed every verdict. **Every request answered, no request
failed, every stream in order, one terminal per route** say nothing about
whether the answer is an answer. Read `sessions-evidence.md`, and use a prompt
whose correct answer you can recognise — a Korean question found two defects
that months of ASCII prompts did not.

### Producing a `sessions-evidence.md`

It is not a file the repository ships; nothing writes it unless asked, and no
copy is checked in.

`p4-drive` writes it only when the environment variable
`P4_DRIVE_EVIDENCE_FILE` names a destination path, checked once after a run
finishes
(`apps/p4/tools/drive/src/main.rs:202`) and written by
[`report::write_evidence`](../tools/drive/src/report/mod.rs)
(`apps/p4/tools/drive/src/report/mod.rs:12`) — the full prompt and complete
response text for every session, plus the aggregate telemetry JSON. Leave the
variable unset and the run still completes; only this file does not appear.

[`run-local-real-two-stage.ps1`](../tools/scripts/e2e/run-local-real-two-stage.ps1)
now sets it for every run, defaulting to `evidence.md` beside the run's other
logs, and overridable with `-EvidenceFile`. It deliberately ignores an
inherited value and restores the caller's environment afterwards: the script
starts `p4-drive` with `Start-Process`, which inherits the parent session's
environment, and an exported path persists across `& script.ps1` calls in one
PowerShell process — so a sweep filed every run's text under the first run's
path. Retaining the text is not optional: a run whose model samples an
end-of-generation token on its first step passes all four driver verdicts —
answered, not failed, in order, one terminal — while returning nothing at
all, and `Assert-NonEmptyResponses` in the same script is what catches that.
One complete
reproduction, assuming locally built `p4-agent.exe` / `p4-drive.exe` /
`p4_staged_server.exe` and a real GGUF model already sit at the script's
default paths (override with `-AgentBinary` / `-DriveBinary` / `-ServerBinary`
/ `-Model` otherwise):

```powershell
apps\p4\tools\scripts\e2e\run-local-real-two-stage.ps1 -RunId manual-run -Tokens 200 -PromptTokens 200
```

It lands in that run's own directory (`target\real-two-stage-5000\<RunId>\`),
next to the run's agent and drive logs, unless `-EvidenceFile` names somewhere
else. An exported `P4_DRIVE_EVIDENCE_FILE` no longer chooses the path: the
script overwrites it for the run and restores the caller's value afterwards.

Whether git keeps it out: root `.gitignore` has no pattern for `target/` at
the repository root — only `apps/p4/tools/drive/target/` (the drive crate's
own Cargo output) and, via `apps/p4/.gitignore`, `apps/p4/target/` are
ignored, and a blanket `*.log` rule catches the stage servers' own log files.
None of those match a `.md` file under root `target/`. A `sessions-evidence.md`
written there is therefore an ordinary **untracked** file: `git status` lists
it, and it stays out of the repository only because the standing instruction
here is to stage files by name rather than `git add -A` — not because git
excludes it. Do not expect to find one already checked in, and do not assume
`git add -A` would skip it.

## 7. Known not to work

- **Gemma 4 E2B cannot use batched decode.** Merging along the token axis is a
  concatenation, and concatenating a rank-3 tensor interleaves rather than
  joins, so the batched lap is refused and the per-sequence path runs.
  Generation is about 6 tok/s against Qwen's 79.5 at the same parallelism.
- **The chunk grouping rests on an unenforced invariant.** Bundles per chunk is
  `descriptors.len() / chunks.len()`, correct only because both stages run the
  same `n_ubatch`. A mismatch produces a clean refusal rather than corruption,
  but the honest fix is for the producer to state the bundle count on the wire.
- **`tools/drive/src/session/replies.rs` is 703 lines**, over the repository's
  400-line limit, holding four things with different reasons to change.
- **The 35B four-node figures in §5.6 predate every fix above.**

## 8. The plan: batching belongs to the adapter

### 8.1 What actually keeps the cards idle

An earlier draft of this section blamed P4's refusal to hand a node a second
hop while one is in flight — `is_running()` is `in_adapter() > 0`. That was
wrong, and the correction matters more than the original claim.

A chain is serialised by the token dependency, not by that gate. Stage 0
cannot begin lap N+1 for a session until the token from lap N has come back
round, and no scheduling choice changes that. What a scheduler *can* change is
how many **independent cohorts** are in flight at once. If every ready
sequence goes into one hop, the whole population moves as a single wave: while
the wave is at stage 1, stages 0, 2 and 3 hold nothing at all, and per-card
utilisation is bounded by roughly 1/S for an S-stage chain. Nothing about the
gate causes that. **The composer taking everything that is waiting causes it.**

The measurements in §5.4 say exactly this, and they were misread once already.
Holding a window open to gather laps raised the mean width from 10.89 to 20.48
and dropped throughput from 768 to 450 tok/s; graph builds per token improved
by a third while graph builds per second collapsed from 132 to 51. The narrow
hops in the `35,1,1` pattern were not waste being cleaned up. **They were the
several cohorts that kept the chain occupied**, and gathering merged them into
one.

So the arithmetic a scheduler is working against is:

- one more row in a hop costs **0.04 to 0.08 ms** (§5.2);
- a stage with nothing to do costs a **whole hop** — 15.8 ms at stage 0, 43.6
  at the tail (§5.1).

Waiting to widen is therefore almost never right, and **fire with whatever is
ready** is the policy the numbers support.

### 8.2 A consequence for how a deployment is sized

Filling an S-stage chain needs at least S cohorts in flight. The four-node 35B
run of §5.6 admitted ten sequences and composed hops of six to seven — about
one and a half cohorts across four stages, which is the 18–45% utilisation it
measured, and is what the shape predicts.

Roughly, a deployment wants

```
parallel  ≳  stages × useful batch width
```

At four stages, ten admitted sequences cannot be both wide and spread. That is
a sizing fact rather than a defect, and it means **the 35B figures should not
be re-measured at parallel 10.**

### 8.3 What the adapter gets that P4 cannot have

Adapter-owned batching does not create the concurrency above — cohort
structure and the parallelism level do. It buys two other things.

**Mixing a prefill with decodes in one execution.** P4 forbids it today: "a
window never mixes lanes". Yet accepting new work while existing work
continues is precisely a mixed execution, and it is what upstream
`llama-server` does — `server-context.cpp` puts every generating slot's
sampled token in first and then fills the remainder of `n_batch` with pending
prompt tokens.

This is safe at a staged boundary, with one check. The output compaction that
would make a mixed batch unsplittable is `ggml_get_rows(cur, inp_out_ids)`,
and in 100 of the 117 model implementations that use it, it is guarded by
`il == n_layer - 1` — the model's final layer. A stage that forwards a cut-set
ends before that layer, so its boundary tensor is shaped by the ubatch's token
count and carries one row per token. Measured: a prefill cut-set of 614,047
bytes over two tensors at `n_embd` 1536 is 49.97 rows per tensor, one per
token.

Seventeen implementations guard it differently, so the adapter must not assume
this — it must **check that the rows it got back match the tokens it sent**,
and refuse the batch otherwise. That is a statement about the transfer, not
about which model is running, so it belongs at this layer.

The prior runtime concluded the opposite and limited a mixed window to one
prefill token per session, citing compaction to the sequence count
(`apps/llama/native/linker-node/inference/session.inc:161`). That reasoning
holds for a stage whose window contains the final layer and over-generalises
otherwise. Its second reason — that dropping the single-session term was
measured on 2026-08-13 and crashed the terminal stage — is not explained by
compaction and has not been reproduced here; treat it as unexplained until it
is.

**Choosing width against latency with the numbers in hand.** The marginal cost
of a row, the size of a prefill chunk and the ubatch ceiling are all backend
facts. upstream spends `n_batch` on this decision and the prior runtime spends
a per-session quantum; neither number means anything above an adapter.

### 8.4 What moves

Out of P4: window composition, lane separation, the decode/prefill preference,
and the one-hop-at-a-time gate — the last not because it is the bottleneck but
because it is a batching decision and batching is leaving.

The adapter receives sessions as they become ready, queues them, and decides
what one physical execution is. The existing event shape already fits:
`HopComplete { expected, outcomes }` reports whatever set the adapter ran.
Only the inbound direction changes, from a composed window to a stream of
ready sessions.

### 8.5 What P4 keeps

- **Identity and routing.** Which session, and where its output goes next.
- **One lap per sequence in flight.** If a node computes token T+1 for a
  session before T has reached the tail, that session's stages diverge and
  nothing recovers. This is session lifecycle, so it is P4's; the composer
  already states it — "every live sequence has exactly one lap waiting".
- **The request's bound and terminal accounting**, including `emitted`.
- **Cancellation and deadlines.**
- **One number: `ceiling`.** KV is the only budget that accumulates — reserved
  at context creation, held until the session ends — while the ubatch
  workspace is reused and the weights are fixed. The ceiling is that budget
  expressed as a session count, and it is the only thing P4 needs to know
  about how much a backend can hold.

### 8.6 The two requirements on the payload

**Row identity must travel in the bytes.** To compose an execution from what
has arrived, a node must be able to take rows from several arrived blocks and
leave others for later, which means knowing whose each row is. That belongs in
the adapter's own payload, not in a P4 field. The arithmetic favours it:
merging is a concatenation, splitting is not — §5.1 measures the split at
4.45 ms, 28% of stage 0's hop — and what a downstream node wants is the cheap
direction.

**Per-sequence order must hold.** The tokens of one sequence need not share an
execution, but they must not be reordered. Position travels per row, so this
is checkable rather than hoped for.

### 8.7 What decoupling costs

Today one hop is in flight per node, so one boundary payload is. Independent
stages mean several laps outstanding, each holding bytes:

| | per row |
| --- | ---: |
| decode, one token, `n_embd` 2048, F32 | **8 KB** |
| Gemma 40-token prefill, 55 tensors | 614 KB |
| 5,000-token prefill, one session | **40 MB** |

Decode is free — ten sequences is 80 KB per boundary. **Prefill is the whole
of the cost**, and it is bounded by how large a prefill chunk the adapter
chooses, which is a knob the adapter holds. Throughput is a decode problem, so
the gain is kept and the bill lands where it can be paid.

## 9. Sequenced work

Moved to [plan.md](plan.md), which states each item, how it is verified,
and what not to do while building it.

## 10. History

| Commit | What |
| --- | --- |
| `cd6643176` | measure a decode hop instead of guessing at it |
| `f925ddf5c` | batch a decode lap by default |
| `b6e16941f` | give P4 a boundary that is not shaped for one backend |
| `09f0223c2` | make a forwarded state actually complete |
| `e75676267` | stop the staged cut-set from having opinions about the model |
| `b94156408` | number response events by responses, not by laps |

# P4 staged MTP protocol

> Document status (2026-09-06): **Domain contract; distinct from implementation**. Read it for the contract/goal of the domain it owns, but do not treat that as implemented. If it conflicts with the current development order, follow the explicit migration in the roadmap.
> For the current goal, status and order, follow the [execution roadmap](distributed-batching-roadmap.md); for document authority and reading paths, follow the [document map](document-map.md).

## Status and scope

This document fixes the decisions and the build plan for sealing MTP into the protocol in staged P4.
The current staged implementation covers only the MTP parser/ownership probe; the existence of this document does not mean
that `mtp_execution` is supported. The execution capability is advertised only after it passes the prerequisite gates and
integration verification below.

In the current code, the Rust staged adapter reports `HopComplete`, `SequenceAcquired` and
`SequenceReleased`, and the C++ `StageRuntime` runs only ordinary HOPs on the production path.
`execute_mtp_hop` is an ownership/test path that uses a prompt, `seq_id = 0` and a draft ceiling of 1,
and it is not connected to production MTP execution. This document is therefore not
a description of current behaviour; it is a build document for opening up execution while preserving the current event/transport
boundary.

Current validation results also take `mtp_parser=1`, `mtp_execution=0` as the baseline.
`validate-mtp-speculative-capability.mjs` confirming parser/ownership support is not enough to
judge production execution a success, and that capability is not raised until this document's A, B and C and real execution verification
are finished.

The scope is as follows.

- Do not expose `llama.cpp`'s internal MTP behaviour in the P4 wire protocol.
- Carry candidates and confirmed tokens using the existing multi-row HOP representation.
- Each stage lazily rolls back its own speculative KV suffix when it enters the next HOP.
- Do not add an MTP-specific `accepted_count` wire field, a commit broadcast or a global barrier.

The related current contracts are the opaque generation contract in [protocol.md](protocol.md) and
the CPS ring flow in [architecture.md](architecture.md).

## Key conclusion

MTP's accepted boundary is already reflected in the `SequencePayload.position` of the next HOP
produced by the terminal. P4's `Sequence`/`Outcome` have no
position field — position is a value inside the staged adapter's own
`SequencePayload` (`apps/p4/layers/adapters/llamacpp/staged/adapter/src/protocol/sequence.inc.rs`),
and at the P4 boundary it travels only as opaque `state`/`forward` bytes.
In the normal CPS flow there is therefore no need to propagate the accepted count as a new field.
The real blocker is whether each memory backend can partially roll back a speculative suffix.

Building starts after the following two prerequisite gates are closed.

1. Fix the meaning of the staged adapter's `SequencePayload.position` in docs and tests.
2. Measure the runtime rollback capability of the target memory backend.

The current P4 event contract adds a third gate. `SequencePayload.n_tokens` is only the number of HOP
input rows; the current `HopComplete` carries one `Outcome` per sequence, an `Outcome` carries
one optional token, and `Continue` also carries only one token. It must therefore first be decided whether MTP emits
several external tokens in one lap, or processes several rows internally but projects them onto the existing
single-token events.

If any of A, B or C is not closed, do not proceed with removing `Some(1)`, rollback calls, multi-token event
projection or advertising
`mtp_execution`.

## 1. Position contract

State explicitly that the current numeric field has different authority at a middle stage and at the terminal.
The base meaning of the number is unified as "the position where the next HOP's input rows start". This value lives not in P4's
`Sequence`/`Outcome` but in the staged adapter's
[`SequencePayload.position`](../layers/adapters/llamacpp/staged/adapter/src/protocol/sequence.inc.rs),
and at the P4 boundary it travels only as opaque bytes in `Sequence.state`/`Outcome.forward`.

| Value | Meaning | Authority |
|---|---|---|
| `SequencePayload.position` of the received HOP | position where the current HOP's input rows start | authoritative input position of the current HOP |
| response `SequencePayload.position` written by the terminal | next HOP input start position computed by the terminal | advances by the number of rows the terminal actually committed to the target KV |
| response `SequencePayload.position` written by a middle stage | echoes the input position unchanged, because the middle cannot know the terminal result | not an authority on progress |

A middle stage must not advance the position on its own. Only the terminal knows this round's verification result
and computes the next HOP's position. This property is currently guarded by the
`a_middle_stage_preserves_the_global_decode_position` regression test
(`apps/p4/layers/adapters/mock/src/tests/core.rs:282`). This test no longer checks
P4-level `Sequence.position`/`Outcome.position` fields. It guards the same property by using the mock's own
opaque `encode_state`/`decode_state` helpers to check that the value carried in `Sequence.state`
is echoed unchanged into `Outcome.forward`. The test
name and meaning are still valid, and this property must also hold on the MTP multi-row path.

This contract keeps "the last written position" and "the next position to write" from being mixed up. Rollback
boundary computation is implemented only against this contract; it is decided by comparing the previous speculative end each stage holds
with the `SequencePayload.position` of the newly received HOP.

## 2. Memory backend capability gate

llama.cpp's memory support is judged from a runtime probe result, not from a static assumption.
The current upstream classification is as follows.

| capability | Meaning | Staged MTP judgement |
|---|---|---|
| `NO` | does not support sequence removal | cannot run |
| `PART` | can remove part of a sequence's suffix | default lazy rollback candidate |
| `FULL` | can remove only the whole sequence | possible through full checkpoint/restore, but expensive; excluded from this scope |
| `RS` | limited partial rollback of recurrent/hybrid state | requires verification of the `n_rs_seq` range and snapshots |

The relevant upstream basis is
`COMMON_CONTEXT_SEQ_RM_TYPE_*` and `need_n_rs_seq()` in [`common.h`](../layers/adapters/llamacpp/upstream/common/common.h),
and the
`common_context_can_seq_rm()` probe in [`common.cpp`](../layers/adapters/llamacpp/upstream/common/common.cpp).
The actual rollback range of recurrent memory is checked in
[`llama-memory-recurrent.cpp`](../layers/adapters/llamacpp/upstream/src/llama-memory-recurrent.cpp).
Upstream is a replaceable boundary, so no Linker-specific code is added to it.

The MTP/Eagle/DFlash/Dspark family may need an `n_rs_seq` sized to the draft ceiling.
Being classified as `RS` is not sufficient on its own; the actual speculative depth and restore results must be
verified on the target model.

## 3. CPS lazy rollback flow

Rollback is not a separate round trip or a global commit barrier. When the next HOP produced by the terminal
returns through the generating edge to the first stage of the ring, each stage cleans up
its own suffix at the moment its input arrives.

```text
terminal
  │ next HOP: position + KV-committed rows
  ▼
generating edge → stage 0 → stage 1 → … → stage N → terminal
                    │         │                 │
                    └─ each stage lazily rolls back its own speculative suffix on arrival
```

Each stage processes in the following order.

1. Compare the previous speculative end stored for the same sequence with the
   staged `SequencePayload.position` carried in `Sequence.state`.
2. Remove its own KV/state suffix left past the new position, or restore a snapshot.
3. Process the new HOP rows.
4. Update the speculative end that it wrote in this HOP.

This state must be per sequence, and parallel sequences must not cut each other's suffixes.
Rollback happens on entry to the next HOP, so no extra broadcast or global synchronization is needed.

## 4. Wire and row count

Do not create an MTP-specific field. Extend the `SequencePayload.n_tokens` already present in the staged HOP from
`Some(1)` to `Some(n)`. This is not a field that exposes MTP semantics; it is the
ordinary HOP input row count.

| Value | Meaning |
|---|---|
| `None` | legacy frame without the trailing field |
| `Some(0)` | corruption; rejected on both encode and decode |
| `Some(n > 0)` | actual input row count |

This change adds no trailing field, so the current exact trailing detection
does not change. The invariant that `n_tokens` is the only optional trailing field of `SequencePayload`
is pinned by a regression test, and adding any other trailing field is treated as a separate wire-format change.
Switching to a self-describing trailing format is out of this scope.

The current fixed point is
Decode `input.n_tokens = Some(1)` in [`hop.inc.rs`](../layers/adapters/llamacpp/staged/adapter/src/adapter/hop.inc.rs),
and the implementation phase replaces
the fixed 1 with the actual row count.
The encoding/decoding `Some(0)` checks are in
[`hop_kv.inc.rs`](../layers/adapters/llamacpp/staged/adapter/src/protocol/hop_kv.inc.rs).

The important boundary is not to assume that `n_tokens` equals the cardinality of external events.
`n_tokens = n` means the number of input rows this HOP consumes. That alone does not mean
that n `Outcome`s can be created or n `Reply::Token`s emitted. If the current event contract
is kept as is, the terminal needs a policy that verifies several rows internally but projects one lap's external result
onto the existing single `Outcome`/`Continue`. Choosing a policy that emits several visible tokens externally in one lap
requires a separate event/message contract change and
replay/index rules.

## 5. Terminal result and token accounting

Internally, the following four counts are kept distinct.

- `candidate_count`: number of speculative candidates MTP produced
- `accepted_count`: number of candidates confirmed by verification
- `committed_count`: number of rows actually committed to the target KV
- `visible_count`: number this HOP actually emitted into external token accounting

`accepted_count` is not a wire field. The boundary shows up in the terminal's next HOP `position`,
and a stage derives the rollback range from its own previous write end and the new position.
**Position is based on KV commit; token events are based on visibility.**
The terminal does not reconstruct the commit count from `accepted_count`; it records the number of rows actually written to the target KV
as `committed_count`. On the ordinary mismatch path this value is
`accepted_count + 1`, but if a terminating token such as EOS occurs within the accepted span,
there is no extra sample row, so the formula can differ. `visible_count`, on the other hand, is
reflected in the monotonic increase of `Reply::Token.index` and in ordinary `Done.generated` accounting.
The only exception is a `length` terminal for which staged native proved `position >= remaining`:
its `Outcome::terminal_generated` preserves the request bound as `Done.generated`,
and it is not a field that exposes candidate/accepted counts
([adapter boundary](adapter-boundary.md#terminal-length-accounting)).

Because of EOS, stop sequences, grammar, cancellation and similar causes, the whole accepted span may not be emitted
externally. So do not treat candidate, accepted and visible tokens as the same number;
verify the terminal's commit position advance and the external token event accounting separately.
- Tokens that were committed to KV but not yet emitted externally, as with a partial stop-string match, must not be
  dropped from the next HOP position.

`remaining` is handled as a local clamp in the last adapter. Do not replicate it
as an inter-stage wire field.

Verification of `HopComplete`'s `hop_id`, `deployment`, `expected` and sequence set follows the existing
P4 contract unchanged. The staged adapter reports `SequenceAcquired` first when prefill needs it,
and on release reports `SequenceReleased` before the corresponding `HopComplete`.
The order in which a node projects an `Outcome` into `Token`, `Done`, next-stage forwarding or `Continue`,
and the `event_seq`/retry rules, are under the authority of the external event contract in [protocol.md](protocol.md);
MTP's internal candidate/accepted counts are not exposed directly
as events.

## 6. Recovery and reconnection

When a restore or reconnection happens, the following state is discarded together.

- the terminal's memory of MTP candidates
- every stage's speculative KV suffix
- the speculative snapshots of recurrent/hybrid backends

After recovery, resume with ordinary decode. If only the terminal's candidate memory is discarded while ghost suffixes
remain in earlier stages, later verification can silently go wrong.

## 7. Build order

### Prerequisite gate A — position contract

- Fix the meaning of staged `SequencePayload.position` given above in code comments, docs and tests.
  P4 does not read that value, so the contract holds only inside the staged adapter.
- Verify middle-stage echo and terminal multi-row advance separately.
- Keep the regression test that preserves the middle position on the MTP path as well.

### Prerequisite gate B — backend probe

- Measure `NO`/`PART`/`FULL`/`RS` on the supported target models and memory backends.
- Verify partial rollback depth, recurrent snapshot restore and the speculative depth ceiling.
- If the probe result says execution is impossible, keep only the parser/ownership probe and do not advertise
  the execution capability.

### Prerequisite gate C — external event cardinality

- The current `HopComplete.expected` is a set of sequences, `outcomes` has one entry per sequence,
  and `Outcome.text` is everything that sequence emits to the requester in this hop. There is no longer
  a unit called a token at the P4 boundary. The sampled token lives in `initial_tokens` of the staged
  `SequencePayload`, which is a vector, so the "exactly one" constraint rests not on the field name
  but on the fact that the tail puts in only one per lap. MTP changes exactly that fact.
- State explicitly that extending `n_tokens` to multiple rows does not automatically make this contract
  a multi-token event contract.
- Before implementation, decide on one of two options: (a) an extension that projects several visible tokens, in order, onto the existing event/message
  layer, or (b) a projection that processes several rows inside an MTP lap and emits only the existing
  single-token event externally.
- For the chosen policy, pin with tests the accounting of `Reply::Token.index`, `Done.generated`, stop/EOS, replay and
  the position/remaining of `Continue`.

### Implementation

- Remove the fixed `Some(1)` from the Decode row count.
- Connect the terminal result to the generating edge and to token
  accounting according to the event cardinality policy chosen in C.
- Implement per-stage lazy rollback on entry to the next HOP.
- On restore/reconnect, discard all speculative state and then resume ordinary decode.
- Advertise the `mtp_execution` capability only for backends that passed A, B, C and real execution tests.

### Integration gate

Finally, the workspace tests, clippy, fmt and the fleet-level driver must pass.
Row counts, positions, rollback and reconnect across the whole ring must be verified together, so this gate
cannot be replaced by separate unit tasks.

## 8. Parallel build feasibility

To build quickly without bypassing the gates, first run only A and B in parallel, and once both results
are closed, decide whether to combine C and D into one or keep them separate. The lead agent owns the contract, worktrees and
integration, and each task agent is given a disjoint write set. The number of agents is therefore
not fixed from the start, because B's `PART/RS/FULL/NO` result determines the amount of rollback implementation and
the benefit of splitting.

### 8.1 Agent split

| Agent | Owned scope | Precondition | Deliverable |
|---|---|---|---|
| A — position contract | staged `SequencePayload.position` doc comments, mock middle echo/terminal commit tests | none | position contract patch and off-by-one tests |
| B — capability probe | `PART/RS/FULL/NO` probe, per-model evidence, capability judgement API | none | probe run results, rollback depth table, failure classification |
| C — event cardinality | external projection policy for `HopComplete`/`Outcome`/`Continue` and multi-row results | A, and a check of the current event contract | decision between keeping single events and changing to multi-token events, plus accounting tests |

A and B are independent of each other and can start immediately. C is decided after checking the current outer event handling and
the `HopComplete` projection in `protocol.md`; D waits for B's result.
Per the key conclusion and the prerequisite gates in §7, C and D
do not start MTP implementation until A and B are each **complete**. Do not start
removing `Some(1)` from A's draft alone, do not fix a rollback abstraction without B's result, and do not start
multi-token event projection without C's decision.

### 8.2 Runtime composition by B's result

After the lead reviews the deliverables of A, B and C, the scope of the runtime (D) is composed according to B's result.

| B result | D composition | Reason |
|---|---|---|
| `PART` | run D as a single adapter-runtime flow | the boundary of position-based suffix removal is narrow, so integration is faster than a separate runtime split |
| `RS` | D can be split internally into snapshot/depth rollback and the rest of the runtime | recurrent rollback greatly increases runtime work, so parallelizing is worthwhile |
| `FULL` | integrate D by default; split only if full checkpoint/restore is brought into actual scope | FULL is expensive, and the capability is not advertised in this default scope |
| `NO` | do not start D as MTP execution; integrate only capability rejection | do not proceed with a rollback implementation on a backend that cannot run it |

In other words, C owns Rust `n_tokens`, event cardinality, result projection and visible/committed accounting, and D owns
`StageRuntime` target/MTP state, draft/verify/accept, backend rollback and restore invalidation.
This role split, however, is activated only after A, B and C are complete. If B is `PART`, D is
merged into one flow; if `RS`, D is split internally to reduce wall-clock time.

### 8.3 Conflict prevention rules

- Only one agent modifies a given file. Two agents never edit a shared file at the same time.
- A owns only the semantic contract and mock tests, and does not modify the C/D runtime implementation.
- B owns the probe and evidence, and does not add Linker code to `upstream`.
- C owns only the wire row count and P4 event projection. It does not implement target KV deletion or sampler
  state.
- D consumes C's payload shape but does not modify Rust protocol files directly.
- Until the capability is confirmed, no agent makes `mtp_execution=1` the
  default.
- Each agent submits its changed files, test commands, unverified assumptions and remaining blockers as a short handoff
  record.

### 8.4 Integration order

The lead agent integrates in the following order.

1. Run A and B in parallel and complete independent verification of each.
2. Integrate A to pin the position contract and the mock regression tests.
3. Review B's real-model probe evidence, and decide the runnable capability and how C and D are
   composed.
4. First pin the event cardinality decided in C, then integrate the chosen C/D flow to
   connect `Some(1) → Some(n)`, visible/committed accounting, the capability gate and
   stage-local rollback.
5. Add integration tests that cover rejection, accepted EOS, stop-string buffering and
   reconnect/restore.
6. Advertise `mtp_execution` only after confirming that the capability report matches the actual probe,
   C's external event contract and the execution results.

Even when C and D are split according to a `PART`/`RS` result, D does not copy C's final implementation;
it consumes only the `SequencePayload` contract pinned in the docs. C's file changes therefore do not conflict with D's
runtime branch. If the combined C/D form is chosen, the lead owns the single change across both
boundaries.

### 8.5 Per-agent verification and the final gate

Each agent first runs only the fast verification for its own scope. When C and D are integrated,
the same agent runs the C/D verification below in order.

- A: mock position tests and protocol contract tests
- B: `validate-mtp-speculative-capability.mjs` and target model evidence
- C: protocol/adapter tests, legacy frame and `Some(n > 1)` round trip
- D: runtime unit, full-tail MTP, rollback/restore tests

After that, the lead agent runs the integration gate, which cannot be split up.

- `cargo test --workspace`
- workspace `clippy`
- `cargo fmt --check`
- staged server/adapter C++ build
- real ring verification, terminal → stage 0 → … → terminal, on the fleet-level driver
- comparison of tokens, logits, KV position and restore results against the baseline full model

Until the integration gate finishes, a parallel agent's "pass" is not treated as final completion.
No parallel agents were created at the current document-writing stage, and during the actual build all
work is done in a separate worktree that does not overwrite dirty changes in the same workspace.

## 9. Acceptance criteria

| Area | Required verification |
|---|---|
| position | middle echo, terminal multi-row advance, actual `committed_count`, prefill/decode boundary, off-by-one |
| row count | legacy `None`, `Some(1)`, `Some(n>1)`, rejection of `Some(0)` |
| speculation | all rejected, some accepted, all accepted, EOS inside the accepted span, remaining clamp |
| output | token index monotonicity, `Done.generated`, EOS/stop/grammar/cancel, stop-string buffering where `visible_count != committed_count` |
| event contract | `Outcome` cardinality per sequence, `HopComplete.expected` verification, `SequenceAcquired/Released` preceding order, single or multi-token projection policy |
| rollback | allow/reject for each of `PART`, `RS`, `FULL`, `NO`; per-sequence suffix isolation |
| recovery | discard speculative state after reconnect/restore and resume ordinary decode |
| CPS | forward direction terminal → generating edge → stage 0…, no per-lap global barrier |
| wire safety | invariant that `n_tokens` is the only trailing field, and legacy decode preserved |

## Related material

- [P4 protocol](protocol.md)
- [P4 outer/event policy](protocol-outer.md)
- [P4 architecture](architecture.md)
- [Current llama.cpp adapter summary](../llamaAdaper.md)
- [staged validation README](../layers/adapters/llamacpp/staged/scripts/validation/README.md)
- [MTP minimum-slice audit](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp-speculative-minimum-slice-audit.md)
- [MTP ownership probe](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp-auxiliary-ownership-probe.md)

## 10. Concrete adapter implementation details

Abstract P4 is responsible only for the HOP row count, position delivery and carrying the opaque cut-set.
The concrete `llamacpp/staged` adapter is responsible for all of MTP's actual semantics and state consistency.
The implementation crosses the following four boundaries, but the authority over MTP state lies with the C++ `StageRuntime`.

| Concrete boundary | Implementation responsibility | Owns MTP state? |
|---|---|---|
| Rust `StagedAdapter` | sets the HOP row count, frame round trip, projects results into P4 events | no |
| C++ protocol/server | `SequencePayload` validation and dispatch, per-sequence request serialization | no |
| C++ `StageRuntime` | target/MTP context, draft/verify/accept, sampler, KV rollback | yes |
| C++ KV bridge | durable KV save/restore/drop and runtime identity verification | persistent state only |

### 10.1 Rust adapter: turning the HOP into multiple rows

The target of the change is
[`hop.inc.rs`](../layers/adapters/llamacpp/staged/adapter/src/adapter/hop.inc.rs).
In the current Decode path, `input.n_tokens = Some(1)` fixes every decode to one row.
Change it to the following rules.

The current Rust adapter's result boundary must be preserved as well. The adapter turns per-sequence results into
`Outcome`s and reports them in one `HopComplete`. The node projects these results into the next stage's
cut-set, external `Reply::Token`/`Reply::Done`, or the next `Continue`.
`SequenceAcquired` and `SequenceReleased` are admission/release events and do not carry MTP candidates or
accepted counts. So even after multi-row input is introduced, do not sneak a candidate list into one `Outcome`,
and do not interpret the sequence set of `HopComplete.expected` as a row
set.

1. When building a new HOP, do not unconditionally overwrite the existing inbound `n_tokens` of `SequencePayload`.
   The adapter computes and states the number of rows it will actually send in this HOP.
2. Ordinary decode keeps using `Some(1)`.
3. Use `Some(n)` only when re-injecting an MTP-enabled terminal result into the next HOP.
4. Do not infer `n` from the hidden tensor's `dimensions[0]`. Verify the logical row count decided by the HOP builder
   together with the descriptor/payload count.
5. Allow `None` only as legacy input, and apply the per-phase default explicitly
  just before execution. In Decode, do not turn an empty row into `Some(0)`.
6. After C decides whether to keep the single-token external projection or introduce multi-token events/messages,
   align the chosen policy with the order of `HopComplete`/`Continue`/`Reply`.

On receiving the response, check the following.

- Is the result sequence id the same as the request sequence?
- If `n_tokens` is `None`, the legacy fallback applies; during MTP execution, is a silent
  downgrade to a single row prevented?
- Does `Some(n > 0)` match the actual descriptor/payload shape and the terminal output?
- Is the outcome position adopted as the next HOP position only when it is a terminal result?
- Is a middle result's position echo kept from being mistaken for global progress?

The current `outcome_from_result` projects one `OutcomeMetadata` into one P4 `Outcome`.
The MTP implementation must split this boundary clearly. If the current code contract is kept,
then even when several rows are processed inside MTP, the external projection must follow a one-`Outcome`/one-`Continue`
policy. Only if it is decided to expose multiple visible tokens externally is the cardinality of
`Outcome`, or of the service messages after it, changed separately.

- The cut-set keeps carrying multiple rows.
- For external tokens, either generate `visible_count` token events in order, or convert them into a multi-token event
  representation that the repository allows.
- `Reply::Token.index` increases contiguously from the previous last index.
- `Done.generated` adds the actual number of visible tokens, not the accepted/candidate count.
  A verified native `length` terminal, however, reports the request bound.
- Candidate rows after stop/EOS/grammar/cancel are not emitted as external events.

In short, do not force the candidate count or the accepted count into one `Outcome`. P4 event conversion
looks only at visible output and uses only the verified length bound for terminal accounting, and the rollback
boundary is delivered through the next HOP's position.

### 10.2 C++ protocol/server: transport validation and dispatch

The relevant structures are
`SequencePayload` in
[`protocol.hpp`](../layers/adapters/llamacpp/staged/server/src/protocol/protocol.hpp)
and the encode/decode in
[`protocol_hop.inc`](../layers/adapters/llamacpp/staged/server/src/protocol/protocol_hop.inc).

The validation to implement is as follows.

- If `n_tokens` is absent, mark the frame as legacy, but judge separately, from the server plan and
  runtime capability, whether it is an MTP execution request.
- `n_tokens == 0` is rejected as `InvalidSequence` on both encode and decode.
- For `n_tokens > 0`, check that it does not exceed `ProtocolLimits` or the loaded context's batch/sequence
  limits.
- Each sequence inside the HOP envelope has its own row count and position.
- Outcome metadata must be produced only at the terminal, and a middle server echoes the
  input position.
- Do not confuse the decoded row count with the actual number of tensor descriptors. The first dimension of a rank-1 hidden
  tensor may be the embedding width.

`Session::execute_hop` calls `StageRuntime::execute_hop` per sequence.
Do not make MTP candidate generation a global state shared by several sequences; keep it in runtime state
separated by sequence id. A rollback failure in one sequence must not truncate another sequence's KV or
reset its sampler.

### 10.3 StageRuntime state model

The current `StageRuntime` has `sequence_ids_`, `sequence_positions_`, `samplers_` and
`sampler_options_`. MTP execution requires extending this state as follows.

```text
SequenceRuntimeState {
    local_seq_id
    committed_position        // boundary confirmed by the target KV
    speculative_end           // exclusive end of the range this stage last wrote
    rollback_capability       // NO / PART / FULL / RS
    rollback_snapshot         // native checkpoint required for RS/FULL
    sampler_state

    // terminal-only state; middle stages must not derive or advance position
    next_input_position       // base + committed_count
    candidate_tokens
    candidate_count
    accepted_count
    committed_count
    visible_count
}
```

The actual type names can be chosen at implementation time, but the following invariants are fixed.

- `candidate_tokens` and the terminal-only accounting (`candidate_count`, `accepted_count`,
  `committed_count`, `visible_count`) are not wire state or inter-stage state.
- `speculative_end` can differ per stage, so storing only the terminal result is not enough.
- The table that maps sequence ids to local `llama_seq_id` is owned by the stage runtime, as before.
- Sampler state is kept per sequence; when the request options change, the existing sampler is
  discarded and a new sampler is created.
- `next_input_position` is managed only at the terminal and equals `base + committed_count` in §10.6.
  A middle stage neither computes this value nor advances the position.

### 10.4 Load phase: capability judgement before MTP execution

The current [`llama_stage_runtime.cpp`](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime.cpp)
allows MTP only as `mtp_ownership_probe` and rejects production execution.
When execution is opened up, split the load order as follows.

1. Create the model/context.
2. Determine whether the stage is the full tail. The only stage that owns the MTP auxiliary head and the target logits
   is the terminal.
3. Run a probe equivalent to `common_context_can_seq_rm()` against the target memory.
4. For `PART`/`RS`, test suffix removal or state restore at the actual draft depth.
5. For `FULL`, test whether a speculative checkpoint can be saved and restored to the confirmed boundary.
6. For `NO`, or if the probe fails, keep `mtp_execution=0` and return an explicit capability
   unavailable error.
7. Put `mtp_execution=1` into the capability report only if every probe passes.

The probe does not only check whether the API call succeeded. After decoding two or more tokens, it must remove the rejected
suffix and go as far as confirming that the logits/KV position of the next decode match the baseline from before the rejection.
Recurrent backends may have no position cells, so they must not be treated like `PART` without verifying
snapshot restore.

### 10.5 Separating ordinary HOP execution from MTP execution

The ordinary path in the existing [`llama_stage_runtime_hop.cpp`](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_hop.cpp)
sets the input tensor, builds a batch of `token_count` rows, calls
`llama_decode` once and then calls the terminal sampler once. Do not overload this path with MTP conditionals;
split it into the following two executors.

#### Ordinary HOP

- Install the input cut-set per descriptor.
- Build a batch of `token_count` rows.
- Continue the sequence KV position from memory.
- Call the ordinary sampler only at the tail.
- Return one visible token and the next position.

#### MTP HOP

- Confirm that this is the tail stage.
- Map the sequence ids of the target context and the MTP context to the same logical sequence.
- Compute the candidate generation ceiling from the current committed context.
- Generate candidate tokens in the MTP context.
- Compute the confirmed token and the candidates in the target context in a single verify decode batch.
- The sampler accepts or rejects candidates in order and stops at the first mismatch.
- Commit the accepted target state and clean up the rejected suffix in both the target and MTP contexts.
- Project only `visible_count` into token events, and compute the next HOP position from the actual target KV
  commit count (`committed_count`).

`execute_mtp_hop` in the current [`llama_stage_runtime_mtp.cpp`](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_mtp.cpp)
is an ownership/test path with a hardcoded prompt, `seq_id = 0` and a draft ceiling of 1,
and it is not connected to ordinary `execute_hop`. The production implementation must not call this function
as is; it must be promoted to an execution routine that takes per-sequence runtime state and HOP input.
Until this boundary is closed, keep the current `mtp_execution=0`.

### 10.6 Concrete handling of the MTP generate/verify loop

With `n` MTP heads, the logical flow of one lap is as follows.

1. Fix the context end confirmed in the previous lap as `base`.
2. The MTP context produces up to `n` candidates. `draft-max`, the model layer limit,
   remaining and the context limit are clamped at the terminal.
3. Put the confirmed input starting at `base` and the candidate rows into the target verify batch.
4. Decode the target once to get the required logits rows.
5. The sampler compares the candidates in order.
6. Everything before the first mismatch is accepted, and the target sample at the mismatch position is included as a new confirmed
   token.
7. If EOS/stop/grammar/cancel occurs, discard every candidate after that point.
8. Keep `accepted_count`, `candidate_count`, `committed_count` and `visible_count` as separate internal
   results.
9. Keep only the accepted target KV and sampler state as the committed state for the next lap.
10. Record the number of rows actually written to the target KV as `committed_count`, and compute the
    staged `SequencePayload.position` passed to the next HOP as `base + committed_count`. The number of visible token events
    is accounted separately. Whether this internal result is projected onto the current external single-token contract
    or extended to multi-token events/messages follows the decision of prerequisite gate C.

The key optimization in `llama.cpp` is "one decode + at most N+1 sampler calls". An implementation that calls a separate
`llama_decode` per candidate violates this contract and loses the benefit of MTP.

The state that the MTP context advanced during candidate generation is also managed separately. Do not assume that rejected candidates
are simply overwritten by the next token; the target context, the MTP context and the sampler must all be restorable to
the same accepted boundary.

### 10.7 Per-stage lazy rollback implementation

`rollback_hop_batch()` in the current [`llama_stage_runtime_hop_batch.cpp`](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_hop_batch.cpp)
is the path that cleans up the native sequence slots newly created in a failed multi-sequence HOP.
It is not MTP speculative suffix rollback. When implementing MTP,
do not reuse the existing HOP failure cleanup as the success path for target/MTP state rollback after candidate rejection,
and do not advertise the two under the same capability.

Every stage runs `prepare_hop(sequence, position)` just before processing the next HOP.

```text
if no prior state:
    initialize committed_position = input.position
else if input.position < committed_position:
    reject as position regression
else if input.position < speculative_end:
    rollback suffix [input.position, speculative_end)
    committed_position = input.position
else if input.position == speculative_end:
    committed_position = input.position
else:
    if phase == Decode:
        reject as position gap corruption
    else:
        verify that the prefill gap is explicitly allowed
        committed_position = input.position

decode current rows
speculative_end = input.position + rows_written
```

`speculative_end` and `position` are both exclusive boundaries that denote the next input position.
They therefore map directly onto the `[p0, p1)` rule used by llama.cpp's `llama_memory_seq_rm(seq, p0, p1)`,
and are never converted to inclusive values partway through the implementation.
Each backend adapter implements the following behaviour.

- `PART`: remove the suffix with `llama_memory_seq_rm(memory, seq, rollback_start, -1)`, and
  check `llama_memory_seq_pos_max` after removal.
- `RS`: call the limited rollback/snapshot path the backend provides instead of position-based deletion,
  and check that the rollback depth is at most `n_rs_seq`.
- `FULL`: keep a checkpoint taken before speculation starts, and after a full sequence restore reinstall only the required
  confirmed prefix. Do not imitate a partial cut. This approach is possible but
  expensive, so the capability is not advertised in this default implementation scope.
- `NO`: do not run the new HOP; return a capability error.

A new decode must not proceed after a rollback failure. The stage's memory and `sequence_positions_`
would then be inconsistent, so isolate that sequence in an invalid state and either send it to the recovery path
or fail the request.

A HOP retransmission with the same `input.position` is treated as an idempotent retry, without a new wire field.
That is, when the same position arrives again it is not silently ignored as a duplicate; the remaining
`[input.position, speculative_end)` suffix is rolled back and the same HOP is re-executed.
There is no separate hop identity on the wire, so no hop-id-based duplicate rejection is done in this scope.

### 10.8 Separating durable KV restore from speculative state

[`llama_stage_runtime_kv.cpp`](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_kv.cpp)
currently verifies the durable KV manifest, model identity, stage range and token position.
The MTP implementation does not treat durable KV and speculative state as the same thing.

The current checkpoint/restore is also a recovery boundary centred on the target prompt/KV; it is not a contract that restores the MTP draft context and
candidate sampler state together. So in the current code, a successful restore alone
is not enough to resume MTP. Unless MTP state is made durable,
speculative state must be invalidated as below and execution must resume with ordinary
decode.

- Even if a durable restore succeeds, MTP candidate memory and sampler speculative state do not
  automatically become valid.
- Right after restore, align `sequence_positions_` with the manifest's token position.
- Delete each stage's in-memory speculative suffix and snapshots.
- Re-initialize the terminal's MTP context from the new confirmed prefix as well.
- Start the next HOP with ordinary decode, and generate new MTP candidates only after its result.
- Run the graph on the restored state only after `llama_synchronize`.

### 10.9 Output, errors and observability

The concrete adapter must preserve not only computed results but also the meaning of failures.

Required error conditions:

- MTP requested on a stage that is not the terminal
- the capability probe judged execution impossible
- position regression or rollback range mismatch
- mismatch between the row count and the batch/output descriptor count
- mismatch between the current event cardinality policy and `HopComplete.outcomes`, `Outcome.text`, staged
  `SequencePayload.initial_tokens` or the external `Reply` projection
- candidate/accepted/committed/visible accounting that is negative or exceeds the context limit
- staged `SequencePayload.position` inconsistent with the actual target KV commit position
- the sampler returns `LLAMA_TOKEN_NULL`
- after restore, the actual KV max position is inconsistent with the manifest
- an attempt to propagate one sequence's rollback failure into another sequence's state

Observability records at least the following.

- sequence id, hop id, phase
- input position, candidate count, accepted count, committed count, visible count
- rollback capability and rollback start/end
- committed position per target/MTP context
- stop reason and whether reconnect/restore happened
- HOP row count and actual batch row count
- projection result of `hop_id`, the `expected` sequence set, event kind and external token index

Candidate tokens themselves and raw prompt text follow the existing sensitive-data policy and are not written to the default log.
The current telemetry records row counts and elapsed time; add the accounting and
rollback results to it, but do not record token content.

### 10.10 Concrete adapter test order

Build the implementation up through the following layers.

1. **protocol unit**: `None`, `Some(1)`, `Some(n)`, `Some(0)`, trailing invariant,
   multi-sequence envelope.
2. **runtime unit**: position state machine, `PART/RS/FULL/NO` rollback adapter,
   sampler reset, restore invalidation.
3. **full-tail MTP test**: change the current ownership test to production-shaped sequence/HOP
   input and verify the draft/verify/accept counts and position.
4. **multi-stage cut-set test**: confirm that the row count and the
   tensor cut-set are preserved across terminal → stage 0 → … → terminal.
5. **rejection matrix**: all rejected, first candidate rejected, rejected in the middle, all accepted, EOS/stop.
6. **reconnect/restore**: after discarding both the speculative suffix and the MTP candidate state, confirm that
   ordinary decode produces the same logits/tokens as the baseline.
7. **real model E2E**: for each PART/RS backend, compare tokens/logits and
   KV position against the baseline full model, and confirm that the capability report matches actual behaviour.

This scope is the actual implementation workload of the concrete adapter. The fact that the wire change in the abstract P4 document is small
does not reduce this internal state, memory, sampling and recovery work.

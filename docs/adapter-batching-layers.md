# Adapter batch layering contract

> Document status (2026-09-06): **Domain contract, distinct from implementation**. Read it as the contract and goals of the owning domain, but do not treat it as implemented. Where it conflicts with the current development order, follow the explicit hand-over recorded in the roadmap.
> Current goals, status and ordering follow the [execution roadmap](distributed-batching-roadmap.md); document authority and reading paths follow the [document map](document-map.md).

This is the target layer contract for the llama.cpp adapter consuming its own
queue in batches. It does not mean that all of L0–L5 in this document are
implemented. The current code status and phases are owned by the
[distributed batching roadmap](distributed-batching-roadmap.md), and execution
verification by the [verification convention](distributed-batching-verification.md).
OUTER decides the model, topology, SLO, request arrival and snapshot triggers;
the adapter composes eligible rows and batches. No llama-specific batching/KV
rules go into the P4 core.
The allowed dependencies, types and build boundaries across P4 as a whole and
native/llama/backend are owned by the
[layer isolation contract](layer-isolation-contract.md). L0–L5 below are the batching semantics inside it.

## Past observations — not a confirmation of current status or bottlenecks

| Observation | Value | Implication |
| --- | --- | --- |
| Sample where step time did not depend on row count | 12.8 rows 105.1ms, 18.2 rows 96.1ms | Fixed-cost candidate; cause unconfirmed until transfer/compute/sampling/waiting are broken down |
| cut-set width per hop | gemma-4: 31/27/23 tensors, 81 transfers per step | Per-model constant. 1 for the Qwen family |
| Mixed physical batches | 0–2 out of several thousand at the time | Observation of membership merging; pipeline depth is not judged from this alone |
| Per-node KV (same n_ctx) | 173.5 / 63.3 / 157.7 / 126.1 MB | Per-cell cost is per node. The most expensive node is the bottleneck |
| compute buffer | 1,412MB@ubatch512 ↔ 386MB@128, 8× the KV | Batch width is the knob that dominates VRAM |
| Slot reuse with 40 requests | `output token positions are not contiguous` | A target for the ledger's invariant detection. Cause not identified — identification and fix are plan P1b |
| KV persistence capability | `kv=0` (`--kv-root` not set) | Persist/Restore is implemented but disabled |
| SWA V allocation | With v_trans, a width of 256 is reserved as 512 | Per-layer over-allocation, a separate fix target |

## Invariants every strategy must keep

1. **Residency/dependency precondition**: before row (s,p) runs on a stage, that
   stage's required prefix KV and auxiliary state must be valid. Concurrent flight of several prefill fragments
   must prove the per-stage execution order of the preceding fragments; it does not mean the whole pipeline must be drained before every row is issued.
2. **No submission without proof**: do not assume KV was preserved after an execution failure.
   Dirty/uncertain native results follow the isolation/recovery contract. Rolling back only a local counter after a speculative attempt is forbidden.
3. **In-flight invariance**: bind the submitted membership and positions to the issue record. Do not reclaim
   resources without settlement/release evidence of completion, cancellation or failure. Persist does not substitute for release evidence of in-progress compute.
4. Ready decode must get bounded service, but not everyone beyond capacity can be put into every batch.
   Ordinary decode has 1 dependent row; the width and atomicity of speculative/verify follow the capability and the actual proposal.
5. Verify/replay atomic windows are processed only in agreed units, and **dependent follow-up work on the same sequence** is forbidden until they are resolved.
   This is not a rule to block unrelated sequences with a whole-pipeline barrier.
6. Memory families that require `equal_sequence_ubatch` keep equal widths. Do not
   unconditionally put 1 decode row together with a long prefill so that the whole prefill width collapses to 1.
7. A prefill row must have destination cells on every owning node. The budget is
   the remaining cells of the tightest node.
8. Restore is published as runnable only after verification/barriers on all stages. Convergence after a partial failure follows the store convention,
   and distinguishes quiescence of the target sequence from the actual exclusivity requirement of the native context.
9. Check engine/model/LoRA/shape compatibility within a batch. Relaxing the sampler options that the current `compatibility` groups more strictly
   first requires per-sequence sampler independence and normal-output tests.
10. Execution and return are bound to the load/sequence generation. Whether a persistent record may be reloaded
    is decided by the store identity and compatibility matrix, which are separate from the execution generation ([store convention](kv-state-store-convention.md)).
11. **Snapshot consistency fence**: Checkpoint, Persist and Fork run only at a
    stop point where the target sequence has no in-flight rows and all stages are settled —
    a snapshot exported mid-batch is a wrong answer with an ambiguous position. Cache operations
    compete with the step schedule as exclusive barriers; the scheduler preserves the OUTER command
    order but chooses the insertion point (after the current step ends). The evidence for a stop
    point is **a per-stage `SequenceQuiesced` attest**, not fragment credit
    — a credit return means "the peer has taken it over", not evidence of compute/KV completion
    (plan.md §3; O9).

## Layers

```
L0  Queue        (existing) arrival and hold. No policy.
L1  Ledger       single source of truth for ID mapping, residency state and cell accounting
L2  Accept/hold  request acceptance/reservation and execution of OUTER persist/restore commands
L3  Compose      per step: demand → allocation. Strategy module per model family
L4  Prove        shape proof before submission + membership capture (strengthens existing)
L5  Transport    (existing) cut-set transport, downstream replay. Efficiency target
```

### L1 Ledger (ledger)

The single source of truth about what is where. All other layers read the
ledger, and only the ledger writes state transitions.

- ID mapping: the external request ID, SessionKey, adapter incarnation and per-node slot are separate axes.
  SessionKey is conversation continuity; an incarnation is the ownership of one execution. The late-message problem is not
  worked around by forbidding reuse of the same request ID and slot after completion. Follow the execution ownership contract below.
- Per-stage residency state: `Resident{pos} | Persisting{op} | Persisted{pos,
  manifest} | Restoring{op} | Absent | Inconsistent`. Each stage's completion frontier and in-flight interval are kept together.
  In a normal wavefront, per-stage pos can differ. The pipeline is not serialized by requiring all stages to agree on pos at every issue.
  What is checked is whether the prefix/auxiliary state needed before that stage runs is valid and whether the order of preceding work is guaranteed.
- Cell accounting: binds the derived/measured per-cell cost for each model, cut and backend/layout to used/reserved/free.
  Even when recurrent memory is not token-cell based, auxiliary state/buffer bytes are reserved separately and never interpreted as 0 memory.
- Token history (or per-position hash): the basis for the LCP judgement between a re-requested prompt and persisted KV.
- Sequence slot lifetime: no reassignment before release is settled on all stages. The earlier 40-request
  position discontinuity is a target for reproduction and cause audit, not causal evidence that this rule was missing.

#### Publisher of approved output

In the current adapter OUTPUT contract, the tail returns the native result to the head, and OUTER output is published only after
the head's ledger validation and commit. The envelope source of OUTPUT is therefore the **configured first endpoint**, which owns settlement.
OUTER checks that whole endpoint, including agent address, node ID and node generation, together with load/session/request,
target/return route, correlation and position. The tail is not also accepted merely because it is a configured node too.
Native compute location and output approval authority are kept distinct; the broker neither interprets llama tokens nor changes the source.
Returning to direct tail output without a separate head approval receipt is a bypass of this contract.

Producer/consumer regression binds wire fixtures with the same meaning to the real paths on both sides. A test that only consumes a fixed
fixture, without also checking the output the current worker produces, does not approve boundary agreement. The conditions and negative tests
for dropping unstable envelope fields from the projection follow verification convention T20. This source check by itself is not a full
verification of user receipt ACK or of model quality, quantity or release membership.

#### Checking OUTER's sampled output budget against fresh-prefill observations

`tools/event-drive/src/run/output_budget.rs::validate_output` applies the submitted max_tokens to the number of sampled
OUTPUTs. An EOS with empty text counts as one; a terminal is required at the limit, and length holds only exactly at
the limit. stop/eos may end early within the limit, and other termination strings are not approved automatically.
`inference.rs::drive` checks before appending to the request's response/token list. Final acceptance also re-checks the whole preserved
outcome, and a limit violation is never hidden by truncating the response or deleting the EOS. An output consisting of a single empty EOS,
which is valid at the protocol level, does not satisfy the separate quality gate of a non-empty normal response/minimum generation amount.

The current event-drive runs new PREFILL requests that start at position 0. `inference_evidence.rs::apply_observations`
aggregates approved observations for different prompts per request and checks them against the first OUTPUT position. The same payload
with the same observation ID is counted once, and reusing the same physical execution under a different observation ID is rejected.
The artifact row count is installed only after validation of all request candidates and the checked sum are complete. Observations that arrive after OUTPUT
are also allowed if they precede the completion/release boundary, but if at that boundary an observation is missing or differs from the position, no success artifact is returned.
execute does not sum again. The optional common expected_prefill_rows is an additional workload condition, not a replacement for this check.
This observation is the fresh-prefill workload reported by the head; it does not substitute for separate native tokenizer/KV proof, the origin for resume/Restore,
a real-time OUTER user delivery ACK, or the separate release membership ledger below.

#### Release-completion authority and per-owner notification — only normal termination in the current run is partially implemented

`worker/release.rs::Worker::tail`/`released`, `worker/effects.rs`, `completion.rs` and OUTER's
`run/inference.rs`/`release_ledger.rs` were migrated together. What follows is the contract of a later uncommitted working tree for **normal sampled
stop/eos/length termination within the lifetime of the same run**. Failure/Cancel without OUTPUT, restart/reconnect/durable delivery,
and full multi-OUTER observation are not complete. The P4 broker does not own these model semantics.

**Current SESSION wire** (`commands.rs::SessionCommand`, `worker/control.rs::Worker::session`,
`worker.rs::Worker::require_stage_source`, later uncommitted working tree):

- The SESSION/SESSION_READY content-type is **v4**. SESSION requires `load_generation`, `session_id`,
  the full ordered `stages: [{agent,node,generation}, ...]` and the receiving node's `stage_index`.
  The separate role/first/next declarations were removed, and top-level unknown fields and the old v3 command are rejected.
- The node address, a nonzero generation, duplicate-free identities and a valid index are checked, and installation requires the full endpoint at the index
  to equal the actual worker and the envelope target. Different generations of the same agent/node cannot be
  declared as different stages at the same time either. A different declaration for an already installed session is rejected.
- first/previous/next/terminal are derived only from the installed order. The source of PHYSICAL/RELEASE/SETTLE must be
  the full endpoint of previous, the source of TAIL/RELEASED/SETTLED must be the full endpoint of terminal, and the target must be the actual worker.
  The checks run before any ledger/KV/slot/output change. In particular, the head's next and terminal differ in a 3-stage pipeline.
- `tools/event-drive/src/run/mod.rs::session_events` is the production path the real execution loop uses, and it
  sends every node the same order and its own index. Per-receiving-node validation alone does not prove agreement that the whole fleet
  received the same declaration, user/network authentication, or freshness after a process restart.
- The current stage path supports only **2 or more** stages, which need a separate head and tail. This limitation means no new single-stage implementation
  exists; it is not a placement policy to reduce the number of nodes per device or the model/KV nodes that are needed.

**Current OUTER wire and processing order**:

- The OUTPUT content-type of `ApprovedOutputPayload` is **v4**. It adds `submission_event_id`, a nonzero `incarnation`
  and an optional `release_operation_id` to the existing OutcomePayload fields. Only terminals carry a
  nonzero operation; nonterminals carry none. The head decides the operation when it builds RELEASE, and it
  validates the whole candidate before output. OUTPUT approval by itself is not completion of KV release on all stages.
- The OUTER notification is `ReleaseReceipt {load_generation, session_id, members}` of the separate **release-receipt-v1**.
  A member is `{request_id, submission_event_id, sequence_id, incarnation, operation_id}`; an empty list,
  a duplicate request/slot, a wrong identity and unknown fields are rejected. The internal RELEASED v4 is still an inter-stage
  ReleaseCommand ACK, not an OUTER receipt. The scalar-only DTO was removed, and old OUTPUT v3 and
  scalar/internal ACKs are not consumed as current OUTER completion proof.
- The real `send_wave` registers the PREFILL event ID it will send **before the send**. A send failure is a failure of that run and
  is not rewound to the same attempt/sequence. For a head PREFILL, the head checks that the source equals the OUTER of the original return_route and
  that the target is the actual head, before remembering/accepting it. This field check is not called peer authentication.
- OUTER pins slot/incarnation with the first OUTPUT that passes the existing route/budget/position validation, and keeps the release member
  expectation only from the terminal OUTPUT. A receipt that arrives first is rejected. Ordering is not hidden behind a new unbounded
  reorder queue. New releases are applied only after the whole receipt has been checked.
- Redelivery of the exact members in a fresh envelope yields **0** new releases. Redelivery with the same event ID is rejected under the existing OUTER
  envelope policy. A modified replay, and A valid/B invalid, are rejected in full. The correlation in the current event-drive
  is the submitted request ID and must belong to the receipt's members. The generic adapter preserves any valid
  correlation, so this OUTER implementation's choice is not promoted to grammar for P4 as a whole.
- `RequestArtifact` records the actual send attempt, the expected members kept from the terminal, and whether the receipt was applied.
  The match check in final acceptance is a consistency re-check of the preserved artifact. It is not a full re-validation of the raw receipt
  or an external signature/authentication proof, and it does not replace the checks on the real consumption path.

- **Submission authority**: OUTER stores the request attempt it fixed before sending, and the head binds it to the original submission.
  A durable session_key, a request string, an empty slot or the first receipt received is not attempt authority. Even when the current envelope
  event_id is used, it includes the issuing OUTER's full endpoint/connection generation and the load/session scope.
  Reusing the same connection generation and sequence in a new Sender has no restart freshness, so
  until that lifetime problem is solved, the record states that only binding within the current run is proven.
- **Compute/release authority**: the slot/incarnation and release operation that the head issued and kept are bound to the actual terminal approval.
  OUTER's expected release identity is fixed at the submission/approval boundary, before the release receipt. A check that installs the receipt's
  operation as its own expected value is forbidden. The handling contract for the order in which the release notification arrives before the terminal
  is also stated explicitly; a count is never incremented first and validated later.
- **Stage evidence**: internal control/ACK is bound to the predecessor/successor/first/terminal
  endpoints of the ordered pipeline fixed independently by SESSION, and to the load/session generation. The terminal is not newly registered by reading
  the sending source of the last ACK. A configured middle or an external sender cannot substitute for all-stage settlement evidence, even if it knows
  the pending members exactly. The endpoint check is logical role verification, not a replacement for transport authentication.
- **Membership**: a valid receipt carries an explicit set of request attempts, and the consumer checks it
  atomically against the submission/terminal expected set. A duplicate of A does not become completion of B. If even one member is unknown, stale or modified,
  the completion count, slots, reservations and effects of the other members are not applied either. Valid control/receipts that group several requests are kept.
- **Routing/effect lifetime**: before a request is removed from resident, the pending release keeps the original ReplySpec and the submission
  reference. Only the small identity/route metadata needed is extracted; the whole long prompt payload is not held again
  while waiting for release. Notifications go only to that owner's full route/correlation/deadline. Native release
  batching across several owners differs from OUTER notification grouping, and notifications are not funnelled to the route of a single physical base. Each notification intent
  is kept together with the state commit, and any unpublished part must remain after an emit failure. A notification failure is never turned into success,
  or into a new native re-release request, because the KV has already been released. Enqueue success and OUTER ACK are separate.

`PendingRelease` keeps only the actual RELEASE identity and the original Envelope/ReplySpec. It does not hold the prompt payload
again. After the source/member/admission of all ACK candidates have been checked against the original source/target/return_route/
correlation/deadline, receipts are grouped **only when the whole ReplySpec is identical**. Even for the same OUTER, a different
correlation/deadline means a separate notification. Slot return, pending removal and the notification intent are committed;
on emit failure, intents not yet published are kept and fenced. Notifications already published are neither reverted nor released natively
again. If the pre-commit ID obligation check finds insufficient headroom or an overflowing sum, the whole return candidate is
rejected and the request, ledger, reservations and effects are preserved. When publishing already committed effects, Full means waiting on and retrying
the same outgoing item, while Closed or a post-hoc ID issuance failure means keeping and fencing the unpublished intent. Neither is
interpreted as partial application of a pre-rejection or as reconnect recovery.

Freshness for recreating a Sender within the same connection generation, or for restarting a Worker/load, does not
exist yet. Several OUTERs using the same request_id concurrently is also not supported within the current request_key scope.
These, together with approval of Cancel/failure without OUTPUT and a bounded outbox/graceful drain, remain target contracts.
Verdicts/counterexamples are owned only by the verification convention, and work order only by the current roadmap.

Observations of the same multi-OUTER batch also need separate binding. Before the migration, `worker/observe.rs::Worker::emit_batch_observation`
sent the observation of all requests to every ReplySpec, and `inference_identity.rs::InferenceIdentity::observation`
rejected requests outside its own submission set. `emit_stage_span` sends only to the first owner. Per-owner routing
of OUTPUT alone is therefore not taken to establish multi-OUTER observation. The **target contract** is to distinguish explicitly between the permitted
request projection for each full route and the total workload of the physical execution. External requests are not silently ignored,
and measurement is not distorted by replacing the total row count with the owned row count. Each route receives observations of its own requests, and the declared measurement
receiver receives the per-execution total statistics once. The amount produced by duplicating the full span for each request of the same route
is not counted as new compute. This stays incomplete until the producer and consumer of the projection/statistics version pass together.

#### Completing per-owner observation and issue evidence — contract and per-version migration

Before the migration, `inference.rs::drive` exited right after receiving the last terminal and the release notification, while the head/downstream
can send observations after publishing Forward/TAIL. A normal observation that arrives late can therefore be missed. Also,
checking a stage span for each execution of the received head observations cannot, by itself, detect **an execution whose head observation and span
are missing altogether**. The following contract closes both gaps together. OUTPUT v4 and the old observations
do not implement it. For the wire migration in the later working tree see the version section below; for verification status see the latest roadmap record.

- The physical totals of rows/phase sums, logical width, request/sequence counts and RPC time stay global values,
  and the `owned_requests` projection of each full OuterEndpoint is kept separate. The head attaches the original submission event ID and slot/incarnation
  after checking them against its own RequestState. correlation/deadline is a carrier chosen deterministically from that route's actual original ReplySpec,
  and it does not stand in for the identity/deadline of every other owned request.
- The owner of a downstream span is reported by per-execution request/slot/incarnation and joined with the trusted head's
  original submission binding and the approved OUTPUT. No upstream reporting policy is put into the native RowOwner or the P4 envelope.
  Only a new native execution creates a span, and a cached replay is not counted as new compute.
- The issue ledger holds a **fixed-size issued-work witness** per request attempt. For every logical issue actually approved,
  the count and a SHA-256 chain are updated. The input binds a versioned domain, the head/OUTER endpoints,
  load/session/original submission/request/slot/incarnation, the logical ordinal, the set of physical executions actually used for that request,
  and the phase/exact position interval list/row membership. Intermediate
  positions are not omitted in favour of min/max/count alone. Time, receive order, JSON object field order and platform hashes are not identity.
  The byte encoding and sort order are fixed with independent literal positive and negative vectors before the wire migration, and a hash name is
  not added without a canonical encoding. Owned-membership evidence does not authenticate the total RPC time.
- Gaps in the ordinal caused by issues that do not involve the request are normal. The computation uses a strict per-request increase and
  the canonical physical/member order within the same issue. The number of fragments is not confused with the number of logical issues.
  Normal position reuse by Verify/Replay is distinguished by phase and ordinal/execution. A count/phase sum alone
  cannot detect the substitution of a different execution/range with the same count, so it is not sufficient binding.
- In the head's `accept_prepared_issue`, all split/owner/witness candidates and overflow are checked in full **before the ledger commit**.
  The small witness is installed on the already prepared request candidates, and the flight/request state is settled together in the non-failing section.
  plan, native start, failure, Uncertain and observation send success do not increment the count. The evidence is not implemented by
  re-copying the past execution list/prompt at every issue.
- The final witness is bound to a **new version** of the normal terminal approval OUTPUT. A separately sent observation must not set its own expected
  count/digest, and the producer/consumer are migrated together with the existing output/release authority chain.
  The current v4 is not silently extended, and after-the-fact evidence is not attached to old captures. This hash is an integrity check, not network
  peer authentication, a durable outbox, restart freshness or a KV stop-point proof.
- Every observation receiver is validated in advance and then kept as a small effect intent. Unsent observations are not dropped because of
  Full/Closed/number exhaustion; the existing fence/retry contract applies. native is not re-run because of an observation failure.
- OUTER completion requires not only terminal/release but also the witness and all-stage coverage of the executions it owns, all holding together.
  Missing is collected until the existing overall deadline, a conflict is rejected immediately, and only Complete candidates are applied at the end.
  Reverse arrival order of head and stage observations is allowed, and repeated full-history re-checks are avoided. Errors are never approved by turning them into null/empty statistics.
- A span's identity is the configured stage, load/session and the canonical set of fresh executions. Time is not a key but a body
  to be checked. An exact redelivery adds nothing; a different body with the same identity, or an execution overlapping another group, is
  a conflict. The required coverage is limited to executions owned by that OUTER; B-only compute is not required of A.
- The scope of one OUTER's data is **owner-visible physical work**. Data that lacks B-only batches is not called the fleet-wide
  total cost/utilization. When several owners' data are combined, the same physical compute is still aggregated only once.
  Comparisons of stage RPC spans with actual GPU time within a verified clock error range are distinguished from unverified cross-host comparisons.

Execution order is owned only by the current phase of the roadmap; negative, delay, production/consumption and cost tests follow verification convention T20/T25/T57/T58.

##### Internal issued-work v1 — implementation scope of the 2026-09-07 working tree

The internal approval evidence of the contract above was implemented first in `v2/issue_witness.rs::IssueWitness` and `node/state.rs::AdapterState::accept_prepared_issue`.
It is a fixed-size value per request attempt in L1, and policy, the neutral P4 core and native/llama/backend do not
update the evidence independently. The production dependency `sha2` was added only to the concrete staged adapter. This internal extraction
is not a native ABI change in itself, and later exposure uses the separate wire version below. For the source baseline and run results, see the evidence record.

The canonical input is as follows. This section owns the encoding, and other documents refer to it.

- All integers are little-endian with explicit widths, and a string is its `u32` UTF-8 byte length followed by the raw text. NUL/empty identifiers,
  invalid endpoints and generation/incarnation 0 are rejected. Addresses use the representation for which the P4 `Address` display→parse round trip
  is identical; DNS resolution, case folding and different IP notations are not treated as equivalent.
- The seed is the SHA-256 of `P4_ISSUE_AUTHORITY_V1` followed by 1 NUL byte, then the head address, node, generation(u64), OUTER address, channel,
  connection_generation(u64), load(u64), session, request, original submission event ID, sequence_id(u32) and incarnation(u64),
  in that order. The original return_route/source/target and the ReplySpec's route/correlation/deadline are checked as well.
  Matching input fields is not peer authentication.
- An issue is `P4_ISSUED_WORK_V1` followed by 1 NUL byte, the previous digest (32 bytes), the next per-request count(u64), the logical
  ordinal(u64), the execution count(u32), and for each execution its ID(u64), row count(u32) and, for each row, phase(u8) and position(u32),
  in that order. Executions are sorted by ID ascending, and rows by `(phase, position)` ascending. phase is
  Prefill=0/Decode=1/Verify=2/Replay=3. Sorting on both sides keeps the received array order from becoming authoritative.
- Within one issue, a 0/duplicate execution, an empty execution/row set and a repeated `(phase, position)` are rejected. The per-request ordinal
  increases strictly, and gaps from issues without the request are allowed. Verify/Replay position reuse across different issues is
  normal. A different intermediate position or execution substitution that keeps only the same count and min/max position also yields a different digest.
- The state itself is an **80-byte Copy value** of seed32+digest32+count8+last_ordinal8. This does not mean that the whole Option/RequestState
  is 80 bytes. Only the current issue's owner rows are collected, validated and sorted, and past execution history is
  not stored. The cost of prompt clones in the existing RequestState/plan is not solved by this.
- `accept_prepared_issue` builds the witness candidates of all requests first and installs them on the requests after flight registration succeeds.
  A lower-priority owner error, overflow or flight registration rejection preserves both committed and prepared candidates.
  prepare/begin/Uncertain, selector bookkeeping, settlement and delivery retries do not increment the witness. Calling only the low-level
  `register_issued_batch` directly does not create request evidence.

`validate_submission_identity` checks the common format of the original submission in PREFILL before tokenization, session key recording and admission.
The same check is reused at ledger approval. A witness is not generated in advance by filling a not-yet-assigned slot/incarnation
with fake values. After the adapter rejects a NUL identifier, normal requests on the same worker continue.
Normal Unicode and a separate correlation ID are allowed. The later `capsule.rs::validate_reply_options` applies the existing LB/PB v4
limit of 4096 UTF-8 bytes to the actual serialized ReplySpec and the raw options, and is shared by Logical/Physical validation and
PREFILL. reply must be nonempty; options may be empty. Exactly 4096 is valid, and the check is not replaced by the field length before JSON escaping,
the character count or the trimmed options length. PREFILL runs this check before session key recording, Tokenize and slot/
incarnation acceptance. The wire version/limits and native parser semantics were not changed. canonical
session/request/key keep the existing command/owner checks. This format check alone does not mean that the atomicity of other admission/resource rejections
is implemented.

##### OUTPUT v5 / BATCH_OBSERVATION v4 / STAGE_SPAN v4 migration contract

This section is the specification that the producer/consumer migration in this working tree must follow. Passing some sub-tests is not to be read as
the full observation gate or final performance promotion. The currently passing scope and open items are owned by the roadmap and the dated evidence.

- `ApprovedOutputPayload.issued_work` is required only on a normal sampled terminal. revision=1,
  issue_count/last_ordinal are u64, and authority_digest/digest are each **an array of exactly 32 u8 values**.
  Unknown fields/revisions, count=0, last_ordinal<count and a proof on a nonterminal are rejected.
  The explicit decode DTO for flat OUTPUT rejects unknown fields and does not rely on the loose deserialization of serde flatten.
- The head copies the witness from the original RequestState on the terminal settlement candidate after checking it against the original submission authority.
  Checks that can fail are completed before request removal, flight settlement and output effect commit. A new
  witness is not created on return/telemetry, and a fake witness is not attached to low-level flight registration. Existing v3/v4 captures are preserved unchanged.
- BatchObservation keeps logical_ordinal and the physical total statistics. The `owned_requests` of each physical execution
  carries request/submission_event_id/sequence_id/incarnation/request_issue_index and
  the exact `rows:[{phase,position}]`. The phase wire allows only prefill/decode/verify/replay.
  The per-request index must equal the count the head actually approved. The index in an observation is not the expected terminal total.
- There is one owner projection per parsed full OuterEndpoint. Different correlations of the same OUTER do not create duplicate full
  observations. The carrier is the first of that route's original ReplySpecs met in actual row order,
  and the route/provenance of every member is validated first. Foreign request IDs are not included, and physical total counts that include
  foreign rows are not reduced to that OUTER's row count.
- The execution_ids of a StageSpan and the key set of executions must be equal. Per-execution owned_requests
  send only request/sequence_id/incarnation. The downstream does not fabricate a submission event ID it does not know.
  Only Fresh is reported as a new span, and a cached-only replay does not emit a new compute span.
- A prepared observation is kept together with the ForwardObserved effect. **Right after the local completion mailbox accepts
  the Forward**, the time is fixed once and the observation becomes a Telemetry intent. This is not a network send or remote receive time.
  While Full, the same event waits; on Closed/ID exhaustion, the unpublished intent and the fixed time are kept and fenced.
  Automatic reconnect, crash recovery, durable delivery or bounded total RSS are not claimed on the basis of this effect queue alone.
- OUTER binds observations to the submission authority it actually sent. It holds per-request issue_index values that arrive out of order and
  hashes each issue once when they form a contiguous prefix. Completion requires the OUTPUT approval owner and the terminal chain to match.
  Per-execution owned membership and the declared stage coverage are checked separately as well. A span that arrives before its observation is
  provisional evidence, not authority to create the head's expected set by itself.
- The existing `elapsed_ms` up to termination/release is latched at that boundary. The time at which the extra observation wait ends is
  recorded separately as `telemetry_complete_elapsed_ms`. The normal response token count and the existing TPS denominator are not silently changed.
  Missing waits until the original overall deadline, Invalid fails immediately, and only successful complete candidates contribute to the row sums.

The DTOs and the canonical validation API are owned by the concrete llama adapter. No reporting policy is put into the shared P4 protocol/core, the native RowOwner,
or llama.cpp/ggml/CUDA types. Peer authentication, restart freshness, KV stop points, actual GPU
time and multi-computer results are outside the guarantees of this wire.

### L2 Acceptance and occupancy (admission)

Runs on request arrival, completion, cancellation and memory changes. Occupancy may be held for a long time, but
this is not a contract that delays the acceptance response by minutes.

- Acceptance: checks not only slots but also the per-model worst-case cell/auxiliary-state budget on every stage and the queue/byte/token limits.
  prepared reservations also count as usage. Unmeasured/unaudited per-cell costs and overcommit ratios are not used as facts.
- Eviction/checkpoint: L2 **has no policy.** When, which session, and under which
  key to persist, checkpoint or discard is entirely OUTER's command (the snapshot command
  model is owned by [kv-state-store-convention.md](kv-state-store-convention.md)),
  and L2 is responsible only for executing the command and its cell accounting. TTL is just one of the policies OUTER
  expresses in this vocabulary.
- Restore: follows the OUTER command and the restore decision ladder, cell reservation and stop-point contract of the [store convention](kv-state-store-convention.md).
- Fairness: admission waiting and runnable waiting are separated. Per-request age/deadline/maximum unselected interval are
  verified. ITL/TTFT are not assumed to be automatically fair just because requests share the same UBATCH or cells have headroom.
  On shortage, queue/reject or an approved OUTER snapshot policy is executed; automatic eviction is disabled until the failure gate.

#### PREFILL prepare and confirm — limited implementation of atomic acceptance rejection

Unlike the full resource contract of L2 above, the current change to `worker.rs::Worker::prefill` covers only **the Result
rejection transition of request state**. Whether it has been run and verified follows the roadmap/evidence record.

- After command/owner/string/existing identity validation, the incarnation and the pending FIFO prefix to be added are checked first.
  `release.rs::Worker::prepare_prefill_admission` virtually appends the new candidate after the existing pending entries and
  validates the whole set of slots/requests to be actually assigned this time. The ACK's existing prefix check uses the same validator and
  does not newly reject other sections of the whole queue. A new candidate already in pending is rejected.
- Only after Tokenize and the prompt+max_tokens validation succeed are the session key recorded, the request inserted, the incarnation
  incremented, the pending entry added and the validated FIFO assignment committed. On the sole worker there is no other handler/yield/
  publication in between. The `P4_SESSION_KEY_ADMITTED` record is emitted only after the commit.
- The error priority changes. After identity validation, the former Tokenize→context→incarnation→admission order
  becomes incarnation→admission→Tokenize→context. This is fail-fast, to avoid unnecessary native lookups for an acceptance state
  that is already invalid; it does not claim that the messages for inputs with several overlapping errors stay the same.
- Tokenize is a synchronous native lookup, not a KV issue. Preserving the request acceptance state on a lookup failure is not the same as
  every internal native/lifecycle state being unchanged. Publishing a generic ERROR exactly 1 time, and its ID
  consumption, are checked separately as rejection diagnostic effects. The contract does not extend to rolling back allocator panics or record channel failures.
- This prepare result is not a durable ticket or space claim usable across an asynchronous hold.
  The count/byte reservation for the actual request, output and future returns must still be attached **before** the first write.
  This fix alone does not declare bounded admission, blocked-input consumption or resolution of actor deadlock.

### L3 Composition (strategy)

Runs at every issue opportunity. It stays a pure function — all inputs are data, and the output is
an allocation. That is what allows replay and verification from recorded traces.

```
plan(demands,            // only those the ledger passed on residency preconditions
     row_budget,         // keeps logical n_batch and physical n_ubatch separately
     cell_budget,        // remaining cells of the tightest node
     pending_cache_ops,  // target-sequence fence + actual context exclusivity requirement
     shape_rules,        // HELLO negotiated values
     cost_model)         // per-model fixed cost and per-row cost
  → allocations + proposed fairness delta
```

A strategy is a per-model-family module behind a trait. The selection keys are the HELLO capability and GGUF
metadata (memory family, equal_sequence_ubatch, swa, shared_kv, nextn …);
hardcoding model names is forbidden.

- `waterfill` (default attention+unified): distributes and rotates eligible decode and waiting prefill within a limited row budget and
  per-request service bounds. The current `plan_ordinary` is a starting point, not a completed fairness proof.
- `equal_width` (recurrent/hybrid): enforces equal width. Decode collapses the width to 1,
  so it is better to separate prefill-only steps from decode-only steps.
- `atomic` (MTP/speculative): atomic window + fence. Composes with other strategies.
- Future: index-aware (DSA/MSA/DSV4), paged/radix families. vLLM PagedAttention and
  SGLang RadixAttention are references, but the condition that per-cell cost differs per node is
  an additional constraint specific to us.

Filling must first satisfy shape, dependency, credit, KV and per-request latency bounds.
Within those, width and issue frequency are compared by actual cost. Do not assume that fixed cost currently dominates on every model or
that a wider batch is always best. Exploration and promotion are judged by the fixed-topology A/B of the verification convention.

Generating plan candidates does not change policy state. A candidate's rotation/cohort
delta is committed only when L4/the issue ledger approves it. A stale candidate whose revision changed because a newer plan was already committed is not reused.
Not consuming fairness turns on rejected candidates and the real receive loop giving a finite number of issue opportunities are
separate conditions. Selector tests alone cannot rule out starvation from an unbounded input drain.

### L4 Proof (proof)

Right before submission, prove that the logical batch splits into **exactly the predicted
physical UBATCHes** under llama.cpp's split rules. The existing checks ((seq,pos) one-to-one, equal width,
verify fence) are extended with the cell budget and a check of the expected split count. On success, submit and
capture the ubatch callback membership; the downstream replays it without re-deriving it.

### L5 Transport (existing, efficiency target)

A cost term separate from strategy. cut-set packing and removal of unnecessary retransmission are candidates, and
priorities are set by separating serialize/copy/network/queue time on real multi-machine setups.
The equivalence gate must pass with no loss of tensor values, alias/view or membership.

## Execution ownership and native control binding — 2026-09-06 working tree contract

This section is the sole definition of the execution wire and in-memory ownership. It does not replace the identity/durable epoch of the stored cache.
The implementation basis is the adapter's `v2/node/ownership.rs::StageOwners` and `v2/control_identity.rs`, and
native's `runtime/physical_authority.hpp::PhysicalAuthority`. The current verification scope follows the latest record of the
[settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

- The execution owner is `(load_generation, session_id, sequence_key, sequence_id, incarnation)`.
  The incarnation is a nonzero monotonic ID issued by the head, and a new value is required whenever the same key/slot is reused.
  A native slot has only one active owner. New ownership starts only at Prefill position 0 of an empty/released slot.
- The LB/PB codec is revision **4** and carries an LE u64 incarnation after the owner's `load_generation`.
  The physical/tail/release/released/settle/settled content-types are v4 as well. Old codecs are not converted automatically.
  The incarnation and operation_id of `ReleaseSequence` and `SettlementSequence` are required; 0 and missing values are rejected.
- identity support is negotiated separately through `physical_identity_revision=1` in HELLO. It is not inferred from
  physical batch support alone. After checking the capability, the event product LOAD sends an LE u64 generation in **BindLoad (opcode 23)**,
  and exposes capacity/slots only after receiving the exact echo. A bind failure, a different echo or a lost response closes native.
  On the same worker, an attempted generation is also consumed, so reuse or decrease is rejected.
- native's BindLoad explicitly binds the load identity to the current Session. It does not bind implicitly
  by looking at the first PHYSICAL owner. In bound mode, bare slot Cancel, legacy Hop and existing KV mutation bypasses
  do not gain execution authority. Re-enabling those state features requires a separate integration that follows the same ownership.
- The native control prefix is `P4ID | revision:u16=1 | reserved:u16=0 | load:u64 | incarnation:u64 |
  operation:u64 | slot:u32 | session:(len:u32,UTF8) | key:(len:u32,UTF8)`, with LE integers.
  key is exactly `session + NUL + request`. NUL inside session/request is not allowed.
  The RELEASE body/response is exactly the prefix. SETTLE is the prefix followed by retain/replay_position/count(u32) and
  a replay token (i32) array; its response is the same prefix followed by a proposal count/token array. Short input, trailing bytes and a different echo are rejected.
- The head assigns operation IDs to approved settlement candidates. Only each slot's latest `(operation, request body, result body)` can
  be replayed. The same ID/body returns the same result without extra native execution; the same ID with a different body or kind is a conflict;
  an older operation or a different incarnation is invalid. Once a new owner comes in after release, old control receipts cannot touch the new KV.
- The released incarnation high-water mark is not forgotten even if another session uses the slot temporarily. Because of the limit,
  the watermark is never dropped; new acceptance is rejected instead. This policy is a bounded-memory contract for safety and
  does not mean unlimited new sessions are supported over long runs.
- 1MiB each for the request and response of each slot's latest control receipt, 64MiB in total and 65,536 watermarks are the current
  safety limits. A control carrying several sequences checks the total worst-case response space before the first native call.
  An exact Replay needs no extra reservation, and for New only the positive increase over the existing receipt is summed. A shrink scheduled by a later
  operation is not lent in advance to an earlier operation. This check is valid only within the serial commands of one worker.

### PHYSICAL receive receipt — 2026-09-07 follow-up working tree contract

`v2/node/physical_receive.rs::PhysicalReceiveLedger` is used in `Worker::physical` on middle/tail nodes **before** the native call.
It is a different ledger from head terminal settlement or the SETTLE/RELEASE receipts.

- The issuer of execution numbers is the head's native Session. The receive key, within the receiving load, is
  `(full Endpoint(agent,node,generation) of SESSION.first, execution_id)`. Issuing authority is taken from the already configured session path,
  and neither the incoming event ID nor a self-declared issuer in the payload is used.
  Reusing an ID with a different session/body of the same head is a conflict, but the same number from a different head is a separate, valid execution.
- The whole event input is validated first. The canonical input bytes bind the invocation, owned rows, incarnation and tensors
  together. Only Fresh entries go to native; an exact Replay that is preserved returns the existing response. The output of a mixed event is
  assembled in the original member order. It is a failure if a later conflict first consumes the owner/ID/native effect of an earlier Fresh.
- Fresh entries are registered as Running right before entering native. If the result membership/role differs, or on a lost response/execution error,
  all Fresh entries of that attempt become Uncertain and the worker is fenced. The KV that has already changed is not reported as rolled back.
- 64MiB/4096 entries for the total canonical input+result bytes of completion receipts, 65,536 Seen IDs in total and
  1024 issuing authorities are the current safety limits. A large normal response can be delivered once, but if it cannot be kept it is Expired.
  Active input, encoding scratch memory, publisher intents and total RSS are not claimed to be included in this cache budget.
- The numeric receive window is 65,536 per head, and IDs pushed out of the window by the highest ID are rejected, including not-yet-arrived gaps.
  The ID progress of another head does not move that window. Past IDs are not revived as new work by recomputing after cache eviction or by
  automatically discarding the issuer memory. Exceeding the new-identity limit is rejected up front.
- The guarantee is **0 extra native executions for an exact redelivery that is still preserved within the same Worker/load lifetime**, and fail-closed after expiry.
  The cache/window/issuer limits have not yet been negotiated with the retry period, edge credit or reconnect contract. This is not approved as lossless redelivery or
  exactly-once after failure. B3 must bind the valid retry period to retention/backpressure/explicit expiry.
- An event that contains only Replay does not newly acquire stage ownership and does not add a compute span. Re-answering an old result
  after release/slot reuse is distinguished from recomputing old KV.

**What this receipt alone does not guarantee:** the row order of new PHYSICAL IDs is handled by the separate frontier below.
Credit, multi-native atomic execution of all events, and restoration of durable receipts/outbox after a crash are separate. Even if the control preflight succeeded, if a later native call
fails uncertainly, the already executed KV is not claimed to be rolled back; the worker is fenced. There is also no load/run epoch authority yet that spans
restarts of the worker/native process. Until fresh fleet identity and blocking of reconnects/previous generations are
implemented, this in-memory guard is not promoted to restart exactly-once.

This change is **a deliberate version change of adapter-owned protocol semantics**. No request concept was put into the shared P4 envelope or CUDA/ggml,
and these are not fields dragged in by a llama.cpp API change. Conversely, this alone does not
complete the B5 gate for stage ABI/state ABI/actual placement/common type isolation.

### stage KV frontier — position/phase contract separate from the receive ID

`v2/node/frontier.rs::StageFrontiers` is a pure adapter ledger. It has no llama/ggml/backend types and
makes no native calls. It checks together the load/session/key/incarnation of the same slot, the next KV input position,
the generated token count, budget and options/reply, and pending Verify and permitted Replay.
It is consumed by the actual issue in `Worker::drive_one_batch`, by downstream `Worker::physical`, and by SETTLE/RELEASE.
This does not mean that native C++'s `PhysicalAuthority` itself performs this position check.

| Current state / input | Approval condition and next state |
| --- | --- |
| New owner / Prefill | From position 0, contiguous input. Generated count 0 before the last output marker. Output only once, on the request's last prompt row |
| Ongoing Prefill / Prefill | Only the immediately next position. No regression to Prefill after the final marker has been processed |
| Ready / Decode·Verify | Exact next position, generated count and budget. Decode is 1 row; Verify is a new speculative round that is not split |
| Full acceptance after Verify compute | Without a separate SETTLE, the next contiguous append confirms full acceptance on head/middle. The tail confirms with the actual outcome |
| Partial Verify acceptance / SETTLE | Only valid boundaries inside the in-progress Verify range. On the tail, must match the preceding rollback outcome exactly. No arbitrary trim on the strength of a new operation ID alone |
| checkpoint SETTLE / Replay | The actual restore end is `replay_position`. `retain_from` is the end to be refilled going forward. Replay the same round, exact token array and range only once |
| Termination / RELEASE | Delete the slot frontier, bound to the already validated control identity and receipt. Authority to accept a new incarnation stays with the StageOwners watermark |

Replay's wire `output=false` does not forbid internal logits computation, nor does it mean there is no actual generation result.
The unconfirmed result of a checkpoint Verify is not output; the result that the permitted Replay recomputes and confirms is output once.
The logits request mask of the native batch and the logical/capsule output mask have separate meanings. Even when logits the engine needs are
requested, the logical mask of the original owner/capsule is not changed. This translation is the native adapter's responsibility, and
no llama_batch fields or backend types are put into the pure ledger or batching policy. Actual logits/sampler/checkpoint
correctness is verified by native model tests, while the adapter's generated count, position and control settlement are verified by actual worker tests.

On the tail, the next issue must match the **full proposal token array** returned by the previous sampler and the Decode/Verify distinction.
head/middle do not see the tail proposal directly, so they do not claim the same token proof.
The proposal/replay length of a continuation must fit not only within the request's remaining token budget but also within the loaded atomic physical
capacity. This is checked before approving the tail PHYSICAL and the native SETTLE response, and head return
approval checks it independently as well. The width is not truncated in a way that changes the correct answer, and a late head rejection does not stand in for stage approval.
A violation after a native effect is fenced without a success receipt, frontier or forward. If the Fresh result bundle of a PHYSICAL has
a valid leading part and invalid trailing results, the whole Fresh set is left Uncertain. This does not mean the already executed native
KV/sampler was rolled back. Exact global deployment width negotiation and placement are a separate load contract.
An exact redelivery of an already completed ID does not advance the frontier again. A late gap is rejected up front, and
that rejection does not implement automatic reordering, retry or lossless delivery.

Whole-event Fresh/control checks finish before the first native effect. A pre-rejection preserves the owner, frontier, receipt,
output intent and native KV. A delta carries only the touched slots and rejects stale/ABA via the slot revision.
The commit happens after the row membership and outcome/proposal of the native response are confirmed; a bad
response or an unknown result after a success opcode is fenced. Errors after that point are not reported with the same rollback guarantee as a pre-rejection.
The current worker is a serial consumer in which no load or other state change can interleave during a native call. If that premise changes,
reservation/commit atomicity must be re-verified. This ledger is not a restart-durability, edge-credit or total-RSS bound either.

### Explicit UNLOAD versus failure cleanup

UNLOAD on a healthy worker releases native ownership **only at a local stop point** of the current load.
If any of the following remain, it is rejected as busy before the native call: waiting requests/acceptances, issue preparation/flight, pending SETTLE/RELEASE, a Verify fence, unpublished effects,
active KV in the stage owner/frontier, or receive Running/Uncertain entries.
A middle node is not considered stopped just because its request count is 0. Completion receipts and Released tombstones are
not in-progress KV, and an idle UNLOAD where only those remain is not blocked forever.

busy sends the caller an explicit error with a matching correlation ID and preserves the ledger, KV and issue effects.
Return, settlement and release of existing work must still be processable. Success is reported as UNLOADED only after local quiescence + successful native
cleanup. It does not guarantee that already published output reached OUTER or that the peer stages have
stopped, and it cannot be used as evidence that a cluster-wide drain completed.

A worker already fenced because of unknown effects/native results is different from the normal busy path. Failure cleanup of the current run
can preserve the remaining work and the original error and terminate as a failure, and it does not emit UNLOADED success.
Even if the native cleanup of a normal UNLOAD itself fails, it is not converted into a recoverable busy. A partially terminated
engine must not keep accepting follow-up requests/SESSION/reload; the failure boundary must be kept.
Forced termination, request Cancel and distributed Drain are separate command/state contracts and are not mixed into UNLOAD's success vocabulary.

### Control progress under output saturation — target contract for a yielding effect pump

**What follows is a target contract that is not yet implemented.** The current `worker/emit.rs::Worker::wait_for_publication`
holds the worker thread on Full. Adding a capacity notification is separate from the worker processing other
input. Run results, phase status and application order are owned by the roadmap and the evidence record.

- **Fixed outgoing items**: output, observation, control, error and LOAD/SESSION/UNLOAD responses are handled under the same retention rule.
  An Event's ID/sequence, body, recipient and correlation information are assigned only once. The Event returned by Full is retried
  unchanged, never replaced by re-serialization, ID reissue or native re-execution. The observation time fixed after Forward approval
  is not rewritten because of a later recipient wait either.
- **Response representability and state approval**: before installing the session path, confirm that the exact response can be serialized, given an ID
  and represented by the receiving codec. Per-field length checks alone do not replace the combined envelope check.
  A prepare failure consumes no authority/ID, and is distinguished from a handle rejection that consumes a separate diagnostic ID.
  SESSION validates the body and an envelope with the maximum issuable ID width before approving authority, but the actual number is
  assigned only at the head of the FIFO, after the earlier delayed observations have passed. Prepare success does not mean queue space was secured, or
  distributed rollback after Closed. If even a generic error's envelope is unrepresentable, no malformed wire is sent;
  the diagnostic intent and the failure cause are kept. The temporary copy cost of encode/decode is not the whole retained-byte budget.
  The maximum-ID-width check conservatively narrows the acceptance region. A boundary input that fits with today's short numbers but would exceed the
  codec limit with a future 20-digit number is not approved. This difference is not a mere move of the call site; for the same input,
  both codec success with the current number and rejection with the maximum number are checked. It is not bypassed by consuming a real number first.
- **Reservation and yielding**: both the effect count and the retained-byte total budget are checked. After validating and reserving all effects
  that one TAIL/ACK produces, the existing whole-event atomic commit is kept. Before the native call, space to keep its result and
  failure is also secured. Counting only the serialized wire length while excluding the cost of retained base/payload copies is
  not a bounded-RSS proof. Existing large wire limits are not arbitrarily reduced just to pass.
- **Resource declarations and kinds of limit**: the adapter-wide byte budget is not derived from `n_batch`/`n_seq_max`/mailbox counts.
  The host resource policy decided by OUTER is passed from the composition root as adapter-local settings.
  The count/byte regions for retained effects, held input, native scratch memory, future returns/notifications and failure diagnostics are
  declared numerically and summed. This is neither a field that puts llama knowledge into the P4 core nor a reduction of native wire format limits.
  Resource policy defaults must be explicit and validated, and if even the maximum obligation of one normal issue cannot be reserved,
  it is explicitly held/rejected before the native call. `n_batch` is not silently lowered, and results are not discarded after they are received.
- **ID headroom for future notifications**: before publishing RELEASE, the worst-case count/bytes of the per-original-submission receipts and the
  **number** of future Event issuances are reserved in the pending authority. Actual sequence numbers are not assigned in advance.
  Otherwise a late receipt would have a smaller sequence than output already published. Ordinary new sends
  use only the share of the remaining ID space that excludes the promised issuance count, and an ACK converts the validated whole-group reservation into actual
  monotonic IDs and fixed outgoing items. An exceeded total, overflow or bad ACK consumes no reservation, slot or ledger.
  Reservations are not returned on uncertain native results, and ordinary capacity is not demanded for the first time when the ACK arrives.
  The current `worker/obligations.rs::Worker::ensure_event_id_obligations` checks the **ID issuance counts** of queued effects, active follow-up observations,
  future release receipts and held diagnostics. It is not a count/byte space reservation.
  Ordinary issues are checked before prepare_issue and the native call, and an unknown result after a native attempt is not
  turned back into a pre-rejection; it stays Uncertain.
- **Retention lifetime**: the budget ledger, the fixed outbox and ID headroom belong to the Worker lifetime, not to the load.
  It is a failure if LOAD/UNLOAD wipes unsent old-generation responses with `clear`. UNLOAD's success response is prepared and reserved before native cleanup,
  and after success it remains an immutable outgoing item of its original cause. LOADED, which can only be built after reading the dynamic HELLO,
  first secures bootstrap/native scratch space and the limits of the success and failure responses. The temporary copies of actual frame receipt and
  parsing are also subject to reservation, and when an issue is Uncertain the obligation is not accounted as gone.
  The cost also includes effects taken off the queue and in execution. That memory is not returned just because the VecDeque length shrank.
- **Where pressure goes**: while unsent effects remain, new native issues do not keep piling up. Input holding itself is also counted in a finite
  count/byte budget. Repeated SESSION, errors and PREFILL are not moved into a separate unbounded queue. Capacity for returns/ACKs of already
  approved work is distinguished from capacity for ordinary new input, and L1/L2 own that reservation authority.
  Progress in a cyclic network where every queue is full needs not this local pump but an end-to-end credit/control capacity contract.
  A new request at the front of the single input FIFO being held because of Full is different from an ACK already accepted by the adapter.
  A finite parked queue alone does not prove progress of an ACK behind unbounded new input. Only when the reserved
  acceptance path for returning input and broker/edge credit are verified together can it be called end-to-end saturation relief.
  Reservations are based on actual space on the receiving side, and the adapter binds the originating work and mandatory return obligations. The neutral transport layer
  only honours the target, generation, ownership and count/bytes of an opaque grant and does not interpret llama content-types.
  The current broker ordering by `(source, correlation)` is kept. A later ACK is not allowed to overtake outgoing items of an earlier region with the same ordering,
  and EventClass::Control written by the sender is not accepted as reservation authority.
  Return credit is a transfer of responsibility for acceptance space, not KV settlement evidence. This is a constraint of the target contract and
  does not mean a new grant API/wire has been implemented.
- **Control execution authority**: the head's pending Release/Settlement does not become completion authority just by being registered.
  It has a monotonic state corresponding to `Queued → LocalApplied → ForwardAccepted`, and a valid ACK is applied only if it satisfies the exact
  load/session/key/slot/incarnation/operation and the last state. It becomes LocalApplied only after local native
  response validation and the owner/frontier commit, and ForwardAccepted only after the mailbox has accepted the exact Event to be sent to the next stage.
  All members of one command are checked first and then updated together.
  This mailbox approval is transport-stage evidence, not remote KV completion. A slot is returned only after the tail's single ACK, which binds
  application on all stages as a chain, has been established separately. It is a failure if a stale effect callback promotes a new incarnation or
  a different operation.
- **Replay and roles**: a native control receipt only provides the basis for LocalApplied and does not substitute for ForwardAccepted.
  An exact native replay keeps the state with 0 native calls and does not lower it to an earlier stage.
  The head's pending state is not forcibly created on middle/tail nodes. Middle/tail nodes keep their own native receipts and
  unsent Forwards. The current rejection of ACKs after retirement is not slipped into this change and turned into idempotent success.
  SETTLED does not require byte identity with the sent SETTLE body, because the tail's valid proposal may be appended.
  The transport stage is added on top of the existing identity/retain/replay checks and Proposal/Replay semantic validation.
- **Native atomic section**: the current `StageOwners::validate_control_batch` is a read check, not a reservation.
  No other command is interleaved between that check and the local native loop of the same command. The first pump
  yields only while waiting on external sends. To also split the native loop into turn quanta, receipt budget reservation and the
  candidate revision contract must be implemented and verified first. Transport-stage fields alone do not create that reservation.
- **Lifetime of validation tickets**: the head's current local/forward tickets are different private types and
  are valid only within the synchronous prepare→effect→commit section of the same worker. A ticket is not used as a reservation
  held across Full. A resend after yielding must re-validate the current load/session, the original control members and the route,
  and a past ticket is not applied to a retired key or a new incarnation.
- **Waiting and failure**: input arrival, output space and shutdown are observed together, preventing lost wakes between registration and re-check.
  A space notification is neither a reservation nor a send success, so the actual offer result is checked again. Full is Pending;
  Closed, ID exhaustion and unknown native results are each explicit failures. The ban on native execution is separated from the remaining lifetime for sending diagnostics,
  forbidding the regression in which an ERROR is dropped by worker termination right after being queued. Unsent items after the termination deadline are
  recorded as explicit abandonment, not completion. This does not claim that durable reconnect or distributed Drain is implemented.

The neutral mailbox provides only ownership of opaque Events and space/termination notifications. The effect budget, control execution stages and KV
settlement stay inside the adapter. Tests follow verification convention T22–T26, and an implementation must also fail if it fixes only normal ACK progress while
allowing early ACKs, or passes only the memory limit by blocking all input.

#### Fixed outgoing items and unassigned ID obligations — limited implementation contract

The committed FIFO in `worker/effects.rs::Worker::flush_effects` currently serializes only the head entry, issues a checked ID
and turns it into `Publication { event, after }`. Whether the actual implementation has been run is owned by the roadmap/evidence.
This distinction is an internal representation contract and does not change the wire version or ordering regions.

| Failure/transition point | What to preserve | Unassigned ID obligation |
| --- | --- | --- |
| Serialization, ID or head pre-authority check failure | Original DTO/body, existing FIFO, native state | Original count kept, no ID consumed |
| Event creation success | Exact envelope, ID, sequence, payload and after-action | Only its own ID consumed, follow-up share kept |
| Full | Retry with the same Event, re-validate current head authority | No extra ID consumed |
| Closed, permanent overflow, Full during termination | Return the same Event to the head of the FIFO and fence | IDs already issued are not rolled back |
| ForwardObserved accepted | forward removed; observations with the once-fixed time placed in original order | N observations not yet created |

`next_event + unassigned obligation` therefore does not change on Event creation. The unassigned share of an ordinary Publication is
0, and that of an Observed Publication is N. `active_effect_ids` includes all N after materialization so that
ACKs/diagnostics during Full cannot use that share. This is **not a space reservation count or an Event retention count.**
The full recipient/provenance/payload of the observations left in the original intent and the queued suffix are preserved.
Automatic fence release, native re-execution and reconnect replay after a failure are not permitted. The manual reconnect in the tests only
checks that the same Event is preserved; it is not an operational recovery API.

LOAD, SESSION, UNLOAD and generic errors also put unnumbered direct response intents into the same FIFO and turn them into fixed
Publications at the head. Follow-up observations of an earlier ForwardObserved get numbers before responses that came in later.
A batch error must not lose the diagnostic intent of the remaining participating requests because of the first delivery failure.
A diagnostic intent that cannot be represented is not treated as a publishable Event; it is kept as a non-published failure.

There are two cases for diagnostics after a native fence. Sending only a single termination diagnostic produced in a state with **no existing effect prefix**
is not a new native execution and does not release the existing fence. If there are existing undelivered/uncertain effects,
the diagnostic is kept after them. Those effects are not re-executed, and outgoing items with fixed sequences are not overtaken.
The existing synchronous Full wait and raw EventNode consumption remain, so this retention representation is not to be read as an async pump or end-to-end
reservation. Securing result space before LOAD/UNLOAD native is not completed by this representation change alone either.
The broker's actual destination `try_reserve` is one slot for an immediate dispatch, not a future/native/remote
grant. Returning the original on a raw failure is owned by the
[neutral event contract](event-protocol-v2.md#local-refusal-ownership--limited-implementation-boundary).
Queue/duplicate-ledger copies and owned-claim transfer on the success path are separately incomplete.

#### Local completion store reservation — implementation contract with limited scope

This section owns only the local store API of `node_adapter/mailbox.rs`. The current implementation/run status and the next steps
follow the roadmap and the evidence record. Do not stretch it to mean distributed grants, a native result budget or a B3 completion contract.

- For cost, `retained_event_bytes` sums the Event's inline size and the **capacity** of every independent String/Vec.
  It is not replaced by the wire length, len or deduplication of identical strings. Entry bookkeeping is added to the claim, and the actually pre-allocated
  queue backing is reported separately in the snapshot. Allocator/waker/native scratch memory and RSS are outside this figure.
- Reservation, ordinary publication, queue retention and owned dequeue use the count/byte ledger of the same actual store.
  `try_reserve` secures one Event, and `try_reserve_group` secures the whole footprint list of known mandatory results
  in one critical section. A failure on the last item, or contention/Closed before the final check, leaves no partial claim.
  A group hands over move-only items in input order, and an item taken out has a lifetime independent of group cancellation.
- `completion_mailbox_with_limits` **separates delivery queue slots from retained count/bytes**.
  Even when the retention space for multiple results is secured first, they can be delivered sequentially through a single queue slot. The existing constructor
  keeps the compatible contract in which queue and retained count are equal. This is separate from budget selection/wiring in the actual composition root.
- The backing bytes corresponding to the group's actual VecDeque capacity are also charged as a separate claim. They are not returned
  even after all items are taken out, until the array is destroyed. The group API is therefore rejected on a count-only
  store without an explicit byte limit. This is the upper bound of successfully retained space, not a bound on concurrent pre-commit arrays, the allocator or RSS.
- The existing count-only constructor declares no byte limit. The fact that the byte-budget constructor was used in some tests
  does not mean the byte limit is enabled in the composition root or the product Worker.

| Transition | Event owner | Store claim | Failure/cancel rule |
| --- | --- | --- | --- |
| reserve | may not be produced yet | the move-only reservation occupies real space | Full is a temporary shortage; exceeding the total limit or cost overflow is a permanent rejection; cancel returns the exact share |
| publish_reserved | moves into the queue | the queue takes over the original reservation | queue Full, wrong store, value larger than the reservation or Closed returns both the original Event and the reservation |
| take_owned | RetainedCompletion | stays occupied after leaving the queue | extracting a raw Event cannot erase the accounting responsibility |
| transfer | moves to a new actual store | old claim returned only after the new store accepts | on failure, the original Event, the old claim and the new reservation are all preserved |
| retire | Event discarded | returned after the Event is destroyed first | a transition distinct from transport acceptance/KV settlement/durable completion |

The existing raw `try_take/poll_take` does not consume or skip a reserved front. **The reserving producer, the owned
consumer and the responsibility transfer must be wired together**; turning on only the producer in operation first causes a stall. Existing raw consumption
returns only the queue claim when taking an item off the queue, so it is not an API that tracks the retention cost of the EventNode afterwards.
This authority belongs to the local actual store and does not substitute for the sender's EventClass, source authentication, peer generation or wire authority.

Returning a delivery slot is different from retiring a store claim. With separated limits, when an owned dequeue empties the queue, the queue
waiter is notified but the claim is kept. A queue Full on an ordinary publication does not trigger a self-wake by creating and cancelling
a temporary claim. A permanent byte overflow of a single Event is rejected before queue Full.
All normal capacity callbacks run outside the store/budget/registry locks. Group cancellation returns both unused claims and the actual
array accounting, then notifies once. The first caller panic is propagated; releasing a claim during unwind only returns
the accounting and skips further capacity callbacks. Delivery/progress after a panic, and the safety of arbitrary RawWaker
destructors, are not guaranteed. Callbacks must still honour the non-blocking, non-panicking contract.

`try_publish_deferred`, `publish_reserved_deferred` and `transfer_to_deferred` perform the same actual enqueue
and then return a move-only `DeferredCompletionNotification`. The existing immediate-notification APIs also consume this return value's `notify`
after the same enqueue. The caller consumes the return value once after releasing all locks;
for a transfer, the old source accounting is returned first, and then the reader/capacity callbacks are called. Drop silently returns only the source
accounting; it neither cancels the enqueue nor runs the notification in its place. Missing `notify` is not a memory
leak but a violation of the progress contract. The return value holds a weak registration slot reference, not the Waker itself.

**Notification delay is not a visibility barrier.** A reader that is already running can read the accepted Event even before notify.
A callback panic is not interpreted as an enqueue failure or permission to resend. The current raw broker does not yet
consume this deferred boundary, so this API change alone does not establish notification outside the broker ledger lock.
Migrating the product composition of the actual broker/owned receiver remains. The owned `RetainedEventBroker`
uses deferred enqueue and out-of-ledger notification through a separate explicit path. This is not counted as a behaviour change of the raw product path.

When a single native job produces several mandatory Events, merely requiring all of their slots in advance with completion capacity=1
does not allow normal progress. The retention space for follow-up effects must be distinguished from delivery queue slots, and wiring must extend to
the responsibility transfer on the actual receiving side. This limitation is not hidden by enlarging queues, forbidding SESSION or shrinking existing cap1 normal input.
Reissuing RELEASE's preview Event ID later in an ordinary Forward is also forbidden. An earlier effect
may consume an ID first, so the one-time issuance of a fixed Event is carried out at the outbox transition where the actual order is settled.

#### Broker responsibility transfer and exact duplicate retention — target contract to keep when wiring

The current `event_broker::EventBroker::dispatch` stores the Event both in the destination and in the duplicate ledger when it succeeds.
Moving the producer claim into the ledger is therefore not a cost separation. Producer space stays tied up until the exact duplicate ledger
is retired, creating a new cyclic wait. The following conditions are the **target contract for full wiring**.

- The actual storage claim of the destination and the independent cost of the exact Event duplicate copy are secured before the success commit.
  On every failure, the original Event/producer claim is returned and the ledger is left unchanged. The original's responsibility transfer completes only after both
  sides accept. If the receiver still retains the Event after dequeueing it, the destination claim is kept.
- The existing comparison is whole-Event equality, and the ordering region is `(source, correlation)`. It is not replaced by hash-only comparison or
  early eviction on byte shortage. If the final cost, reflecting normal count-window eviction, exceeds
  the limit, that is not a temporary Full waiting for a destination dequeue. It is classified as a permanent
  rejection by storage policy/explicit limit. No capacity waiter that can never be released is registered for it.
- The retirement/capacity callbacks of the source claim must run **outside** the broker ledger lock. Merely wrapping the
  `transfer_to` call inside the ledger lock can deadlock through re-entry from the claim Drop after success.
- Responsibility must carry through not only the ordinary `EventSender/Receiver` but also the connection writer, EventNode held items, adapter input and worker
  held items. A bridge that takes a raw Event out midway and immediately drops the claim proves only the queue limit.
  A successful remote write is not evidence of receiver acceptance; a separate grant/acceptance contract is required.

The required wiring scope of the actual owned migration is as follows. **This is a table for judging missing wiring, not a completion list.**
Switching only one row to opt-in while the next consumer keeps a raw copy long-term is not approved.

| Product boundary | What must be preserved within the same lifetime |
| --- | --- |
| NODE_LOAD in `event_runtime::{run,control}` → broker queue/ledger | Actual destination budget, the original and the independent duplicate copy, the cost of rejection and eviction |
| `EventNode` → `NodeAdapter` → `WorkerInput` | Owner of held values in both directions, offer rejection, completion dequeue, terminal return |
| worker → `RequestState`, `PendingRelease`, follow-up effects | Cost of long-lived raw text/parse results, sharing scope of candidate copies, source of release after terminal |
| `effects`/`emit`/`ack_service` → completion | Claims until fixed Events, follow-up observations and diagnostics are accepted by the actual store |
| control reply and `NodeOwner` lifetime | Broker rejection, completed task results, retention or explicit termination verdict for NODE_UNLOAD/Drop |
| transport receive → broker → connection writer | Frame/decode scratch space, Full, send failure and unknown results, responsibility before remote acceptance |

The legacy Frame runtime is not counted as the Event path of this table. The existence of an OUTER that reads the same Event wire
is not evidence of remote acceptance either. Migration status and execution order are owned solely by the roadmap.

**Owned local delivery boundary:** `RetainedEventBroker` is a type variant that shares registration, generations and the whole-Event duplicate ledger
with the raw broker. `RetainedEventNode` consumes the mandatory `RetainedNodeAdapter` contract and has no raw
fallback. On the new path, the source claim is returned only after the queue slot and retained count/bytes of the actual destination
have been secured together and the item enqueued. Held input/output and terminal returns keep their claims.
An independent front reads the envelope and the actual capacity cost, reserves at the destination immediately, then dequeues conditionally.
A destination Full leaves the front in place, and the same `(source, correlation)` is not overtaken.
A ticket is not kept across an await, a native call or a remote send. Route registration is re-checked up to the enqueue and
held under a read lock. Source retirement, reservation cancellation and receiver notification run outside the ledger/registration locks.
The whole duplicate copy and the count-window are kept, but **the byte limit for the duplicate copy is not wired yet**.
The consumption tests of this type boundary do not mean the runtime composition root or connection writer migration is complete.

**Owned consumption in llama.cpp:** `RetainedLlamaNodeAdapter` uses the same actual Worker loop.
The claim of `WorkerInput::Retained` is kept together with the raw text in the bounded std input queue, the current handle/native call, non-ACK holds during Full and
deferred ACK errors. The raw text is borrowed for processing, and there is no bridge that turns it into a raw Event clone.
`try_publish_owned` accepts the bytes/count of the current Event in the existing completion store and blocks raw dequeue.
It does not mean advance reservation for future native results or for effects not yet materialized.
On interruption, the causing input, held input, ACK raw text, unprocessed receivers and state/effects are preserved in the adapter owner.
An owner Drop is an explicit local discard, not a successful drain, replay permission or remote acceptance. Apart from the raw input text,
independent byte reservations for parsing, follow-up effects and duplicate copies, and the responsibility wiring of runtime control and the writer, remain.

#### Immutable accepted input and mutable progress candidates

**Accepted input storage budget (implemented 2026-09-11):** `node/request_budget.rs` limits, under one account, the input shared and retained by the head's pending/active requests and
their follow-up effects. The default limits are 4,096 inputs,
512MiB retained footprint, 16Mi total input tokens and 16Mi total `max_tokens`. This is separate from the slot count, and
an acceptance rejection must not change the reservations of request/session-key/incarnation/free-slot/existing requests.
The footprint includes the independent allocation capacity of the original Event and the normalized command, the reply, and conservative input/bookkeeping
costs. Tokenizer scratch memory, KV, issued rows/capsules, future output bytes and broker receipts are
outside this account. An output token reservation is not a reservation of future output **bytes**. This does not mean B2/B3 as a whole are complete.

The reservation is secured after all acceptance validation and read-only tokenization, and before the first admission write.
Input is shared as `Arc<RequestInput>`, and the reservation is charged once per actual input. Even if the entry is removed from the request map,
the reservation is not returned while the shared input is alive; it is returned after the data of the last input is destroyed.
`P4_STAGED_TRACE_REQUEST_STORAGE` records the input account values of that node/load at acceptance.
Account retirement on normal completion, full-width release and slot reuse is checked with actual Worker loop tests.

The command after acceptance normalization (including tokens/options), the original Event and the reply source are not
subject to change by later candidate transitions. A `RequestState` copy shares this input and copies only the progress
state, such as prompt/ready/outstanding, independently. The production path provides no mutable accessor or copy-on-write for the shared input.
If observation/error reporting during an issue must outlive the original request, a read-only shared owner is kept. Legitimacy
lies with the original request and is not switched to the source of the current PHYSICAL/TAIL/ACK.

A shared allocation is cloneable **data ownership**, not a linear transport claim or a space reservation.
A later owned migration must not hide both in the same Arc and skip the accounting. The separate cost incurred when the wire raw text and the parsed tokens
are first created, and the costs of ready/continuation/RowOwner and native results, also remain. The fact that sharing
reduced copies does not approve byte admission, an RSS bound or batch throughput.

#### Optional pipeline request grouping (2026-09-11)

`P4_STAGED_PIPELINE_BATCHING=1` uses generation-first assignment and independent prefill cohorts for ordinary attention.

This policy's `min_batch_rows` applies only when the actually prepared plan is decode-only. When the number of eligible decodes in the same SESSION
is smaller than `min(min_batch_rows, generation group width, actual row capacity)` and an existing flight exists, the first wait decision grants a 2ms monotonic-clock
deadline. Additional input does not extend this time. The Worker's `recv_timeout` makes it re-check the issue condition even without new input.
This is not a 2ms response guarantee that includes OS scheduling or native/control execution time.
A plan that includes prefill proceeds immediately within the existing row/member/quantum limits. The legacy and atomic/equal
paths are not affected by this change. An empty plan or no work to prepare clears the wait, and other blocking reasons such as a full flight or a fence
disarm the timer. Expiry grants no native/KV/flight/storage authority.
`SchedulingSnapshot.pipeline.decode_coalesce_max_ms` records the applied limit and may be absent from past observations.
The prefill participation limit is the ceiling of the sum of accepted unfinished prompts (ready/inflight/waiting) and requests waiting for their last prompt return,
divided by the existing window. Waiting for the last prompt return preserves only the initial group width and
does not supply new input. The actual participation width is at most the unfinished prompt population, and pending admission is excluded.
The decode participation limit is the ceiling of the total active generation population divided by `min(active generation population, flight window)`.
Independent groups are not merged on the basis of the momentary ready count, the number of empty flights or simultaneous returns. Rows are
reserved starting from the selected generation group, and the explicit `OrdinaryLimits.decode_members` and actual row capacity are respected. The full row budget of pure-prefill is kept, so
16 long inputs with window 8 and batch 512 issue 2 requests × 256 rows and can prepare the next batch with the remaining requests.
Node count or GPU count is not used in place of execution cost, and this arithmetic is not guaranteed to be the throughput optimum.

While actual decode is in progress in the same session, the prefill row budget of
`P4_STAGED_MIXED_PREFILL_ROWS` (experimental initial value 128) applies even if there is currently no eligible decode. When the last decode
finishes, it returns to the pure-prefill budget. The initial state that only waits for the last prefill return does not turn this limit on.
Native calls are non-preemptive, and this row budget is not a time/SLO guarantee.
The rotation of participating request selection and prepare/validate/commit fairness keep the existing scheduler authority.
The selection result is recorded in `SchedulingSnapshot.pipeline` as window/open/decoding_active/effective_limits.

It requires explicit `max_open_batches > 0`, `mixed_prefill_rows > 0` and fragment 1. An invalid combination is
returned as a request error before tokenize, admission and KV. It is not applied to Verify/Replay or equal-width recurrent paths.
Disabled by default. This request grouping by itself does not include window growth, multiple fragments, a time cost model, end-to-end B2/B3 or performance promotion.

`P4_STAGED_MIXED_BATCH_ROWS` is an optional row limit on the **generation+prefill total**, chosen by prior profiling.
While actual decode is active, it applies to both the logical and physical selection capacity. Selected generation rows are reserved first,
prefill is assigned to the remaining rows, and the existing prefill-only limit is also respected. It does not apply to the initial pure-prefill.
0 or invalid values are rejected before tokenize and reservation. Using it together with the experimental online time controller below is also rejected.
Observations gain `mixed_batch_rows` and `decode_groups`, which may be absent from past observations.
The profile's model/native/assignment/context/phase/width range and actual ITL verification are bound by separate real-hardware evidence.

#### Optional pipeline RPC service budget (2026-09-12)

Setting `P4_STAGED_PREFILL_SERVICE_MS` to a positive integer adds pipeline RPC completion time prediction and
prefill chunk reselection to the policy above. The same number does not mean the same latency target as in the earlier per-stage backlog policy.
Disabled by default; only ordinary attention, fragment 1, the pipeline policy, open 1–128 and stage 2–64 are allowed.
It is enabled on every agent participating in the same run. A stage sends the head the monotonic-clock time before and after the actual Frame call, plus the exact
load/session/execution list, per-phase row counts, request count and maximum input position.
The head checks the declared stage source and the accepted membership. Exact duplicates are not learned, and
conflicts for the same execution/stage are rejected before any state change. This information does not retire the flight/KV/edge.

decode-only also sends feedback and is included in the cost of open work. For the same stage/load/session/row count/request count and
base-2 bucket of the maximum position, the maximum of the latest 8 samples is used. An unmeasured width in the same bucket is predicted conservatively,
scaled up by the row/request growth ratio. When each of two widths has been observed 2 or more times, the prefill width is 2× or more, the cost has increased,
and the decode/request counts are equal, an affine growth cost that does not multiply the fixed cost repeatedly is used. Different stages and
context buckets are not merged. This does not mean the actual backend `n_kv`, mask or kernel cost is known, and it is not a hard limit.
It is bounded to history 128 and profile 256, and only completed history is replaced. UNLOAD clears this cost history.
Late cost feedback from an earlier load is ignored and neither reopens a completed load nor creates output authority.

When generation service is needed, the open batches and the candidate are projected onto every stage in actual issue order. Each stage's next
expected completion is `max(arrival from the previous stage, completion of the previous batch)+RPC cost`. The execution of a stage for which an actual completion sample has arrived, and of the stages before it,
is not charged again. This changes only the cost estimate and does not return settlement/output authority.
If the candidate's predicted last-stage completion exceeds the budget, or the cost is unknown, the prefill rows are halved to build a new plan.
The maximum number of search steps is the bit width of the row count, and prepared/rejected candidates do not change fairness, requests or flights.

Unknown cost is not 0. If there is an unfinished prefill or open work of unknown identity, only generation is replanned with `calibration_wait` or
`defer_prefill`. However, to avoid the initialization barrier in which a small shape can only be learned by waiting for a large prefill to return,
an unmeasured candidate may have at most 1 calibration probe of 1 row within the existing window.
If a 1-row prefill job is already open, or the open identity is unknown, no new cold probe is sent.
Once all earlier prefills are settled, at least 1 row is measured as `cold`/`progress_probe` to avoid permanent prefill starvation.
The original large quantum that was rejected is not re-allowed as is.
prefill 0 is not expressed as the 0 (unlimited) of `OrdinaryLimits`; the plan is built with prefill demand excluded.
Pure-prefill that does not yet need generation service keeps the existing full width.

In addition to the verdict, budget, existing stage backlog aggregates and known stage count, `SchedulingSnapshot.service_budget` records
`predicted_tail_rpc_us`, `examined_prefill_rows` and `selected_prefill_rows`.
When omitted on the existing wire they are None, and the default disabled output format is unchanged. An issue that fell back to decode-only also
keeps the original candidate's `defer_prefill` verdict. When decode was not ready and the issue itself was deferred, it is re-checked at the next actual
input/feedback/settlement. The total time and reasons of this wait are currently not aggregated separately in production observations.

This is a **soft budget for the whole pipeline RPC**. Executions not yet confirmed are conservatively charged the full expected cost,
and elapsed time after the actual start is not subtracted. It does not yet cover the client completion time including transport, dispatch and sampler return,
per-request deadline/time deficit/aging, or up-front reservation of all returns. It targets only ordinary attention, and
does not force the same physical shape on equal-width hybrids that cannot mix.
A small budget can also reduce GPU feed. In 6 MI250 comparison runs on c6bd6c597, merging all decode reduced independent
flights and generation performance, and a 250ms time budget reduced ITL during long prefill but worsened prompt completion and total throughput.
Do not read this description of current behaviour as a recommended policy. It stays disabled by default, and performance promotion is refused.
The next change contract is owned by the [roadmap](distributed-batching-roadmap.md#v11-plan), and the measurements/counterexamples by the
[6-arm verdict](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#generation-service-screen).

Tampering with immutable input in test fixtures is allowed only through an explicit test-only COW. The candidate's shared original/progress isolation and
the preservation of the original after rejection are checked together through allocation identity and value comparison. The input must stay valid while the last read owner
remains, and must be retired once the last owner is gone. Actual consumption and execution status are owned by the evidence.

## Measurement: batch width versus pipeline depth (2026-08-31, re-measured 2026-09-01)

Arrival-phase fragmentation and its correction were measured A/B on a 4-node harness. The results actually correct
this contract's filling rules.

**Observed defect**: a batch is formed only from sequences ready at that moment, so a group of requests that arrived
together is regrouped with the same members forever. In a continuous-arrival run (2 requests every 5 seconds, 24 parallel),
4,483 physical batches formed only **38 unique membership sets**, with a width of
2.84 rows. In a wave-arrival run (20/10/10), 3,000 batches round-robined over exactly
**3 sets** (20 rows 1000 times, 10 rows 2000 times). Groups born at different times
become ready while the other group is in flight, so they never meet.

**Correction**: a merge threshold (`P4_STAGED_MIN_BATCH_ROWS`) was added to the scheduler to defer planning until enough
rows have gathered. It worked as intended — for continuous arrival, width
2.84 → 10.56 rows, physical batches 4,483 → 1,205 (-73%), mixed batches 2 → 19.

**2026-08-31 observation (RTX 4080 + 3090, asymmetric placement): throughput was worse.**

| Scenario | Policy | rows/batch | ms/batch | Generation TPS | Semantic verdict |
| --- | --- | --- | --- | --- | --- |
| Continuous arrival | Default | 2.84 | 24.5 | **108.9** | 40/40 |
| Continuous arrival | Threshold 8 | 10.54 | 102.3 | 96.9 | 40/40 |
| Continuous arrival | Merge all | 10.56 | 103.2 | 96.2 | 40/40 |
| Wave | Default | 13.57 | 68.2 | **195.4** | 40/40 |
| Wave | Threshold 40 | 30.41 | 213.5 | 139.8 | 40/40 |

When rows grew 3.7×, step time grew 4.2× (continuous); when rows grew 2.2×, it grew 3.1× (wave).
**In this configuration, step cost was dominated by row count, not by a fixed cost.**

**2026-09-02 re-measurement (RTX 3090 x2, pin `0eadefebd`, same launcher, fence verified)**

The earlier measurements used a different launcher, pin and batching policy in each run, and the evidence could let stale records
through. This table is 4 runs on **one launcher and one pin**, each judged only from its own
records cut out with a fence.

| Scenario | Policy | rows/batch | ms/batch | Physical batches | Generation TPS | Mixed batches |
| --- | --- | --- | --- | --- | --- | --- |
| Continuous arrival | Default | 6.38 | 117.3 | 1,994 | 51.15 | 5 |
| Continuous arrival | Threshold 24 | 16.78 | 264.2 | 758 | **59.73** | 19 |
| Wave | Default | 18.41 | 164.3 | 2,212 | 109.92 | 0 |
| Wave | Threshold 40 | 34.19 | 304.3 | 1,191 | 110.27 | 2 |

For continuous arrival, rows grew 2.6× while the step grew 2.25× — **sub-linear**, throughput +17%.
For wave, rows grew 1.86× while the step grew 1.85× — **exactly linear**, throughput tied
(109.92 vs 110.27, a 0.3% difference).

**And the baseline itself is bimodal.** The wave baseline with the same pin and same policy comes out in some
runs as width 13.57, 3,000 batches, 93.7 tok/s, and in other runs as width 18.41, 2,212 batches,
109.9 tok/s. Whether the groups merge depends on timing, and **no policy can be characterized from a single
sample**. What this document earlier wrote, "threshold 40 recovered 94 to
111.8", compared 1 fragmented baseline run with 1 threshold run — the recovery is real, but its size depends on which mode the baseline falls into.

**Contract**: the sign of the threshold's effect is set by whether fragmentation is present, and fragmentation itself depends on timing.
It is therefore neither a standing policy nor a value to be set once by experiment — nodes must report their own
batch width via telemetry and react to it (P1a). The default is off,
because without fragmentation it only makes step latency 1.85× and gains nothing.

**Measurement limits (2026-09-02, corrected)**: this document earlier said "6 baseline samples on a clean commit".
**That was wrong.** Checking each run's evidence showed that `repo_commit` was `ed846d920` for all of them, but
only 50.40 was clean; the other five came from 4 different dirty diffs.
The same commit does not mean the same source — this was another sentence written without opening the artifacts.

Only the four runs from the same source state (dirty `f63266ff`) can be used for a paired comparison.

| Policy | Generation TPS | Source state |
| --- | --- | --- |
| Continuous arrival default | 50.45, 52.82 | `f63266ff` |
| Continuous arrival threshold 24 | 58.74, 58.93 | `f63266ff` |

In this pair, the threshold gives +14–17%. The remaining baseline samples (50.08 / 50.40 / 53.41 / 54.96) are
**from different source states**, so they cannot be pooled as one population, and they cannot be used for a before/after
refactoring comparison either.

**Non-degradation still cannot be judged.** There are two reasons — the run-to-run spread of the baseline is about 10%,
which covers any change a refactoring could make, and the source states of the samples to be compared were mixed. Judging it requires
**many more samples from the same source state**, or a metric with lower spread. Fragmentation
appears to cause the spread, so once nodes report their own batch width (P1a), comparisons conditioned on width
become possible.

**Status of figures without provenance**: the runs in the August 31 table and the two wave runs on the morning of September 1
(110.6, 110.1) were measured with stage binaries that left no provenance, and
those binaries no longer exist. They must be read not as a baseline but as **observations of an unidentifiable build**,
and this is why `patch_set` was put into HELLO and runs were made to check against the expected
pin.

**Past cost interpretation (not a current universal conclusion)**: this paragraph earlier said "reduce round-trip latency,
i.e. the per-hop cut-set transfer cost (D7/P6)". **It is not transfer.** Measuring per-stage
spans showed the node 2→3 hop at **2 ms** (p50) when the tail is empty and
**128 ms** when it is busy, and 63% of batches meet a busy tail. The 130 ms average was not transfer; it was heavily mixed with
waiting in front of the tail. This observation of a specific link/model does not support the conclusion that multi-machine transfer is always cheap.

**(Correction 2026-09-04) The next paragraph was wrong.** The correlation below was measured in runs where width was an *effect*
— the first node plans with what is ready, so the faster the run, the sooner the ready set
empties and the narrower the batch. When width was fixed by policy to make it a *cause*, the sign flipped:
in 8 alternating runs with the issue width limit (`P4_STAGED_MAX_ISSUE_ROWS`), **width +0.898, concurrent compute
−0.060**. At limit 12, it achieved 95.4% concurrent compute, the highest GPU utilization and 3,698 mixed batches,
while total throughput dropped from 544 → 198 rows/s.

In this experiment the strong candidate was **per-batch fixed cost**. The tail (22 layers) step time versus width is
**34.2 ms/batch + 1.051 ms/row**, giving 0.208 rows/ms at width 8 and 0.705 rows/ms at width 98, so
**wide batches are 3.4× more efficient per row**. The curve is still rising at width 98.
Busy stages were busy only because they kept paying that fixed cost.

Below is the record from before the correction, kept as a disproven paragraph.

~~The real lever is **the number of stages computing at the same time**. In 10 runs of the same scenario, the correlation with generation~~
~~TPS was **+0.923** for the ratio of 2 or more concurrently computing stages, +0.684 for tail utilization, +0.213 for batch count,~~
~~+0.121 for open depth, and **−0.211 for batch width**.~~

And what blocks that concurrency appears to be balance, not scheduling. Stage cost is
**a fixed 54.7 ms per batch + 3.04 ms per layer**, so the 22-layer tail takes 121 ms and a 4-layer stage
63 ms. The 5/4/4/22 cut is fixed because of KV sharing (layers 13–34), but placement is not —
the default placement puts the tail and node 2 on the same card, so the two that should overlap contend.

**What the step cost actually is (2026-09-04)**: instrumenting four sections inside the stage server showed that
parsing, owner matching and response encoding together take under 3 ms, and the cost is only `llama_decode` and
**sampling**. On the tail, sampling is larger — running 22 layers costs 0.11 ms per row,
while picking the token costs **0.29 ms per row**, so the sampler costs 2.7× the transformer.
This is because the vocabulary is 249,157 or more and the candidate array is built per row on a single thread.

**This is a past parallel sampler experiment, not evidence for operational promotion.** The synchronize/output reorder safety of the shared llama_context
is unverified, so the default stays serial.
In 8 alternating runs at the time (block order reversed),
generation TPS went 194.06±6.54 → **213.45±8.09 (+10.0%)**, and in width-matched comparisons sampling went
−28% to −49% (9–128-row range). **The distributions do not overlap** — the lowest parallel run, 204.7, is higher than the highest serial run,
202.3. All 8 runs passed 192/192. In this record, it is the first change that survived alternating verification.

`P4_STAGED_SAMPLE_THREADS=1` restores serial behaviour exactly, and that is the control group.

**Placement cannot be judged either.** Giving the tail a whole card (3+1) makes the tail step
12.7% faster and raises utilization by 10.3% (in all 4 pairs), but throughput is +2.7%, within the spread
— the best default run beats three of the four tail-alone runs. Looking only at the first two pairs gave +4.8%.

**And the session itself drifts.** Across today's 18 runs, the correlation of run order with tail utilization is
+0.696, and with batch width −0.700. The later a run, the busier the tail and the narrower the batch —
regardless of policy. A sequential sweep inherits this slope as is, so **it is not evidence.**
Every later policy comparison must use alternating runs.
**The issue policy remains undecidable.** When the tail-aware issue limit
(`P4_STAGED_MAX_OPEN_BATCHES`) was tested at 0/2/3/4/8, the sweep run in order
increased monotonically by +21%, but **it disappeared once the order was shuffled** — the control group itself spreads from 170.8 to 216.9,
a 27% gap. No issue policy can be judged until this spread is reduced.

## Three axes of concurrency

llama.cpp's `-np` historically tied the number of KV partitions, the number of server slots and the concurrent inference width
to a single number, and even after unified KV removed the first meaning, the other two remain
tied (upstream discussion 22401 asks for exactly this separation). This
contract separates all three from the start.

| Axis | Meaning | Owner |
| --- | --- | --- |
| `kv_capacity` | Logical capacity of the shared cell pool (n_ctx) | Store convention, OUTER plan |
| `max_resident_sequences` | Number of sessions whose state can be kept alive in KV | L1 registry + L2 acceptance. The requested value comes from the coordinator; the physical limit is the per-stage intersection (below) |
| `decode_parallelism` | Number of sequences to put in this step's UBATCH | Chosen by L3 at every step. Bounded by the runnable count and the row budget |

KV residency and compute concurrency are separate dimensions: even with 20 sessions resident, only
4 may be put into a step, and nodes execute only the membership they are given (the membership replay
invariant). Ownership, however, splits into two layers — the **requested value** is set by the coordinator, and
the **physical limit** differs per stage:

```
effective_resident     = min_i( resident capacity of stage_i )
effective_decode_width = min_i( n_ubatch/backend limit of stage_i, edge credit )
```

A CUDA 3090 stage and a Metal/CPU stage cannot be assumed to have the same value.
Also, `n_seq_max` is not an intrinsic capability that the backend discovers and reports; it is a
**configuration value** that OUTER sets when creating the context, and HELLO merely returns
it — an echo of the contract, not a discovery.

Two caveats came from measurement. First, `max_resident` is not free —
the SWA cache cell count is `n_swa×n_seq_max+n_ubatch`, proportional to this value (measured
12,800 cells), and the sampler graph metadata budget is also proportional to active sequences (the 368-byte shortfall incident
in the 256-session experiment). The residency limit has its own cost term in cell accounting.
Second, the current OUTER plan sets both `--n-seq-max` and the scheduler width with a single `parallel`
— the separation happens in plan P3's acceptance/occupancy implementation.

A "resident sequence registry + runnable scheduler" without a fixed slot count is the logical end point of unified
KV, and that is exactly L1 and L3.

## Persistence protocol

`CacheAction` (PreparePersist/Persist/PrepareRestore/
Restore/Reconcile) and `CacheReceiptState` in `p4-adapter` already contract this structure.
Session state is fragmented across the N participating nodes (`stage_id`, `operation_id`,
`generation`), so every persist and restore is multi-step coordination.

The execution order of persist, restore and truncation (including the restore decision ladder) and the 2PC convergence rules are
owned solely by [kv-state-store-convention.md](kv-state-store-convention.md). The 7th review
found that the flow summary an earlier revision kept here had drifted out of date relative to the convention,
so the summary itself was removed.

The current implementation (`llama_stage_runtime_kv.cpp`) already has `seq_rm` after save, and
`llama_synchronize` + position check after restore. Remaining defects: a 128MB per-node
limit (long sessions need chunked persist), and `kv=0` because `--kv-root` is not set.

## Model diversity: 3-axis gate

A strategy module may exist only in the intersection of three audits.

- Axis A — stage split: `linkcpp_stage_residency_supported` opt-in.
  Currently OPT-IN: kv_cache, kv_cache_iswa, memory_hybrid, memory_recurrent.
  DENIED: msa, dsa, dsv4, hybrid_iswa.
- Axis B — unified sequence separation: KV is separated by the KQ mask, but if auxiliary state
  (an indexer, for example) uses only position as its key, sequences collide (the qwen4exp case).
- Axis C — backend conformance: the concrete backends below the llama abstraction layer (CPU/CUDA/
  Metal/…) must pass load, cut tensor
  alias/view, batch split, Persist/Restore, TrimTo and **numerical equivalence** for each
  `{memory_family × backend}` combination. llama.cpp itself does not guarantee bit-for-bit identical logits
  across backends or batch compositions, so the criterion is defined
  so that it can be judged: with a fixed model, prompt and seed, per-dtype/backend NMSE and
  absolute/relative error bounds, and agreement of the greedy token sequence (or top-k order). A Persist→Restore round trip
  on the same backend and a cross-backend move each have their own separate
  criteria. Even with the same abstraction layer, concrete backends differ in buffer layout and
  compute path, and this equivalence is not a public llama.cpp contract.
  CPU is required at every pin; production backends are required before promotion (plan U0).

Combinations that do not pass are rejected fail-closed at load time, as today.
## Tolerance to llama.cpp updates

Two layers are distinguished. The claim "after a pull only values change, not code" holds only for
the first layer below.

- **Policy layer (strategy, ledger, acceptance)**: does not link against llama.cpp and sees only HELLO negotiated values,
  the GGUF-derived cost table and calibration constants. When upstream diverges, the values
  diverge and this code does not.
- **native compat layer**: patches llama core internals (context, graph, memory, loader),
  so an update is not a value change but a **semantics-based rebase at every pin**.
  In the d7a207411→d7bd3bfc dry-run, 5 of 24 patches conflicted (observed in the 5th review:
  model-loader header, public API impl, stage/recurrent residency,
  MTP tail). This cost is owned by the per-pin compatibility gate (official prepare + conformance),
  and the patch queue is managed as a 3-way split — stage hooks / independent upstream fixes / model and speculative
  feature ports — so that upstream absorbing one independent fix does not conflict together with the whole
  port (plan U0).

The current implementation lives in the staged adapter's `src/v2/`. Whether a separate crate improves the dependency boundary is decided
after mechanism and policy are actually shared and verified. Creating a crate or replaying goldens alone does not claim the shared transition is complete.
## Per-layer attribution of past failures

| Observed failure | Attributed layer |
| --- | --- |
| 40-request position discontinuity (slot reuse) | L1 detects the invariant violation; identifying the cause and fixing it is plan P1b |
| 0 mixed batches | Merge/shape/arrival/policy observation; depth and actual device overlap are measured separately |
| 5GB of wasted load after gemma-4 load rejection | L1 cost table + preflight before load |
| Qwen3.6 VRAM overflow discovered after the fact | L1 cost table (computed before load) |
| Fixed cost of 81 cut-set transfers | L5 + cost_model |
| compute buffer over-allocation (3.5% occupied) | n_ubatch at plan time (OUTER input) |
| Random session failures on cell exhaustion | L2 — unified has no isolation, so policy provides it |

## Introduction order

Phases, order and acceptance criteria are owned solely by the [distributed batching roadmap](distributed-batching-roadmap.md).
This document owns only the L0–L5 contract and does not restate the order.

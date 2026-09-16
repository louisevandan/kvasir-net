# Distributed batching verification and real-hardware acceptance rules

Created 2026-09-06. **The T/I/K/H tests below are requirements; any item without a PASS record is unimplemented/unverified.**
The [roadmap](distributed-batching-roadmap.md) owns the current status and run order.
This document owns the inputs, verdicts, evidence and prohibitions of the tests. The looser criteria of historical documents cannot replace it.

## 1. What the gates mean

| Layer | What it proves | What it cannot prove |
| --- | --- | --- |
| T: deterministic/fake stage/integration | State transitions, boundedness, return consistency, fairness, failure convergence | Real model quality, GPU parallel execution, real-hardware TPS |
| Native conformance | The real compute, state and split contract for a declared model/memory/backend combination | Overall performance under a strong service wave |
| H: real multi-computer wave | Useful throughput, GPU utilization and sustained operation of a very large distributed model that produces normal responses | A global optimum across other models/devices/links or an unbounded search |

Do not look for a fix direction with H before passing T. Do not claim the product goal is complete before passing H.
Process READY, a HELLO match, 40/40 terminal, absence of U+FFFD, RPC peak or average GPU util cannot, each on its own, count as success.

## 2. Test constraints that apply to every fix

1. Save the failing counterexample first. Write down whether it fails on the existing code and why it must fail under the intended contract.
2. Keep normal paths and negative paths together. An implementation that blocks everything by adding more rejections must also fail.
3. Compute value invariants independently. Avoid identities that compare implementation-produced counters with the same formula.
4. Keep unit tests that call a shared function directly separate from tests that reach the real consumer (worker/simulator).
5. Every critical fix must fail when the fix is removed or under an equivalent fault mutation. **If the mutation passes, that test cannot be promoted to a gate.**
6. Test-only fault injection must not widen the runtime public API. Never restore mutations with git checkout/reset in the user's checkout.
7. Starvation checks examine every request's progress and the first selection, the gaps between selections and the time after the last selection. They include ready counts below, equal to and above capacity.
8. A virtual-time split test first asserts that real in-flight work remains at the split point and that the second segment creates a new trace.
9. An error checker must not run unbounded on errors or re-walk the full history every tick. Give it fixed tick/memory limits and injection tests.
10. Async/worker tests check wall timeout, join completion, residual queues/ledgers and retry/wake count limits together.
    `attempts > 1` alone, which `yield_now` also passes, does not prove backpressure.
11. A golden change needs an independent semantics/position/token-limit counterexample first. Never rewrite a golden just by copying actual output.
12. Do not count a runtime early-return in a required test as a pass, and do not hide it with ignore/feature. Mark tests with external dependencies as opt-in, and treat absence as a failure when explicitly required.
13. While fixing a test failure, do not create substitute completions such as passing only by shrinking normal requests, relaxing the judge, reducing concurrency, reducing nodes, forcing restarts or extending timeouts.
14. Whether code is shared and whether tests protect both paths are separate claims. Do not classify test level by file location or test name.
15. Bind the real source digest, build inputs and test binary to every mutation. Use a separate Cargo target/build directory or
    confirm that the mutation was recompiled. A run where a shared cache reused the baseline binary is invalid as both pass and fail evidence.
16. On rejection, compare not only the effect queue length but also each intent's target, body and order, and the native call counts.
    The receipt retention window limit does not replace limits on active flights, queues or RSS.
17. For budgets across several control commands, do not read individually checked results as an approval of the total. For input that passes
    each individually but exceeds the total, the first native call must also be 0. Do not spend a later operation's scheduled receipt reduction
    in advance as headroom for an earlier operation. A standalone retry or exact replay of the same operation must still proceed.
18. After wire validation is tightened, an existing negative test that now fails only in the encoder is not a worker regression. Tamper explicitly with bytes
    built by the normal codec, or construct a different semantically valid identity, so that the input reaches the real consumer.
    Do not report a test that the new guard stopped before it reached the original short/count/role fault as a PASS for the earlier fault.
19. Check for trial-and-error regression at the start, after running the first counterexample, after the fix, before commit and before a real-hardware run. First write down the
    causal path for state/ownership/capacity, the hypothesis to falsify, the expected observations and normal-progress conditions, and the change scope. When an unexpected failure
    appears, re-examine the existing hypothesis and preserve that input. Do not switch to the next knob/layer without an explanation,
    do not fit expectations to observations, and do not add tests or tools that do not answer the same question.
    Report separately a counterexample reachable in code versus an executed RED, and a local GREEN versus full product completion.
    Documents, fixtures and check scripts that the full test reads are also inputs. Finish lightweight format checks first, and
    do not edit inputs during a run. A code seal alone does not justify claiming a seal of the full verification including documents.
20. This follow-up work limits repetitions of the pre-pinned verification bundle to at most 3 rounds. This does not mean
    shrinking to 3 individual tests or reducing H's required repeat samples. The first round includes all normal, rejection, saturation and
    recovery conditions of the fix. The second round allows only the causal explanation of the first failure and the fixes it requires, and
    verifies the critical fix's removal mutation as well. The third round is the final check of the seal candidate. If a new cause or design
    change is needed, or required verification remains, do not claim completion; judge it as needing a redesign.
    Do not change the bundle, inputs or expected values mid-run, or exclude failures to make the count fit. For each round,
    record the run command, the reason for changes and the rounds remaining; if it passed, do not run unnecessary second/third rounds.

## 3. Required deterministic test list

The IDs below are reused in per-stage PASS tables and run logs. When one test covers several IDs, mark each input/assertion.

### T00~T04: baseline and counterexamples

| ID | Input / what must be checked |
| --- | --- |
| T00 | Do not read mismatched source/runtime/build/model/workload/summary identities, git failure, untracked files, binary changes or record-channel failures as clean/success |
| T01 | Verify the call map of the current event path; reject reports that substitute past Chain/Hop test results |
| T02 | Current normal seed: prompt length 0/1/multiple chunks, max_tokens 1/2/long generation, continuity of the first token and decode positions |
| T03 | Full baseline suite run, exit code, pass/fail/ignored/feature-excluded tally; distinguish missing external fixtures from functional failures |
| T04 | Move the R-A/B/C counterexamples into repository tests; do not close them with a temporary local probe alone |

### T10~T19: publication and settlement

| ID | Input / pass condition |
| --- | --- |
| T10 | 4 rows issued/6 rows returned, open batch registered. After rejection, request, open_batches, credit, reservations and output intents are all unchanged. Check not only the tail but also the handle's later persistent state |
| T11 | Mixed A normal/B wrong return, swapped request order, late outcome/sequence errors. When the whole event is rejected, A is also unchanged; 0 external side effects |
| T12 | F1 normal settlement → F2 publication → F1 redelivered with a new event ID. F2 outstanding/position preserved; ledger and requests agree. Identical content is a no-op/receipt; different content is a conflict |
| T13 | Partial prefill without an outcome: wrong sequence/key/generation/phase/position range, unregistered execution, duplicate/missing rows and owner/invocation mismatches are all rejected up front |
| T14 | One logical batch split into several physical capsules. Partial returns alone must not release the batch/sequence; only the last valid return completes it, once. Ordinary splits are allowed, but splitting a Verify/Replay atomic group across capsules or reordering it internally is rejected |
| T15 | Cancel after plan creation, partial reserve failure, failure before stage accept, response lost after accept. planned/issued/settled are distinguished and converge to Uncertain; 0 double publications/missing rollbacks. On the real drive's reject → replan, fairness/cohort resume is not consumed either |
| T16 | The last prompt fragment generates the first token; with max_tokens 1, 0 extra decodes. Decode positions advance by 1; verify/replay positions and generation counts follow their own explicit contract |
| T17 | Inject a malformed travelling fragment into the Simulator's real advance. Same rejection and state preservation as the worker. A mutation that replaces the simulator's shared call with separate bookkeeping must fail |
| T18 | ID wrap/exhaustion, reuse of the same slot and return of a previous generation, late completion after cancellation. Redelivering an old RELEASE/RELEASED/SETTLE/SETTLED to a new incarnation that reuses the same load/session/request key/slot preserves every stage's new KV, reservations and slots. Do not approve on the head nonce alone. Also check the same operation with a different body/kind, retired incarnations and the watermark limit |
| T19 | issued rows = settled rows + travelling rows + explicit cancel/failure accounting. Cross-check request counters against the identity ledger independently; mutations of the error cap and incremental checks |

### T20~T28: real worker and fake stage

| ID | Input / pass condition |
| --- | --- |
| T20 | Real event ingest → worker loop → stage API → capsule → tail → release → output. N=1/2/4/8; distinguish from tests that call only the tail directly. Even while the producer keeps filling the input, issue, completion and control each get a bounded processing opportunity |
| T21 | max-open 1/2, multiple capsules, partial/last returns. Confirm the point where the real second issue is blocked and then released |
| T22 | Input queue Full: the event is preserved and delivered exactly once after the space notification; no spin or permanent wait on an empty completion mailbox |
| T23 | Output mailbox Full/Closed, broker/network saturation: 0 lost or duplicated computed tokens/terminals, control progress guaranteed |
| T24 | timeout/reconnect/duplicate/reversed/partial messages. Physical phase/range order violations are not hidden by settlement. On duplicate downstream physical delivery, native KV/sampler effects are 0 beyond the first approval; a valid opcode with a wrong body on a mutating native call also results in Uncertain/fence and 0 further executions |
| T25 | load/unload/cancel/release racing with decode. No deletion while native is in use; 0 slot reassignments before release evidence from every stage |
| T26 | shutdown while input full/output full/stage blocked. Join within the deadline, explicit completed/cancelled/uncertain state; no unbounded drain and no hiding of losses |
| T27 | Per-host shared GPU/independent GPU/slow stage models. Distinguish dispatch count from device concurrency; do not use fake latency as GPU performance evidence |
| T28 | repeat run without agent restart, reuse after partial failure. Ledgers/memory/queues return to the declared steady bound |

T23's bidirectional saturation is not replaced by a single output queue recovery test. Two capacity-1 EventNodes connected to a real broker
each hold a completion destined for the other, and the normal broker input is then filled.
If the adapter can accept input, both sides must make progress by processing inbound even while outbound is Full.
If the adapter is also Full, measure 1 held input and 1 held output each plus the residual events in the next bounded queue;
after space recovers, the full Event bytes/order must be preserved without extra consumption, discard or duplication. Do not extend this local obligation
into proof of deadlock resolution for a fully saturated cyclic network. That verdict needs end-to-end credit/control capacity.
Another condition of the test is that cleanup must not add a second panic on top of an assertion that already fired and so mask the original failure.

T23's **control progress inside the same worker** is also an independent obligation. Advance A on two real stages through native release and
hold the exact RELEASED. Fill the capacity-1 completion with B's real terminal, then confirm the input
acceptance of A's ACK. A's settlement must proceed before the output space opens. Then restore space and keep the positive case in which the existing A/B
token/text/position/stop, native KV, OUTPUT/release receipts and observations complete exactly once.
The test name for the current counterexample is `completion_full_cannot_starve_a_genuine_release_acknowledgement`.
Confirm together that a new Full is observed in the snapshot and that the real B OUTPUT is in the queue; a 200ms wait alone
does not count as saturation evidence. This test's time limit is a regression window, not a real-time SLO or a proof of no CPU spin.

A yieldable pump must pass the following in addition to the normal ACK test above. An early ACK where only pending is installed, or only local
native is applied, and the control Forward has not yet been accepted, is rejected as a whole event. After rejection,
pending/slot/Verify fence, native calls and effect targets/bodies/order are all preserved. Mutations that apply an old effect success notification
to a different incarnation/operation, or treat a native replay as ForwardAccepted, are also detected.
Keep SETTLED's normal terminal proposal and checkpoint replay, and check that no other control interleaves inside a command's native loop.
The count and byte sum limits of effect/held inputs, repeated errors/SESSION,
Full → recovery, Closed, ID exhaustion, shutdown and fatal ERROR preservation are each tested, and none passes by blocking everything.

Regressions for the head control stage are recorded at two boundaries. `control_progress_tests` builds a pending state
and then checks whole-event rejection through the real codec → Worker::handle and normal resumption on the same ACK.
`control_dispatch_effect_tests` builds the load/session/KV initial state and then checks the real flush_effects →
native Frame/P4ID → receipt/frontier → completion acceptance. It also covers response loss/tampering after a native change,
cached replay, Closed/ID exhaustion and a wrong last member. Do not count the former test's handcrafted
ForwardAccepted as a real send history, and do not count the latter test as proof of Worker::run/real llama.
Mutations that remove the phase check/success hook, promote early or regress replay must fail without changing expected values.

SESSION's up-front response preparation is checked separately in the real session()/handle(). On ID exhaustion,
session/ID/effects are unchanged; when a wire-valid input produces a summed envelope over the limit, SESSION authority is unchanged;
a normal response keeps the exact wire, consumes the ID once and preserves the Unicode original. The fact that the general ERROR fallback can still
produce an unreceivable diagnostic with the same large original ID must not be mistaken for a successful normal rejection notice.
Detect mutations that install before preparation, omit/duplicate the ID commit or remove the decode preflight, and do not extend a pass of this SESSION test
to the unmigrated LOAD/UNLOAD/ERROR or to overall count/byte reservation and Full progress.

For the effect representation migration, compare the exact Events of OUTPUT/observation/control using the same input with a large TAIL body,
and confirm that provenance is a type that does not own the payload. Check the content, order and owned allocation of the remaining body and
nested telemetry after a send failure, and do not re-run a successful Forward because of a later observation failure.
Do not approve removal of the front clone based on a mere increase in test count or an estimated RSS reduction.

Budget regressions check together the positive case, where a pre-approved ACK proceeds with its own future notification/ID reservation even while the general area is exactly full,
and the up-front rejection of new work that lacks such a reservation. They include the ID headroom boundary and sum overflow,
a command where only part of the reservation succeeds, multiple OUTER groups, small payload/large capacity, telemetry fan-out,
and LOAD/UNLOAD/error response lifetimes. Mutations that treat a space notification as a reservation or remove the pre-ACK reservation
must fail on the real consumption path. Do not add the local ACK test and the FIFO/broker full saturation test together and
call them one proof.

The capsule's declared count check verifies rejection **before** the Vec reservation in the real decoder on a small invalid wire.
Use the minimum wire size of outcome headers/generated tokens and the remaining cursor bytes to block impossible pre-allocation, but do not arbitrarily shrink the existing
valid count limits. A huge-count negative test must be stoppable before the allocator call by a test-only
safety device; do not use OOM on the development host as a verification method. Original payload
length, parsed object capacity, temporary serialization copies, allocator overhead and total RSS are different quantities.

The independent completion tests below were, at the 2026-09-07 stop point, **written-only candidates that were not run**. The resume/run budget
follows roadmap §0; this does not mean they are already GREEN or should be run now.
T22/T23's independent completion progress is checked with a real EventNode and a real destination slot. A different correlation
removes the original only after securing a slot, and the same source/correlation cannot overtake even with a different destination.
Full or a front mismatch preserves the original allocation, ledger and slot, and terminal returns both the existing bidirectional held items and the candidate.
The broker separately checks duplicates/conflicts, the receipt pin during eviction, rejection precedence for a wrong Envelope/sequence,
generation/channel changes and reservation cancellation. The actor prevention branch must, while keeping the head un-polled and destination capacity 0,
attest together to genuine R's full pending identity and the real OUTER delivery of the normal C1.
The 14 inputs, 6 results, 8 responses, native and the final normal_progress verdict are not reduced. Mutations that remove independent progress, remove the same-order
region check or remove the front match check are detected in a separate checkout with a real recompile.

T22/T23's **neutral space notification** is distinct from the actor test above. Pin the order listener registration → real offer retry,
and check drain/close before, during and after registration with barriers. When the last publisher exits, an empty reader's
Pending is woken and becomes Closed, and buffered Events must be consumed first. When the last receiver exits,
the waiting publisher is woken and gets its original Event back as Closed. Check callback re-entrancy, self-deregistration,
registration of another waiter, the lifetime of the last clone, the listener limit and capacity recovery after release, and waking outside the lock.
Mutations that remove notify or wake before the real close must fail. The mere existence of this primitive
does not justify reporting that the current staged worker's 1ms wait or the EventNode input retry has been removed.

T20's sustained inflow is separated from a brief merge of two waves. With a runnable request placed first, an independent
Tokenize fake feeds the next valid PREFILL into the input, causally keeping the queue nonempty.
With chains of 0/16/256 and longer inputs, the number of events processed before the first issue must satisfy the declared bounded opportunity
contract. Finishing eventually after all input has been drained does not pass. In the same way, check tail/control processing opportunities
while publication remains possible. This contract's quantum is an actor fairness bound,
not an optimal TPS value or a wall-time deadline for a blocking native call.

T20's speculative path puts full acceptance, direct partial acceptance and checkpoint Replay into the actual run separately.
Cross-check the literal tokens, positions, generation counts and KV append/trim/restore/reappend at the native boundary independently, and do not loosen the ordinary
once-per-position append oracle. Distinguish atomic group splits from ordinary splits.
SETTLE is a chain that walks the stages, and the final SETTLED is a single response coming from the tail to the head. Hold the last
SETTLE hop and the final SETTLED separately, and confirm each stage's real effect. The test for the current global Verify
barrier also includes a separate runnable request to prevent the false positive of stopping merely because the target request is not ready.
Detect mutations that release the barrier after full acceptance or drop Replay output, and cross-check release on every stage and the new incarnation.
Distinguish the observation that there is no reuse until RELEASED from an independent proof of the free-slot return mechanism alone.
A pass of the token/KV fake is not approved as normality of real llama logits, sampler or checkpoint bytes.

T20's OUTER boundary checks the OUTPUT of the real Worker::run together with real InferenceIdentity/drive consumption.
Preserve the encoded Events of ordinary and checkpoint Replay, and the current producer must also match the same semantic projection.
State the reason for per-run event_id/causation_id/sequence values that may be excluded from the projection, and separately check ID uniqueness,
causation existence, per-source sequence increase and Event validity on the real set. The full source/target/return route
endpoints, protocol/class/content/adapter/correlation/deadline and every Outcome field are not normalized arbitrarily.
Distinguish the arrival order of different requests from the token order within one request. Reject tail/middle/other heads, old generations and wrong
routes; check duplicates of the same Event and output after terminal on the real drive. Mark the scope of a simple receive helper versus the
full drive, and in-memory wire versus real network/GPU, separately. Mutations that change the producer to tail publication or
change the consumer to tail-only/any-node must all fail.

T20's completion verdict is separate from the output publisher check. The following are required acceptance conditions, not a declaration that the current implementation is complete.
On both the real drive and the final acceptance, check the per-request submission limit (an empty EOS also counts as a sampled token), the allowed terminals,
and the relationship between length termination and the exact limit. The first output position is not replaced by a single value chosen for all requests;
it is bound to each request's tokenize/approved prefill boundary evidence. Include counterexamples with different prompt lengths, 2 outputs under a limit of 1,
and all positions of one request shifted by the same amount. Also cover missing, duplicate and reversed separate observations.
Release completion is not judged by summing totals alone. Cross-check it against the set of actually approved request/attempt/release operations, and
a duplicate notification with a fresh event ID must not stand in for another request's release. Also keep the positive control in which one normal notification releases
several requests. Do not break that cross-check with a temporary fix that deduplicates only on correlation.
The test's release oracle must be independent of the payload's self-reported count, and if evidence is
impossible with the scalar-only legacy wire, change the adapter command version explicitly and check fail-closed consumption. Do not put
model-specific completion semantics into the P4 transport. Source approval, terminal output, KV release on all stages and the OUTER receive ACK are different evidence.

The fresh-prefill consumption regression makes the current normal peer, after receiving the real submit command, return observations and results of different lengths.
Cross-check both sides for a missing observation, an execution duplicated under a different name, the same ID with a different body, exact retransmission and reordering.
An observation after OUTPUT is a positive case before final completion/release; it does not mean that reconnect convergence of later late observations
has been verified. When a whole observation candidate with A normal/B wrong is rejected, and on sum overflow, the counters of both requests
must stay unchanged. Independently re-check the raw sampled limit of the final artifact and the observation-to-first-position cross-check.
A mutation that removes the actual drive's budget call or the final observation apply must break not only the helper unit test but also the real
consumption regression. A mutation that changes the producer's prefill observation to decode must be detected, on the same input workload,
by the real producer observation cross-check, separately from the token/KV oracle. If synthetic
observations were added around an existing OUTPUT capture, do not report that envelope as captured from the real producer.

T20/T25's release boundary checks the batch contract's release authority chain on both the real producer and consumer.
SESSION declarations are tested both before and after installation. Rejection of post-install immutability alone does not prove
validation of a wrong initial declaration. Check forward installation for each role, redelivery of the same declaration, empty/single/duplicate stages, a wrong index/
receive endpoint/generation and the old wire. Also pin the real OUTER production function and the receive expectation together.
Negative source/target cases for every stage message family must pass codec → real handler and produce the exact route rejection.
Confirm that, with a normal source/target, the original body/ledger negative tests still pass the corresponding semantic checks.
Keep a normal multi-stage run and wrong-sender injection in the ACK-held state together; neither the accept-all nor the reject-all mutation
may pass. Do not count the envelope gate's empty-state matrix as a real native/ledger effect test.
First assert that A/B terminals from different OUTERs are in the same physical batch; each one's
full target/return route, correlation, deadline and explicit membership must return exactly. Also keep the positive case that bundles
several requests into one OUTER. Build the expected set from the real submission/approval data, not by reading the generated receipts.
On Full/Closed/event ID exhaustion of the first/later notification, check the already committed KV and the remaining notification intent
separately. An implementation that resends the same control after an error and increases the native release count must fail.
Internal ACKs are tested with a normal tail positive control plus middle/other head/external source, an old endpoint generation and a wrong
session/operation/incarnation. Even if the body is an exact pending member, when the source role is wrong,
the head's free/admission/pending/fence/effects must stay the same. Distinguish the execution scope of the direct released call, the real handle, the broker/
run-loop and the native chain. Do not add up the success of one path as a pass of the remaining boundaries.
Restart of a new OUTER/Worker and key/slot reuse within the same loaded Worker are checked separately, and a previous attempt's
fresh-ID receipt must not be able to approve the new attempt's terminal/release. Also include rejection of the old scalar receipt,
no change at all for candidate A normal/B invalid, no termination before all members are confirmed, and no increase from duplicates.

When the completion wire changes, do not overwrite existing captures. Capture the original PREFILL, OUTPUT and receipt together from a real run
as the new version, and keep the token/text/position/stop, physical width and KV oracles. A separate legacy projection that removes only the explicitly named new fields
must also match, and the new-version cross-check preserves **every payload field**. The explicitly named volatile `event_id`/`causation_id`/`sequence` of the OUTPUT/receipt
envelope are excluded only from semantic equality, and
separate uniqueness, sequence progress and causation existence checks remain. The original PREFILL is cross-checked exactly, including its envelope.
Fixtures in which the consumer builds the expected submission ID from the received OUTPUT,
or attempt fields are attached to captures after the fact, are prohibited. Cross-check the real sent original against the
terminal approval and the head RELEASE/native control bodies as different observation points.

Also test **different valid values** of the original Envelope and ReplySpec. Beyond wrong address syntax, confirm ingress/
channel/connection/correlation/deadline/source/target/return_route mismatches and full preservation of slots, pending,
effects and native calls in both A/B orders. Negative PREFILL source/return_route/target cases are rejected before recording/acceptance,
and a correct declaration of the same original must keep proceeding in the real handler. Fixture migration is limited to the point where normal inputs are
generated; do not neutralize negative tests by automatically normalizing the source of every input.

The consumer-side attempt registration → real send order is also checked with a failing writer. An exact fresh-envelope receipt
redelivery increases releases by 0; the policy of rejecting the same envelope ID is separate. Pass receipts without a terminal, a different attempt/slot/
incarnation/operation, mixed members, redelivery variants and a missing remaining B through the real drive. Do not merge pure ledger tests with
separate consumption tests. Independent mutations that bypass registration, cross-check or atomic apply must also be detected by the real consumption test.
Do not judge this saturation from a stale Full metric snapshot alone. After confirming no ACK sent, receive waiting and an empty queue,
set the observation baseline and wait for new saturation; do not count a test-only observation reset as a real state transition.

The multi-OUTER positive case feeds not only releases but also the actually generated OUTPUT/BATCH_OBSERVATION/STAGE_SPAN to each real
consumer. Distinguish a missing observation of one's own request, exposure of another owner's request, confusion between the physical total row count and the owned row count,
and per-request double counting of the same span. Passing the positive case by turning off the general unknown-request check is a failure.
Mutations that delete all observations and mutations that copy the full request body onto every route must each be detected. Keep the existing
path that bundles several requests into one OUTER together with the original physical width and per-request boundary oracles.

The target contract for observation completeness follows the batch contract's **per-owner observation and publication evidence completeness** section. The following are
the counterexamples required for T20/T25/T57/T58; internal ledger tests do not replace production/consumption completeness tests.

- Connect the producer's originals to a consumer that sends the same envelope/command as the real original submission. Do not call explicit tokens
  input and prompt input the same thing. When using a fake Tokenize, cross-check the call's real prompt and response,
  but real llama tokenizer/quality proof is separate.
- If the last observation/span is held after the terminal OUTPUT and the release receipt, the consumer has not yet succeeded.
  If it is delivered within the existing overall timeout, it succeeds; if it is permanently missing, it fails. Do not use a quiet sleep as the completion condition.
- Keep the prefill observation but delete the intermediate Decode/Verify/Replay observations together with all their spans. A workaround that
  cross-checks only received executions must not pass. Also reject execution/range substitutions that preserve the count and phase sums.
- A logical issue ordinal missing a request, physical fragment order/reversed arrival and exact redelivery are handled by the defined canonical rules.
  A different body for the same identity, an execution overlapping different span groups and a different attempt/slot are rejected.
- In a logical batch containing an execution that only A belongs to and one that only B belongs to, do not require B-only spans for A.
  An A/B shared execution shows each OUTER only its own rows, but the physical total width and cost are the same and are not re-summed.
- A lower-priority owner error checks that the ledger/witness/effects are entirely unchanged; a lower-priority fan-out Full/Closed/ID exhaustion checks preservation of the unpublished
  intent and 0 native re-calls. Mutations that remove the publication approval hook or compute the witness only at observation send time
  must be detected by both the actual worker and the actual consumer connection tests.
- Appending a long generation history does not grow the evidence state size of the RequestState candidate clone, and each arriving observation does not
  rescan the full history once per request. Check the fixed-size evidence and touched-work counts independently.

The executed sub-scope of internal issued-work on 2026-09-07 is as follows. It is not a promotion of the full observation gate.

- `issue_witness/tests.rs` compares against independent Node literal inputs/bytes/digests. It does not re-save values that Rust output
  as goldens. It checks the order-swap positive case, a different interior position with the same count/min/max,
  substitution of the execution and each authority field, normal Verify/Replay position reuse, and ordinal gaps/overflow.
- `node/issue_witness_tests.rs` goes through the real `prepare_issue`/`begin_native_issue`/`accept_prepared_issue`.
  On a lower-priority owner error it preserves not only committed requests but also the PreparedIssue candidate, flight, numbering and reservation-related
  state. Do not call direct selector/shared bookkeeping tests a full Simulation test.
- `worker/loop_tests/issue_witness.rs` goes through the real Worker::run and native command/capsule/EventWire,
  and views the approval state through a test-only read observation. It checks 2/4/8-stage logical/physical counts against independent
  digests, multiple OUTERs, a real Full, duplicate terminals and a second native error. The read observation cannot
  modify state. The existing exact output token/text/position/stop, KV and release oracles are kept as they are.
- Removing the approval witness installation, committing early before the check completes, and omitting the execution ID from the hash input must each fail
  after a real recompile in an independent copy. Mutate only the production helper and keep tests/goldens fixed.
  Record the exact commands, current source, failing tests and executables in the evidence record.

For the OUTPUT v5/observation v4 migration, beyond the internal tests above, bind **the real production wire and the real drive**.
The batch contract owns the per-version contract; the roadmap's latest record and the evidence index own the run results, seals and unpassed scope.

- Do not overwrite existing OUTPUT v3/v4 captures with the new fields. Capture the new v5 again from the actual Worker::run, and
  preserve the original submission, outputs, receipts, observations and spans together. Distinguish post-approval expectations built by the test from received observations.
  Obtain the expected execution set from a read-only record at the real publication approval point, not by back-computing from received observations.
- Reject on the real drive a missing terminal witness, tampered revision/count/ordinal/digest/authority, and deletion of all intermediate issue observations.
  Mutations that remove live production's witness installation/terminal copy must also fail separately.
  A test that replays a pinned wire into the consumer alone does not count as checking the changed producer.
- Put two OUTERs and several requests of one OUTER together in a real physical batch. Feed each route's original to a separate real
  consumer, and do not build the positive case from a synthetic body with the foreign owner removed. An empty foreign projection and
  global physical counts mean different things. Also cross-check the grouping, membership, row count and stage order of the same execution.
- Reject timestamp/body conflicts for the same span and overlapping execution groups. Swapping list order and exact redelivery are
  normal. Numeric execution IDs with a different endpoint/load/session are not merged into the same execution.
- Even if OUTPUT and release finish first, wait for observation completeness without extending the original deadline. Terminate even when the next wave is
  scheduled later than the deadline. Pin the existing throughput elapsed to the release boundary, and preserve the late observation
  completion time in a separate nullable field. Detect mutations that turn observation delay into TPS loss.
- Check preservation on mailbox Full/Closed/event ID exhaustion in the real effect pump. After a successful forward,
  the span time is pinned only once. This is the local completion mailbox approval time, not a network arrival ACK.

Do not assume that input identifiers are valid under the lower approval contract just because the P4 envelope allowed them.
Formats that would be rejected after native are checked at PREFILL ingress before tokenization and ledger changes, and keep the positive control in which a later normal submission
runs to completion on the same worker. Do not pass this by turning it into an unconditional worker exit.

The submission row string boundary is checked with a real run in `submission_limits.rs`. The literal serialized ReplySpec
is built at 4095/4096/4097 UTF-8 bytes from ASCII, JSON escapes and multi-byte characters, and the raw options preserve the whitespace after valid JSON.
Cross-check normal output, KV and release for both tokens and prompt, and resubmission with the same request ID/new session key after an oversize rejection.
Do not report success of the wire size limit as success of the real native option syntax.
Mutations that remove the ingress guard, defer it after session key recording, count chars instead of bytes or reject at the exact limit must each fail.
The Rust codec and the model-free C++ `physical_wire_test` also cross-check independent literal boundaries. A mutation that raises only the native limit
and detection of false asserts in Release are done in an isolated copy, and they do not replace the full CTest or model inference.

T25's explicit UNLOAD is sent in the real run-loop after separately creating a held ordinary tail return, a held speculative final SETTLED,
and residual KV on a middle stage. Always include a middle stage with no requests and a settlement wait with 0 flights.
Confirm exactly 1 error response to the exact caller/generation, 0 UNLOADED, and unchanged native calls, KV, sampler, release
history and request output. Then, without regenerating the same request, resume the original return, complete the literal output and
release on every stage, and the idle UNLOAD must succeed. Removing the busy guard, checking requests only,
and unconditional busy rejection must each fail. A wrong generation on FIRST/LAST is also rejected before native.

A normal busy rejection and a post-hoc native cleanup failure are different paths. If an idle UNLOAD fails in native shutdown,
the error/uncertain termination is kept, and even an already queued later SESSION must not be ACKed. A mutation that removes this fence
must also fail in the real run. A fatal failure or Drop cleanup of a worker that is already fenced/Uncertain may close
native resources, so the healthy-busy promise of 0 native calls does not apply to that path. A local stop point is
not a global drain or delivery ACK of the input queue, peer stages or published mailbox; Cancel/Drain is a separate test.

T20/T26's actor shutdown test distinguishes no input from a severed input. Once one logical publication
has proceeded, the next prepared work must be publishable without new input, and after a real input EOF/stop is observed
no new native request starts. Inject a normal SESSION inside the first native execution and check that the ACK comes out before the next
voluntary logical publication. This processing count limit is not claimed to guarantee interruption of multiple
native operations inside a single PHYSICAL/SETTLE event or of a synchronous call already running.

Shutdown classification preserves, before cleanup, requests, pending, release/settle, prepared issues, flights, effects, real owner/frontier and
PHYSICAL Running/Uncertain/fence. A completion receipt/Released tombstone is not unfinished work, but
Stopped KV and Uncertain without an active attempt are still unfinished. Do not collapse each ledger's independent evidence
into a single total. Announcing `closed` first during unload, or attaching an unload failure only after the normal prefix, is a failure.
Preserve the original failure, the cleanup failure and the last event rejection separately. Even with 0 local remainder, settlement of already published
mailbox/network results and peer KV is separate, and this alone does not pass graceful drain.

T24's native continuation keeps a normal opcode/codec and token budget and sets only the proposal width to
physical capacity+1. It passes through the PHYSICAL/SETTLE response and head return approval separately; with width 1 and the exact
capacity, the subsequent Decode/Verify must proceed. An error after native must have no success output/receipt and
0 further native calls, and unlike an up-front input rejection, it does not claim a KV rollback.

T24's **new execution ID position bypass** is kept apart from the receipt duplicate test. First send a normal
partial Prefill → final Prefill → Decode to the real middle/tail, then send a past position, a future gap and a Prefill regression separately.
Mix normal A/wrong B in both orders and check 0 native calls and full preservation of owner/frontier/receipt/effects.
Always keep as normal controls: the next publication without SETTLE after full Verify acceptance, direct partial SETTLE, and an exact Replay after checkpoint restore.
Do not pass the negative tests by unconditionally rejecting Verify or unconditionally requiring SETTLE.
Arbitrary SETTLE, tampered Replay, an old round or changed proposal tokens are rejected before any real effect, and a wrong native response is
checked for 0 further native effects after the fence. A mutation that removes the pure check and one that removes the real worker wiring are distinct.
This method path does not replace T20's full Worker::run/network or a real llama model run.

T24's duplicate execution sub-tests include the following. Passing part of this list does not complete all of T24.

- Deliver the same PHYSICAL to middle/tail with a new event ID: the fake native actually changes KV/sampler on every call, and
  the check is 0 second calls and a replay of the first response. An effect-free echo fake alone does not prove idempotence.
- The same numeric ID from different configured heads executes normally and replays separately. Changing each of the head endpoint's agent/node/generation
  also distinguishes them. The same ID from the same head with a changed session or input/tensor is a conflict.
- cached+Fresh mixes, Fresh followed by a conflict and the reverse: full up-front rejection atomicity, the real Fresh subset and the original result order.
- Just below, at and above the receipt byte/count/Seen/issuer budgets; first normal delivery of a large result and rejection of expired redelivery;
  per-head ID windows, a low normal not-yet-received ID and out-of-window rejection. Detect mutations that remove the limit or merge issuing authority.
- After release/slot reuse, an old Replay does not acquire the new owner or revive native KV. An event with only Replay
  produces no new compute span, and the span of a mixed event contains only the executions actually computed.
- The counterexample that sends a past position/future gap/different phase of the same sequence with a new execution ID is a **separate frontier test**.
  A test that rejects only cached IDs does not replace it. Restart, reconnect, credit and retention period are also separate.

### T30~T38: acceptance and credit

| ID | Input / pass condition |
| --- | --- |
| T30 | Just below, exactly at and above the pending count/bytes/prompt tokens/deadline limits; explicit queue or reject, 0 silent deletions |
| T31 | Different per-stage KV unit cost/shape/SWA/recurrent auxiliary budgets. The number that can be resident is the intersection of reservations on every stage; 0 over-admission |
| T32 | Failures during multi-node reserve Prepare/Commit/Release/TTL/reconcile. Prepared is included in accounting, stale commits are fenced, 0 leaks from partial acquisition |
| T33 | Exhaust edge row and byte budgets independently. Small rows/large tensor, large rows/small tensor, fan-in. 0 overruns of the negotiated limit |
| T34 | Duplicate/lost ACKs, reconnect, cancel, previous epoch. The same ticket is returned once; bounded queue/RSS even while nothing is returned |
| T35 | Fragment credit reclaimed but compute/KV unfinished. Snapshot/slot reuse does not proceed |
| T36 | A large prompt after a small prompt and the reverse order, continuously arriving decode/prefill. Acceptance wait and runnable wait each bounded, with explicit deadlines |
| T37 | Change the resident limit, decode width and ubatch independently. Changing one setting does not silently widen another budget |
| T38 | Multiple fragments 1/2/4 combined with credit shortage/queue saturation/cancel. Same model semantics, row accounting and maximum memory preserved |

### T40~T47: policy

| ID | Input / pass condition |
| --- | --- |
| T40 | ordinary attention: several prefill lengths + decode. Legitimate budget filling and per-request starvation prevention; exhaustive comparison with a reference allocator on small states |
| T41 | equal-width: ready sequences 4/7/8/16/64, capacity 8; decode 1 + prefill 17; reverse direction. Individual first/middle/last gaps and progress |
| T42 | Permutations of demand array order, dynamic arrival/completion, sparse IDs/wrap. Same selection and settlement for the same logical state; detect mutations that use the index as identity |
| T43 | Detect prefill width collapse when cohort separation is removed; tests that look only at total cohort counts are prohibited |
| T44 | Verify plan budget and physical split separately. Logical batch>ubatch, a short last chunk, row preservation across mixed/equal-width/atomic splits |
| T45 | Work-conserving reason cross-check: a reason for every eligible row not sent; detect mutations that count requests as rows |
| T46 | Fixed seed trace + independent invariants + run(A+B)=run(A);run(B), split/additional arrival during real flight |
| T47 | Conflicts among age/deadline/priority/credit/KV constraints. In an overload where absolute fairness is impossible, the outcome is explicit rejection/SLO failure, not an unbounded-wait success |

The cost checks of T19/T45 are also required. Measure with independent counters that, as prompt length grows, each token's publication/settlement candidate does not re-copy the immutable tokens or
the original event payload, and that, as the number of unrelated open batches grows, a single return does not re-search every
owner. Do not reduce cost by removing safety checks.
An incremental implementation must still pass the test/debug independent full-ledger cross-check and the existing rejection and atomicity mutations.

### T50~T58: native and harness

| ID | Input / pass condition |
| --- | --- |
| T50 | Enforce build/model/ABI/capability verification on the product LOAD/inference path. Bypass paths that agree only in the driver fail. Reject physical support without an execution identity revision or with an unknown one, a different bind echo, a lost response and reuse of the same generation, and do not publish the slot. Binding tests do not replace state ABI/placement verification |
| T51 | Report host UUID/device UUID/backend/plugin/NUMA/MIG/Metal/multi-device/host fallback as the real placement; agreement by matching the `CUDA0` string alone is prohibited |
| T52 | Pin pristine prepare, patch classification, private header gate, CPU + declared production backend CTest; reject stale DLL/cubin/mixed deployments |
| T53 | Three axes: per-model stage residency, unified sequence separation and backend conformance. Numeric/state gates for load/alias/view/split/logits per memory family and for the KV/trim features in use |
| T54 | Per-host record [begin,end) at both ends, byte length/digest, strict UTF-8, sticky record failure/control signal. Same negative tests locally and remotely |
| T55 | Sampler serial baseline. A parallel candidate solely owns synchronize/output reorder and is split into independent logits/sampler, verified with fixed seed/membership/TSAN or an equivalent race check. A real-hardware pass alone does not rule out races |
| T56 | Multi-agent/host deployment and placement spec, real per-host binary/model hashes. Reject fixtures that report multiple machines based only on remote SSH access |
| T57 | Per-request submit/admit/prefill/first/terminal times, token IDs/text, stage/edge/device spans. If cross-host clock error is not measured, the overlap value is unavailable |
| T58 | Negative fixtures for report/judge: missing/duplicate/wrong request, broken UTF-8, empty response, wrong answer, different run, stale log, failed channel, tampered hash |

### I00~I09: required tests for layer isolation and upstream tracking

Check the responsibility/dependency boundaries of the [layer isolation contract](layer-isolation-contract.md) with real builds and change falsification.
Passing only the existing private-header string gate does not turn this table into PASS.

| ID | Input / pass condition |
| --- | --- |
| I00 | Injecting a concrete backend back-reference/FFI into the Rust normal dependency graph fails. Policy/ledger/model tests build and run without a llama checkout, GPU or network, and the mock event boundary is kept. Also reject workarounds that put phase, KV or llama sequence semantics into the generic capacity/delivery path |
| I01 | Inject private/common/ggml-src include violations into the consumer for both the full build and imported relink. Real compile failure; check transitive INTERFACE paths too. Allowed bridges build normally |
| I02 | Mutations that put common/private types into public signatures/forward declarations, or give a consumer access to internal headers/Impl/raw/plan_params, fail. Allowed compat white-box tests succeed. 0 pointer/upstream ordinal leaks in pure DTOs; an engine-only tensor codec binds a separate identity/code table and rejects unknown dtype/flags, ordinal meaning changes and mixed-codec fleets |
| I03 | Adapt semantics-preserving private/common API renames inside the compat module. 0 changes to P4 protocol/ledger/policy sources; trace, settlement and membership match for the same normalized input. Mutations that pass a new native enum/option to upper layers without capability translation fail |
| I04 | Preserve both direct and indirect option consumption of the opaque plan. Golden comparison of parser/default/new unknown options, grammar, sampling and device split. The existing 27-field white-box assertions stay in the compat test target; getter duplication and test deletion are prohibited |
| I05 | Compilable mutations that change KV/state/alias/position semantics. The native conformance or state compatibility gate must fail. Detect workarounds approved by a mere signature/patch clean |
| I06 | Clean pin replay, LF/CRLF checkout, patch classification/dependencies, absorbed-fix candidates. Verify the final prepared tree/hash; unclassified patches or patches outside the allowed scope fail; patch removal is proven by the corresponding regression |
| I07 | Fresh build per declared CPU/production backend, real device/layout, ABI negotiation, and mixed binary/plugin negative tests on product LOAD. Kernel/stream/buffer semantic changes are detected by the corresponding conformance. Do not count enumeration of available backends as the real placement, and cross-check the fix scope of engine-only changes and backend-only changes separately |
| I08 | In Release, an intentional false assert and an internal type violation each fail CTest/build. Privileges of optional internal test targets do not propagate to the runtime consumer |
| I09 | Preserve both the adopted pin move record and synthetic compatible/incompatible changes. Measure fix scope, manual cost and remaining debt; promote through real final-wave non-regression. Rollback to a previous pin is cross-checked separately from the state matrix |

The following are **required sub-cases** of the IDs above, not a record that new gates have run.

- **I00/I01/I02 — real dependency closure**: check the `cargo metadata` graphs per normal/build/dev, target and feature, and
  the compile/link inputs computed by CMake. Actually build a normal public consumer, an allowed compat white-box consumer and
  a private/common/Impl-violating consumer, each with the full source and with imported relink.
  A passing name/string canary does not replace this check. Distinguish link-only propagation of a static library
  from API exposure, and also detect workarounds in which a prohibited call resolves at the final link.
- **I02/I05 + T18/T24 — Rust/C++ dual implementation cross-check**: feed a canonical command transcript of the same version into
  the real codec/ownership consumption paths of the Rust adapter and native C++. Include load bind, first execution, new incarnation,
  exact replay, the same ID with a different body, a late command after release, budget overrun and response loss after native success.
  Cross-check per-input accept/reject/uncertain, KV/native effect counts, response bytes and state preservation against an independent oracle.
  A mutation that removes the guard in only one language must fail. Since both sides may produce the same error,
  agreement between their results alone does not approve anything. When there is no native model run, state that limit as well.
- **I07/T50/T52 — separating source identity from real loaded identity**: inject new-pin headers + previous-pin lib/DLL,
  a different backend plugin with the same name, and a stale DLL that appears first on the dynamic search path.
  Source/lib mismatches may be rejected earlier by package verification, but that alone does not replace runtime verification.
  Cross-check the actually loaded module path/hash, plugin and device/buffer placement against the sealed build manifest, and
  **product LOAD must reject** real load mismatches such as stale/shadowed entries on the search path.
  A combination that could not be demonstrated because of a different OS/backend is unverified, not PASS.
- **I04/I08 + T03 — execution and lifetime of isolation tests**: separate model-free parser/scalar/contract checks into an explicit unit path,
  and vocabulary/real KV/sampling checks into a model-required path. If some bodies in an executable of the same name
  return early, do not count that as a full conformance pass. A missing optional model is SKIPPED;
  a missing explicitly required model is a failure. Check move/clone/destroy of opaque handles and preservation of the original on failure, and
  do not let a regression that dereferences a moved-from value slip past either the normal model path or the model-independent lifetime tests.
- **T53/I02 — separating Replay logical output from native logits**: observe the logits requests that the real FIRST/downstream batch-generation consumer
  passes to the engine. It must request every needed Replay row, and the wire owner/output must
  be preserved. A model-free native API intercept proves only the batch generation body. The real sampler's
  logits access, checkpoint restore and recomputation results need a separate native/wave gate with a loaded model.
  Do not mark this model path complete on mock success alone, and detect mutations that restore the mask at the real consumption point.

I03's identical trace is a condition for identical semantics/capability inputs. If the engine actually changed shape or KV constraints,
the new capability contract and expected results are reviewed separately, and the real constraints are not hidden to match the old trace.

The isolation gate's inputs need an allowed-dependency manifest per real crate/target/module and a public/internal interface register.
Until the lists and runner exist, the I gates are TODO. Do not self-approve by putting an allowlist expansion and a detector change into the same
fix. Check both normal consumers/allowed white-box tests and prohibited violations.
I02's engine-only codec exception does not give generic P4 or L1/L3 authority to interpret dtypes.
T11/T23 must also include tail output → head rejection and partial output publish → Full/Closed,
and look at real external output and native settle/release call counts. Preserving only the head's internal counters is not enough.

### K00~K09: additional required tests when persistence/snapshot features are enabled

These tests check the contract of the [storage convention](kv-state-store-convention.md) by execution.
Do not redefine the convergence direction or identity definitions here. A PASS for stage B does not make K an automatic PASS.

| ID | Input / pass condition |
| --- | --- |
| K00 | Same operation ID, multiple cuts, multiple sessions, shared/node-only roots, colliding digests. 0 namespace collisions and 0 overwrites of other sessions |
| K01 | Late writers of CONTROL/lease/token/epoch, lock reacquisition, host reboot, stale-break. A shared store without authority is rejected; a late publish/release does not damage new ownership |
| K02 | Crash/ENOSPC during each of state/tokens/meta creation, flush and pointer publish. 0 partial combinations in exposed bundles; orphans are not arbitrarily re-indexed |
| K03 | 1-byte tampering of model/variant/sidecar/token files, LoRA scale/auxiliary artifact changes, ID prefix collisions. Full identity cross-check; failing combinations of N-1 writer → N reader and the layout matrix are rejected |
| K04 | Before/after each stage's Prepare/Committing/Committed, coordinator/node restarts, partial receipt loss. Realize the convention's evidence-based convergence, and do not guess at recovering volatile residents |
| K05 | Cache commands racing with compute/release. No export/trim/reuse before quiescence is attested on every stage for the target sequence; other sequences keep progressing |
| K06 | Settle the signatures, versions and immutable snapshot/mutable reference choice of Checkpoint/Fork/RestoreInto/List first. Full tuple cross-check, OUTER restart ledger recovery, branching without aliases |
| K07 | Shorter than the stored prefix/branched/identical request, arbitrary/bounded/none trim, partial TrimTo. The convention's pre-import judgement and all-stage barrier; 0 revived dead suffixes |
| K08 | Source/target storage domains, read-pin versus Discard/GC, failure during move. 0 deletions while a reader is in use; do not report cross-domain as zero-copy sharing |
| K09 | Separate budgets for disk/RAM/resident tiers, chunk size and compression; quota/ENOSPC, restore reservation and TTL races. 0 partial acquisition/large-state leaks; logits/position/normal responses preserved for a fixed model |

Every K test also follows the mutation, real consumption path and failure tally rules. A release with storage features disabled
states why those tests were not run and the unfinished scope. A mere feature exclusion is not counted as feature completion.

## 4. Real-hardware wave contract — the only evidence of final results

### Run order — performance improvement after service integrity

H numbers are test kinds and do not imply run order. The current Release A first passes, under the H0 spec, H1 normal responses,
H2 sustained inflow, H3 overload, H4 complete denominators, H6 real distribution and H7 sustained operation/failures on one source to produce
`integrity_baseline=GREEN`. Before that, do not run H5 candidates, and do not count infrastructure or unit test passes as product
progress. If H5 changes the source or policy, the selected candidate must pass the same integrity bundle again.

Integrity runs must also collect, in the same time window, TTFT, prefill rows/s, useful generation TPS, per-phase physical batch width/fill, blocked reasons
relative to runnable, and GPU sample coverage/util/memory/power. These figures are the P0
baseline, not a claim of improvement. In the performance phase, pin the same model, artifact, topology, resident, KV, corpus,
arrival sequence and output conditions, and change one cause only. If either the single request or the sustained wave is missing, or
normal responses, settlement or cleanup break, it is a product regression, not a performance candidate.

### H0. Spec to approve and seal before the run

The final model's name alone is not enough. Preserve the model/split GGUF/auxiliary artifact digests, total/active parameter counts (MoE distinguished),
quantization, context length, KV format, tokenizer/chat template, sampling/seed and the request corpus.
The very large final model is a model approved by the owner, and the spec states the weight or KV capacity requirement that one machine cannot meet
at the target context/concurrency and the real reason for multi-machine distribution. Do not estimate capacity from parameter count alone.

`multi_host_final` approval requires real stage computation and KV ownership on 2 or more physical hosts.
A `hardware_scoped` run within the current resources follows the per-resource-stage scope below. Record each host identity, device UUID, real placement,
link bandwidth/latency, power cap, driver/backend/plugin, CPU/RAM/VRAM budget and stage cuts.
A LANES string, process count, SSH tunnel or a single remote host does not substitute for this condition.

Seal the values below in `benchmark-spec` before A/B. The current runner does not support all of this spec (B5 work).

- `source_commit`, `runtime_kind=event`, `scheduler_version`, binary/library hashes, compat pin/patch digest.
- `model_manifest`, `execution_layout`, `workload_digest`, `judge_version`, `summary_version`.
- Resident target R, queue count/bytes/tokens, stage KV reserve, edge row/byte bounds.
- Per normal/overload wave: total request count, size, interval, length distribution, token cap and absolute timeout.
- TTFT/ITL SLO, minimum significant improvement, A/B order, repeat count, holdout seed, GPU sample interval/warm-up window.
- Every numeric bound must be a concrete value. Starting a final run with an infinite or undecided core budget is prohibited.

#### Proof scope per resource stage and the RAM offloading gate

The resources currently approved by the user and the order of real-hardware expansion are owned by [roadmap §1](distributed-batching-roadmap.md).
`benchmark-spec` distinguishes `resource_tier= vram_only | ram_offload`, the real `physical_host_count`, and
`coverage= hardware_scoped | multi_host_final`. In single-host verification, H6 is not PASS but
`BLOCKED: only_one_physical_host`, and the results of the remaining applicable gates are not hidden.
This distinction is a B5 requirement for spec/verdict implementation, not a fact about the existing runner's implementation.

**VRAM-only sufficiency** must satisfy at least all of the following. A single smoke, GPU utilization or token count alone does not promote to the next stage.

- Pass the relevant T/I safety and real execution identity gates, an audited family/backend, and confirmed real placement on every stage.
- General control, tokenization, the CPU sampler and host staging are marked separately, but if the computation, compute weights or KV of the model layers
  actually executed depend on CPU or host RAM, `vram_only` approval is prohibited even if that was the intended configuration.
  Do not confuse model file mmap, CPU marking of non-owned layers that are not computed, or RAM for general control with model offloading.
  The classification is decided by the real compute/storage placement, not by option names.
- H1 on the sealed normal corpus, cold/sustained/recovery H2, a separate overload H3, and H4 raw data/denominators pass.
  Prove normal waves and H7 sustained/drain stability at the approved large model's target context/resident.
- Every normal response is preserved in full, 0 errors/losses/session contamination, and the up-front memory/SLO budgets are met. If performance optimization is claimed, H5 must also pass.
  Do not force a new TPS gain figure into the VRAM-only baseline approval itself.

**The RAM offloading spec and negative tests** add the following to the conditions above. A test that reports only planned values without observing automatic fallback fails.

- The real layout and runtime options that distinguish CPU/host-resident weights, experts/layers and KV, GPU-resident parts, NUMA (when used), compute placement,
  and pageable/pinned staging. A requested value such as `-ngl` does not replace the real placement.
- Seal numerically the host RAM budget after subtracting OS/existing service headroom, the resident set/commit, the pagefile/swap policy and the KV/queue/credit
  budgets. Do not treat the sum of two VRAM cards and RAM as one continuous pool.
- An expected shortage of headroom is explicitly rejected before load/admission. Check false admission, no response, leaks and model quality loss on counterexamples with forged headroom, reservation failure, CPU fallback, transfer delay, cancel and
  drain. Do not cause unbounded OOM or disk exhaustion on an operational host.
- In the same analysis window, collect host RSS/private commit, available RAM, CPU usage, page faults and paging I/O (when supported),
  host↔device transfer bytes/time, and GPU memory/power/util. For unsupported metrics, record unavailable with the reason.
- Pass H1~H4/H7 again for the larger model. Numerical error from batch/backend changes and the model's intrinsic quality are judged separately.
  A small VRAM-only model success or an identical API call is not substitute evidence for large model + offload conformance.
- Policy comparisons apply H5 within the same offload layout. A VRAM-only↔offload comparison on the same model, where possible, is
  a separate resource placement experiment; do not combine TPS from different models and report it as a policy improvement rate.

The inventory's unit of evaluation is the logical model + exact artifact set/variant. Candidates that could not be verified still
get a concrete status such as `unsupported_family`, `insufficient_budget`, `artifact_incomplete`, `path_inaccessible` or `not_yet_tested`.
Do not complete verification of all candidates because a filename carries a large parameter count or a load merely succeeded.

### H1. Normal prompts and responses

- Do not repeat a single fixed question. Use a normal request corpus: general explanations, summaries of documents of different lengths, questions about given facts,
  small code/table/formatted results, and so on. Do not pad long inputs with meaningless repeated sentences.
- Use only public/synthetic data. For each request ID, preserve the raw prompt, the templated text, the real token IDs/count,
  the full response with token IDs/positions, the stop reason and the judge's reason. Preserving only representative outputs abbreviated with `...` is a failure.
- Separate the required language/format, question relevance and correct-answer checks. Both a machine-checkable oracle for the controlled corpus and
  a hash-bound semantic review of the full general responses are required. Passing semantics on a TypeScript keyword count alone is prohibited.
- The normal service gate requires **every request to pass** structure, settlement, output and semantics. Transport 64/64 with semantics 55/64 is a failure.
- A fixture that the model itself cannot solve is diagnosed separately, then versioned identically across all arms and fully re-run.
  Do not swap in a different seed for only the failing arm, or relax the correct-answer criteria after the fact.
- Do not unconditionally require bit identity across stochastic runs or different batches/backends. Declare in advance the controlled same-layout token/position baseline and
  per-backend logits tolerance, and keep the natural-language quality gate separate.

### H2. Strong overlapping waves

Development smoke/mixed runs are auxiliary. Final normal service includes all three modes below.

| Mode | Minimum structure and required observations |
| --- | --- |
| Cold burst | Submit at least R requests together at time 0. Record from tokenize/prefill through fill. Do not hide this cost with prewarming |
| Sustained waves | At least 8 waves, at least 8R requests in total. Mix short/medium/long normal prompts in each wave, arriving at a fixed interval shorter than the initial baseline's completion time |
| Recovery waves | Keep the same agent/model load and run load→drain→next wave blocks at least 3 times. Do not avoid O12 by restarting on every run |

Arrivals are pinned by an open-loop spec, and the interval between A/B is not adaptively changed. If the client waits for responses and delays requests,
it is not the promised wave. Record the real send skew and delay against the target; exceeding the tolerance is INVALID.
In Sustained, the first token of a later wave must appear before the initial wave completes, and overlap of decode/prefill
active intervals between repeated waves is proven per request. Do not claim overlap merely because the pipeline has many pending items.
A configuration that fails to saturate because all requests finish is re-run with a stronger wave intensity, **with both arms entirely under the same new spec**.

### H3. Overload and acceptance

In a separate overload spec, apply a concurrent burst of at least 2R, above the resident target R.
Within the declared budget, queue/serve; beyond it, return explicit rejection/deadline results. Keep allowed rejections and their exact reasons
separate from the count of normal service successes. Request loss, failures of innocent resident sessions, over-admission and unbounded RSS must be 0.
Do not remove requests rejected under overload from the denominator and report it as if TPS went up.

### H4. Metrics and denominators

- Primary metric `useful_generation_tps`: real generated tokens of requests judged normal / (from the first scheduled send to the last terminal).
  It includes admission, transport and wave gaps. The first sampled token is counted, but prefill input and speculative draft/rejected tokens are not counted as generation.
- Separately record total generation TPS, useful/failed request counts, prefill input rows/s, decode model-evaluation rows/s and total rows/s.
  Do not mix these into the same TPS column or hide a difference of a single completed token.
- TTFT is **each request's real submit → that request's first token**, and ITL is that request's real output interval.
  Do not call the first-token time measured from the start of the run TTFT. Also report the queue/tokenize/prefill intervals separately.
- Report, for each GPU within the same analysis window, sample coverage, mean/p50/p90, zero%, memory peak, power,
  and useful kernel active time/SM/memory metrics (when supported). If unsupported, it is unavailable, not 0.
- `nvidia-smi utilization.gpu` is a GPU active-time metric, not SM occupancy or useful FLOPS.
  Do not report stage RPC spans, rather than the device, as "concurrent GPU computation".
- ubatch fill is computed as physical batch rows / the real ubatch limit of that stage, with prefill/decode/mixed separated.
- Report stage queue/compute/sample/copy/encode/network wait, per-node KV used/reserved/free,
  edge credits, runnable/blocked/pending, rejection reasons, inflight peak and the remainder at drain end.
- Overlap across multiple machines includes clock synchronization error. Overlap smaller than the error is not confirmed.
  Request end-to-end time is measured with the same client's monotonic clock, and device spans with each device's appropriate clock.

### H5. Optimization comparison

The default comparison changes only the policy under **the same model, requests, topology, KV capacity, resident limit and backend settings**.
Node/cut/placement changes are a separate experimental axis and are not added to batch policy improvement figures.

Run at least 8 paired repetitions (e.g. ABBA/BAAB balanced blocks), alternate arms without running them concurrently, separate warm/cold windows,
and run a separate holdout of at least 4 pairs. Preserve GPU temperature, clock, power, background work and order effects.
If the baseline is bimodal or drifts, do not merge it into one mean; report each run and the distribution.

Default promotion criteria (to change them, version the spec before the run):

- H1/H2/H4 pass for every normal service arm; deleting failed arms is prohibited.
- Median paired useful generation TPS improvement ≥5%, the lower bound of the 95% confidence interval of the paired improvement >0, and the improvement direction holds on the holdout.
- TTFT p95 ≤ 1.10× baseline, ITL p95 ≤1.05×, and the up-front absolute SLO is met.
- An improvement that sacrifices KV/VRAM/RSS/credit limits, token count or quality is invalid.
- Record the Pareto result of GPU util/device active and TPS together. If only util rises and TPS falls, promotion is prohibited.
  If TPS rose but util fell, keep it as an efficiency improvement candidate, but **do not report that GPU utilization also improved**.
  Under saturating load, trace the cause of intervals where useful work was ready but the device was idle, and state any shortfall against the final joint goal or physical limits.
- Preserve the chosen search range, candidates, exclusion reasons and neighboring candidate results. Use "maximum" only within this range and these limits.

≥5% is the default product improvement threshold, not a law derived from past figures. A release that re-baselines an already improved baseline
does not claim a new performance gain and marks itself as a non-regression verdict. Do not lower the threshold without owner approval.

### H6. Real distribution evidence

Every participating host must have that run's model shard/KV ownership, stage execution, device trace and deployment hash.
Collect connection/transfer byte evidence that traffic actually crossed host boundaries. Do not claim the model is distributed across multiple machines just because only the GPU process is remote and the client
is on another computer.
If CPU fallback or another backend is used, mark it in the real placement. Do not allow state compatibility because
a different backend family has the same name. Execution that flowed to an undeclared host/device is INVALID.

### H7. Sustained operation and failures

On the sealed final candidate, sustain normal waves for at least 60 minutes or 32R total requests, whichever is longer.
Repeated waves on the same load, cancel, slow/broken edges, node restart and late returns run as separate failure arms.
The normal arm needs 100% normal responses, the failure arm needs 100% convergence to the predefined completed/cancelled/failed states, and no response, loss and contamination of other sessions are 0.
After the run, ledgers, reservations, credit and queues must return to the baseline state, and host RSS/VRAM must converge within the up-front allowed range.
If repetition is possible only by restarting, the product goal is not complete.

## 5. Evidence bundle and reporting

Create an immutable directory for every run. Preserve both successes and failures/aborts. Do not complete with a link that points only to `target/`.

```text
<run-id>/
  benchmark-spec.json        # sealed input from H0 above
  provenance.json           # source/runtime/binaries/models/hosts/layout
  requests.jsonl            # raw/template/token IDs + scheduled/actual send times
  responses.jsonl           # every full response/token IDs/positions/stop
  judge.json                # per-request structure/semantic/correct-answer verdicts, reviewer, version, digest
  telemetry/                # host/device/stage/edge/queue/credit raw data
  report.json               # every denominator, coverage, verdict and missing list
  commands-and-exits.txt     # reproduction commands and exit statuses
  checksums.sha256
  failure.json              # cause on failure/abort; reusing an earlier success report is prohibited
```

This layout is the target contract. The current `config.json/artifact.json/report.json/gpu.csv` are existing formats that hold part of this information;
on migration, preserve the field/format versions and the mapping to raw data.
If raw data is large, upload it to an accessible long-term artifact store and record the location, full digest and recovery command in the Git evidence.
Do not record accounts, tokens or personal data. A local temporary path that cannot be accessed again is not long-term evidence.

**Git inclusion review:** do not track raw data just because it is small. Include the source to keep, real regression tests, the required
minimal fixtures and concise contract/verification records; keep generated logs, duplicate source manifests, one-off probes/archiving
tools, binaries, models and build caches under the existing ignored paths. Promoting a run-specific tool to a shared
tool requires a reusing consumer and a path-independent run test. Do not ignore fixtures or
required tests to hide failures. If there is no long-term storage location for raw data, leave the re-inspection/reproduction condition as not met;
do not substitute an unauthorized upload or copying the originals into Git to close it.

When code or a run is completed, leave an index entry in [runtime-evidence](runtime-evidence.md) and
the details in `layers/adapters/llamacpp/staged/scripts/validation/evidence/<date>-<topic>.md`.
The record includes the source commit, tests run/not run, failing counterexamples, all arms, the access path to the full responses,
verdict limits and next work. Do not fill in verification not done in this report with past passes.

## 6. Commands available now

Based on PowerShell at the repository root. Run GPU/remote commands after confirming the relevant resources, permissions and the H0 spec.

```powershell
git status --short --branch
git rev-parse HEAD
cargo test --workspace --no-fail-fast
cargo test -p p4-llamacpp-staged-adapter
npm run docs-lint
npm run test:docs-lint
node tools/scripts/docs-lint.mjs --all
$harnessTests = rg --files test/benchmarks/p4-4node -g '*.test.mjs'
node --test $harnessTests
node layers/adapters/llamacpp/staged/scripts/validation/validate-private-headers.mjs
```

Re-check the current pin at HEAD and in `layers/adapters/llamacpp/staged/compat/`. The following paths are the audit baseline pin.

```powershell
node layers/adapters/llamacpp/staged/scripts/validation/validate-compat-manifest.mjs --manifest layers/adapters/llamacpp/staged/compat/0eadefebd/manifest.json
node layers/adapters/llamacpp/staged/scripts/validation/validate-patch-classification.mjs --manifest layers/adapters/llamacpp/staged/compat/0eadefebd/manifest.json
node layers/adapters/llamacpp/staged/scripts/prepare-pipeline-upstream.mjs
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --backend cpu
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --cuda --cuda-architectures '86;89'
```

`86;89` is an example from past development devices; set it again from the real fleet capability. Do not pass PowerShell `;` without quotes.
Run CTest in the real build directory/config that the build script printed. Do not use CTest on stale binaries as evidence for new source.
The existing `node test/benchmarks/p4-4node/run.mjs smoke|service|prefill_mix_35b` commands are for development diagnostics, and
there is not yet a runner that automatically enforces all of H0~H7 in this document. Run each scenario as a separate command.

<a id="v11-gates"></a>

## 6.1 Applying verification to the v1.1 integrated diagnosis

Planned addition on 2026-09-11. Only [roadmap v1.1](distributed-batching-roadmap.md#v11-plan) owns the run order.
The following are per-change verdict conditions that have not been run yet, and they do not relax the existing T/I/H/K gates.

| Change | Required counterexamples and consumption path verification |
| --- | --- |
| Observation | Recompute per-request admitted/eligible/blocked, issue/settle and byte state at the same instant. Preserve the first error and approved partial evidence even on failure after delay/rejection/partial OUTPUT. Compute the real ITL from consecutive OUTPUT receive times |
| Byte acceptance/return/receipt | Inputs with small counts but large payloads, accumulating completion receipts, duplicate replay, budget boundary ±1, unknown delivery results, release/settlement control progress. Ledger, reservations, credit and output effects are identical before and after rejection. Do not arbitrarily delete still-valid duplicate-detection grounds with TTL/LRU |
| Decode bundle/prefill quantum | A counterexample that spends 16 decodes in one bundle, cumulative unfairness among long requests of equal length, prefill starvation under continuously arriving decode and the reverse case. Published token ranges/membership are not reordered or split afterwards |
| Multiple prefill fragments | A fragment arriving ahead of a stage's KV prefix, duplicate/reversed returns, later fragments remaining on error/cancel, recurrent/verify/replay boundaries. Decode outstanding≤1, node backend concurrent execution≤1. Mutations that remove the fragment limit or the order check must fail |
| Shutdown/reuse | Preserve in-flight sends and unknown native results during timeout, a control channel that can still progress, slot reuse and idle UNLOAD only after settlement is confirmed on every stage. A forced process kill alone does not substitute for passing normal cleanup |
| Memory/offloading | Compare the device-KV-first plan with the real backend allocation; distinguish peak RAM including CPU weights, VRAM, and agent heap/receipt bytes. Keep rejecting insufficient configurations. Double-counting unified memory is prohibited |

Every functional fix is proven by failing counterexample → real consumption path → recompiled, hash-bound mutation in an independent worktree.
Backend neutrality tests apply to the core byte contract, and the corresponding I gate applies to adapter/native changes.
If the relevant recurrent/hybrid/backend combinations are unverified, exclude them from the multiple-fragment promotion scope and keep them disabled.
The UTF-8 regression is verified separately, preserving the existing 69~113-token failing input/seed/binary baseline and fragment boundaries.

Real-hardware runs pin a per-model/topology baseline and candidate, and separate the load, prefill, decode, mixed and cleanup analysis windows.
GPU samples and RPC spans are separate metrics. Global overlap/hop latency without a cross-host clock error bound is kept as a reference value only.
Report token count/elapsed denominators, unfinished, failed and length-terminated requests, and concurrent load together. A single-run best or a best-of-3 selection is not H5 approval.
H5's minimum of 8 paired repetitions and 4 holdout pairs, the useful TPS/confidence interval, and the relative and absolute TTFT/ITL SLO conditions apply unchanged.
Acceptance of a declared configuration is recorded only when normal shutdown, content quality, long waves, settlement, UNLOAD and memory stability all pass together.

<a id="hf-integration-contract"></a>

## 6.1.1 HF adapter integration acceptance (2026-09-14 plan)

Work scope, implementation/repository ownership and prerequisite order follow the [HF acceptance plan](external-analysis-improvement-plan.md#hf-integration);
fix boundaries follow the [isolation contract](layer-isolation-contract.md#external-hf-boundary). The HF-* entries below are new planned test IDs.
Past results of the initial plan follow the HF acceptance report, and the current source migration follows the migration report. Independent Python worker tests are not promoted to a P4 integration pass.

| Planned ID | Input and real consumption path | Required verdict |
| --- | --- | --- |
| HF-PKG | Locked build that actually depends on the HF crate from the P4 root, package metadata/tree, implementation → `Arc<dyn RetainedNodeAdapter>` wiring. If a feature is used, separate builds with it on and off | Uniqueness of package ID/source/version for each of `p4-adapter`/`p4-protocol`. An independent negative fixture with a duplicate source fails type wiring or is rejected by the graph gate. Verify the internal workspace and single-source restoration. Record exit/summary for both the full P4 workspace and the HF member tests |
| HF-REGISTER | Real agent event NODE_LOAD/INSPECT. Enabled/disabled kinds, wrong kind/generation/capacity. Load llama and HF nodes on the same agent | LOAD-able kinds match what is advertised. A disabled HF is rejected before adapter/worker spawn and model allocation. Qwen model names do not appear in P4 branches. Existing llama keeps the same Agent-target lifetime contract |
| HF-RETAIN | Full/Closed/peek/matching take/poll wake with queue/completion cap 1 and a small fixed byte limit. The real RetainedEventNode and broker consume the HF adapter | The original allocation/claim is returned, peek consumes nothing, a stale front is not consumed, no lost capacity resume/wake. Queued/held/IPC/output authority preserved. Rejection at the bytes boundary ±1 has no effect; input acceptance is kept separate from execution/downstream acceptance |
| HF-IPC | Inject magic/version/reserved/length corruption, partial header/body/write and flush failures, stdout pollution, ready mismatch, excess stderr, worker death and hangs into the real Rust↔fixture Python binary pipe | Incompatible or over-limit input is rejected before tensor/model execution. No arbitrary stream resynchronization or automatic issue re-execution after failure. Uncertain, the first error and cleanup errors are preserved separately. Waiting on blocking Python I/O inside a nonblocking retained call is prohibited |
| HF-LIFE | P4 NODE_LOAD → several requests → cancel/Release → NODE_UNLOAD → NODE_LOAD of a new generation. Cancel/UNLOAD while executing, while holding results and with the queue saturated; arrival of stale results/releases | Keep cancel acceptance/publication stop separate from confirmation that the worker stopped. Reject UNLOAD while unreclaimed output, an unknown snapshot or live state exists. Absent succeeds only after child exit and reclaim of physical state, route, owner and claims. Bypassing the node task health and output lifetime checks is prohibited |
| HF-MODEL | Run the pinned Qwen dense FP32 below on an independent controller and on the P4 path. Chunked prefill, decode, request interleaving, cancel, slot reuse, DeltaNet/attention cuts, supported CPU/GPU combinations | Keep the existing logits atol=0.125/rtol=0.01 and per-step greedy identity criteria. Compare not only state positions but also the composition/content parity of the declared cache against per-request references. An FP32 PASS does not cover the existing heterogeneous BF16 FAIL. Separate original, quantization and distribution error |
| HF-DIST | On 2 real physical hosts with the same target and precision, each P4 node owns an HF worker and its assigned weights/state. Boundary tensors and tail results are passed by P4 events | Evidence of host/PID/device, source/model/plan hash, allocation, sent/received bytes and request attribution. Fails if a central Python walks all workers directly. Verify settlement and the next normal request after disconnect/late reply. Several local processes do not run this gate |
| HF-SWAP | With one P4 binary hash: compatible Python bundle A → normal UNLOAD → LOAD bundle B → same task. Also feed bundles with different IPC/schema/identity | A compatible swap needs no P4 recompile, and the actually used bundle hash changes. An incompatible bundle fails LOAD and is reclaimed. A Rust bridge change requires a separate P4 binary/hash. Overwriting a bundle during execution is prohibited |

### HF pinned test configuration and deliverables

- The contract fixture tests boundary ±1, large payloads and output saturation with a separate profile: input/completion queue 1, retained count 2, 4KiB per store and an IPC payload of at most 1KiB.
  These figures are not the limits for real Qwen readiness/tensors.
- The real model uses the Qwen3.5-0.8B revision from the HF manifest. The first integration profile is FP32/quantization none,
  24 layers split [0,12)/[12,24), context 2,048, request limit 8, output cap 64, physical batch 1 and 1 outstanding step at a time.
  Keep the original `short/chunked_prefill/interleaved_cancel` scenarios and the legal attention boundary profile.
  Use the approved FP32 configurations for CUDA/CPU, and pin the physical host/device identity in the manifest before the run.
- Derive the model profile's integer store/IPC/RSS budgets by adding up the existing worker's 32MiB frame limit and 64KiB readiness metadata, tensor/state sizes and copies, and the P4 retained/receipt
  lifetimes. If any field is missing, reject before the run. Do not report the frame limit
  as the total heap limit. Pin a per-request timeout of 120 seconds, LOAD readiness of 120 seconds and idle shutdown confirmation of 5 seconds as
  the initial integration regression limits, distinct from performance SLOs. On overrun, perform error/isolation and cleanup of owned children within a separate 10-second limit,
  and do not allow an unbounded join. A run with these limits raised after the fact is not the same acceptance arm.
- The additional operational regression processes and reclaims a block of 8 requests 3 times on the same load, re-admitting the next requests between blocks.
  This is impossible under the current cumulative limit `active+retired≤max_requests`, so verify a new execution epoch/duplicate-prevention lifetime contract
  separately from the old v1 contract. Confirm rejection of late steps/results/releases from a previous epoch and that they have no effect on state/claims;
  an implementation that passes only the 24 requests by deleting the duplicate-prevention check or raising the cap fails.
  If only `short` succeeds and cancel/reclaim/regeneration fail, acceptance is unfinished. Separate output-cap termination from EOS/normal answers.
  Keep the conformance teacher-forcing comparison separate from the review of full general generations, correct answers and format.
- Each changed authority/limit/identity check is proven by a real consumption counterexample and an independent fix-removal mutation. Seal the Rust recompile and binary hash
  together with the Python source/import path and bundle hash. If a feature is used, put the HF-enabled tests into the required CI/run commands, and
  do not mark HF verification as passed because it was excluded from the default build.
- Keep the independent/integration runners, fixtures, profiles, environment lock and run/result reports in the HF adapter folder; P4 keeps the creation/advertisement/common contract regressions and
  the command and result index that reproduce the tests at P4's pinned revision. In a restore test that starts without the sibling, an explicit build
  bundle must prepare all internal sources from a single P4 commit. A build where only a temporary path
  happened to exist during the test is not reproducible deployment evidence.

HF acceptance completion is completion of the integration capability above. A small Qwen does not approve H0–H7/performance promotion for a very large model.
Without a physical host/bridge source, the item is BLOCKED/unimplemented, and waiting for a later A success does not substitute for acceptance.
Full workspace results such as the existing llama timer RED are preserved separately. Do not classify a regression newly introduced by HF as an existing failure.

<a id="release-a-contract"></a>

## 6.2 Release A acceptance contract (2026-09-13 plan)

The [development plan Release A](external-analysis-improvement-plan.md#release-a) owns product scope, models and implementation entry points.
The following is **the target contract for new development**, not a record of implementation or real-hardware passes. It does not replace the existing T/I/H/K or v1.1 regression conditions.
The A- prefix marks planned test IDs. The implementation report records the mapping between real test functions/runners and IDs.

### A run spec and cost check

**2026-09-16 H0 v3 real seal:** the [benchmark-spec](../test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v3.json) and the
[H1 second run INVALID report](../tests/reports/release-a/20260916_104300.md) bind source 25edd33cf, the binary/model/3-host/layout,
resource bounds, the H1–H7 workload, A/B, telemetry and the LOAD/UNLOAD lifetime. H1 quality is
a max_in_flight=1 RELEASE closed loop, and the per-request E2E deadline and per-class TTFT/ITL nearest-rank p95 are
all judged against real artifact timestamps. Missing samples and order inversions are fail-closed. It rejects H0 v1/v2, open-loop
quality and missing deadline/run/verdict authority. H0's verdict is
load_authorized=true, runtime_acceptance=false, and it is not read as having passed H1–H7 below.
H0 v1/v2 are kept only as historical evidence of their reports.
Per the user's 2026-09-15 instruction, the target changes to the development plan's Qwen3.5-122B-A10B UD-Q5_K_S 3-shard with resident 8. It uses at least 2 physical hosts, and the exact fleet/stage/cut is sealed after a new PLAN, the shared pool budget and a cost comparison. The past 550B's 7 hosts/8 stages are not applied as is. This release's
acceptance goals are streaming for short queries and asynchronous analysis of long documents. The new figures below are not measured predictions but
product usage limits. Instead of an arbitrary improvement rate, they put upper bounds on user wait and job completion. Feasibility is checked in A-COST,
and if infeasible, that release is FAIL/scope re-review. Relaxing figures after the fact does not turn an existing arm GREEN.

**2026-09-16 H1 first run counterexample:** H0 v1 quality submitted all 64 requests at time 0, and after 30 minutes only 8/64 had
completed and been RELEASEd. Resident/pending limits do not imply that every request can be served within the product deadline.
The [run report](../tests/reports/release-a/20260916_084650.md) is preserved, and new LOAD approval under H0 v1 is stopped.
H1 quality judges all 64 requests with a RELEASE-based closed loop on the same LOAD, and only H2 cold/sustained
owns open-loop overlap. H0 v2 and H1 are not approved unless the per-request deadline vector exactly matches the corpus and is
consumed as real event deadlines.

| Input/operational item | Value to pin before the run and verdict |
| --- | --- |
| Product context/generation | After template application: short 2–8k, medium ~32k, long 100,038 input tokens. Sequence context 102,400, output cap 2,048. Check that input+cap fits in context for long as well. A `length` early stop on a normal response is a failure |
| User wait limits | Per-class TTFT p95: short 60 s, medium 300 s, long 900 s. ITL p95 ≤250ms for each class. End-to-end deadline after submit: short 600 s, medium 1,200 s, long 1,800 s. Queue wait included |
| Purpose of the limits | A 100k input is an asynchronous job of at most 30 minutes, not a real-time chat promise. At 250ms/token, generating cap 2,048 takes about 512 s, which together with the 900 s long TTFT budget leaves operating headroom within the deadline. This does not guarantee every token interval or a measured capacity; per-request deadlines are checked separately |
| Initial load/shutdown | Full cold LOAD/SESSION within 3,600 s; normal drain/idle UNLOAD within 30 s after the last completed job. Request cancel acceptance/new publication block within 1 s, graceful reclaim within 30 s. If native stop is unknown, switch to failed/uncertain and quarantine within 30 s, and do not count it as a normal reclaim PASS |
| Fault recovery | Block results/new work of the isolated epoch. After owned process exit and device/reservation cleanup are confirmed, perform the new epoch's cold LOAD/SESSION within 3,600 s and re-admit a normal recovery wave. A host that cannot be confirmed stays BLOCKED; guessing automatic remote KV cleanup is prohibited |
| Admission | Resident 8; pending queue at most 64 requests/128MiB serialized request/6,553,600 input tokens; at most 2MiB serialized input per request. Reject explicitly before exceeding. Actually check input bytes per UTF-8/tokenizer. Apply both the overall max token and byte limits |
| Host/device/edge/output/receipt | Use the owned weight/KV/auxiliary state/maximum physical result/scratch from the native PLAN and the real available and host shared pool from INSPECT to derive **integer byte limits for each stage and edge** in the manifest. An input budget alone is not a substitute. State count×max serialized response, the separate lifetimes of pending/retained/receipt, and double charging of shared payloads. Reject before the run on a missing required field, infinity, a negative value or exceeding the remaining amount |
| Current observation spec | GPU/host sample period 1 s, sample coverage ≥95%; detailed cross-host overlap figures are judged only when wall-clock host skew is within 10ms. If unmet, that analysis is INVALID and is not filled with 0ms. Same-host monotonic latency and client TTFT/ITL are preserved separately |

The memory byte values above are not pushed onto the machines as estimated fixed constants. PLAN→manifest materialization is
a required implementation. Seal the generated integer budgets and environment before A/B, and state the host reserve from the executable amount after subtracting the OS/existing services and real
allocations. Do not proceed if current available is exceeded. Start the policy A/B after the topology/quant/cut
selection is finished. Limits in the config that grow automatically during an experiment are prohibited.

### A corpus and comparison arms

1. Preserve the existing `target/nemotron550-all-fleet/` 100,038 input ×8, output cap 2,048 and original deadline, and
   the unstarted workload in `target/nemotron550-mixed-waves/`, **unchanged as historical regression evidence**.
   Since the user removed 550B from the target, re-running 550B is not required as a precondition for the new Release A.
   That failure stays as is and is not replaced by a Qwen pass. The Qwen corpus is regenerated with a separate spec, tokenizer and token IDs,
   applying the same correct answers, input grades, 8 waves and SLO. If the originals are lost, state that reproduction is impossible.
2. Product cold burst: submit 4 short, 2 medium and 2 long requests together at time 0. Sustained is 8 waves × 8 requests,
   with the same length mix and different tasks in every wave. The initial fixed arrivals are 0/180/480/780/1080/1380/1680/1980 s.
   The real send error against the scheduled time is at most 1 s. The overall timeout is the last scheduled send + 1,800 s, and per-request deadlines are also checked.
   If this schedule produces no overlap, re-run both arms entirely under the same new spec, per H2. Do not wait for responses before sending.
3. Use public documents/code and fact/table material from a fixed seed. Short covers code fixes/formatted answers, medium covers linking facts across multiple documents, and
   long covers queries that combine evidence from several locations. Meaningless repeated padding is prohibited. Preserve the exact originals, generator, data version,
   tokenizer/template, token IDs/hash, correct answers/evidence and expected stops in a tracked corpus manifest. The fixture semantic review is
   finished before measuring both arms. The existing `required_substrings` alone does not substitute for correct-answer judgement.
4. Controlled problems get machine judgement of correct answer, format and source, and general answers get a hash-bound semantic review. Every normal service request must
   satisfy H1. A fixture that even the baseline model cannot solve is changed only through H1's common versioning procedure, and the failing originals are also preserved.
5. Recovery drains and re-admits at least 3 wave blocks on the same load. A separate fault arm crosses native delays at head/intermediate/tail,
   return interruption, output backpressure and cancel timing. Overload tests a concurrent burst of at least 16 requests and ±1 around each of the queue/byte/token limits
   separately. Requests rejected from the normal corpus are not counted as normal completions.
6. The comparison distinguishes (a) the current source's spec-off default policy, (b) the same profile with only safety fixes applied, and (c) the selected batch profile.
   If the diagnostic baseline fails, judge only functional recovery, and do not compute an H5 improvement rate against a failed baseline. The optimization comparison between a normal baseline and a candidate
   applies all of H5's 8 paired/4 holdout, quality, relative/absolute SLO and 5%/confidence interval thresholds. A new source/layout is
   a separate arm. Keep the results of every arm and the unselected neighboring candidates.

### A counterexamples and real consumption paths

| Planned ID | Failing counterexample/run environment | Passing oracle and mutations |
| --- | --- | --- |
| A-RED | The current `phase_pacing_actual_loop_expires_decode_wait_without_another_tail_or_input` alone and in a selected bundle. Collect the event order of the real Worker and fake native | The timer wait itself does not change request/flight/slot/input authority. If a normal RELEASE interleaved, pin the linearization point to tell them apart. A mutation in which the timer changes the related authority fails. Acceptance by deleting the existing assert is prohibited |
| A-PLAN | Real OUTER → native PLAN/LOAD. Reduced available, double use of the same host pool, illegal cut, shard/patch/backend/layout mismatch, partial LOAD failure | Do not silently accept a larger allocation than expected or a different placement. Track and reclaim resources already created. Re-passing the 6,678 pure policy cases is a required regression, but it does not replace real-hardware runs. A mutation that removes the consumption path check fails |
| A-BYTES | Results with small counts but large bytes, exact boundary ±1, duplicate/late responses, queue saturation while receipts are retained, unknown send results. The core is also tested with a neutral fake adapter | 0 native/output effects before reservation. All ledgers, reservations, credit and output are identical before and after rejection. A valid duplicate takes effect once. Losing valid duplicate evidence through expiry/eviction is prohibited. A mutation that removes any one of the reserve/commit/response bounds fails |
| A-LIFE | Cancel before/after submit→issue, during partial output, before/after the tail result; a new request during Drain; head/mid/tail disconnects; an old generation returned after slot reuse | Output approved before cancel linearization is preserved, and no new publication after it. Reusable only after per-stage settlement and KV quiescence are confirmed. Unknown states kept separate. Across 3 normal recoveries, 0 remaining obligations to reclaim active owners/flights/unsettled items. Retained receipts for duplicate detection are preserved for their valid lifetime, with a separate byte limit/H7 kept. A mutation that removes the fence/epoch/release checks fails |
| A-BATCH | cap 1/8, D demand below/equal to/above cap + P, multiple sessions, continuous arrival, controller on/off, repeated prepare rejection. Run both the selector and the real Worker | A selected session's eligible P progresses within the at most 8 approved issues allowed for that session. A session that stays eligible gets its turn within as many approved session selections as there are active sessions. Also report the total wait that composes both bounds. Rejection does not consume a turn or policy budget. D's real-time SLO is verified separately by A-SERVICE. Keep equal-width/atomic verify/replay. A mutation that removes the real fix in reserve-P, session rotation or rejection preservation fails |
| A-COST | Change unified KV occupancy/other sequences/CPU expert layout at the same position/rows. Collect full timestamps for native decode→capture→return→Worker forward→client output | Separate real n_kv, phase, mask/graph, copy/network, sample and queue. Do not lump unknown time into compute. Bind to the actually consumed profile/blocked reason. If n_kv or return cost is deliberately omitted, cost conformance fails. Report the instrumentation's own overhead in the same arm |
| A-SERVICE | The full cold, sustained, recovery, overload and fault set for the target/fleet above; real CLI user commands | H0–H7 plus the absolute SLO, resource and reclaim/recovery conditions above. Full normal request texts, EOS/normal stop, quality, settlement and reclaim all pass. Missed evidence, first error and cleanup error are each preserved. Do not drop failed arms or count a forced kill as a graceful PASS |

The round-based fairness above bounds **work that is publishable given resources**. If credit/KV/native stays blocked,
do not subtract that time to make the user SLO a success. Record the real blocked period and end it with a deadline or an explicit
admission rejection. Even in A, which does not support multiple fragments per request, prove progress across independent requests.

Shipping evidence includes the CLI submit/query/cancel/drain/recovery commands, the supported profiles and errors, the machine-readable manifest and
the summary generator. Record the full workspace exit result, the related native/backend, independent mutations and multi-host approval
separately. Preserve raw artifacts by hash/access path, and the corpus/run method/summary under tracked paths. If a required runner
is unimplemented or the hardware is unavailable, that gate is not run/BLOCKED and is not approved with a checkbox.

## 7. Final completion checklist

- [ ] Every required T gate has implementation, runs, mutations and real consumption path evidence.
- [ ] The I gates verified per-layer dependencies and the native/update boundaries, and there is real promotion evidence for the declared backends.
- [ ] The K gates for the persistence/snapshot features in use passed, and the disabled scope is stated.
- [ ] The gates for declared native/model/backend features passed; unused features are marked disabled/unfinished.
- [ ] There is H0~H7 evidence for the final very large model, multiple physical computers, strong waves and full normal response texts.
- [ ] Useful TPS and GPU utilization improvement/limits are reported in the same window; no confusion between RPC/GPU, prefill/decode or structure/semantics.
- [ ] Failures, exclusions and runs not executed were not hidden, and results are bound to the real deployment image and source state.
- [ ] All evidence can be re-inspected and reproduced in a new session or on another machine, and the roadmap's next action has been updated.

Document checkboxes are not a runner. Automated execution and CI wiring of the required gates are stage deliverables, and
without them, state that manual records were used instead and do not claim automated verification is complete.

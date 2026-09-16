# 2026-09-06 — Actual scope of the shared settlement extraction and unresolved counterexamples

Type: code audit and CPU-only deterministic checks. This is not GPU/remote execution evidence.
Initial audit baseline: `a9e1967fc59dffa6c2e458f1b91f916b1df826c1`; the working tree was clean at the first check.
What follows is a chronological record. **The latest state is the last follow-up implementation section**; do not read past RED/clean results as the current state.
The current work order is owned by the [roadmap](../../../../../../../docs/distributed-batching-roadmap.md),
and the test promotion criteria by the [verification protocol](../../../../../../../docs/distributed-batching-verification.md).

## What was run

- `cargo test --workspace --no-fail-fast`: 844 passed / 0 failed / 7 ignored, exit 0.
- Tracked `*.test.mjs` in `test/benchmarks/p4-4node`: 57 passed, exit 0.
- C++ and clippy were not rerun in this audit. Earlier results do not count as this run.
- In a separate temporary copy, the existing worker test fixture was extended to call the real `CapsuleSet::encode/decode`,
  `Worker::tail`, and part of `Worker::handle`. This did not test the full worker loop or the whole transport.
- The probes below existed only in that temporary copy and **are not yet formal regression tests in the repository**.
  The next session must reproduce and commit them as T10~T13/T17 of the verification protocol. Do not treat the temporary paths as long-term dependencies.

## Accepted changes

`v2/node/state.rs::RequestState::settle_fragment` checks the request's outstanding count and prompt bounds
before mutating. A rejection by this function itself preserves the request. The production `Worker::tail` and
`Simulation::advance` really call it, so the source is genuinely shared.
The maximum unselected gap per cohort is now checked through the final interval, and the comment's "9 times" was narrowed to a cohort upper bound.

## R-A — The return event as a whole is not atomic

Anchor: `v2/node/worker/release.rs::Worker::tail` @ a9e1967fc.

Reproduction steps:

1. Align the session/request/load generation and set prompt length 10, issued=4, cursor=0, outstanding=1.
2. Use `AdapterState::open_batch([1])` to create a real open ledger state.
3. Put 6 rows into the terminal prefill capsule of execution=1 and pass it to tail.
4. The request-bound error is returned, but the open batch count drops 1→0. The request is preserved at cursor=0/outstanding=1.

Mixed counterexample: return 4 rows for A (issued4) and 3 rows for B (issued2) in the same CapsuleSet.
With IDs chosen so that A is visited first, the state after the error is A=(cursor4,outstanding0), B=(0,1), open batches=0.
Given the same over-return and a normal completion publisher, `Worker::handle` publishes an error response and
returns `Ok(())` while keeping open batches=0. So the reading that it fails closed through worker termination is also wrong.

Cause: `close_execution` runs before validation, and per-request validate/write steps are interleaved in one loop.
Requirement: pre-validate the entire return event, and apply the request, ledger and output-intent changes atomically (T10/T11).

## R-B — Stale returns and wrong ownership/ranges get settled

Anchor: `v2/node/state.rs::RequestState::settle_fragment`,
`v2/node/worker/release.rs::Worker::tail` @ a9e1967fc.

- Settle F1 `[0,4)`, then create a state where F2 `[4,8)` is the only in-flight fragment (issued8/cursor4/outstanding1).
  Redelivering the F1 capsule under a new event ID yields `Ok(())` and cursor8/outstanding0.
  The F2 execution stays in the open ledger and diverges from the request counters. This holds even without the multi-fragment opt-in.
- In a partial prefill return without an outcome, changing only the sequence ID to 99 is accepted even though the real request is sequence0.
- For an actually issued `[0,4)`, shifting both the invocation and the owner positions to `[5,9)`
  passes capsule format validation, and tail accepts it as cursor4/outstanding0.

Requirement: check against the issued identity/range/membership, and a duplicate must not consume the next flight (T12/T13).
This is a concrete risk of the idempotent ledger that the report declared not yet started; we do not claim it is a regression newly introduced by this extraction.

## R-C — Tests do not protect the shared wiring on the real simulator path

Anchor: `v2/simulator_tests.rs::the_worker_and_this_model_settle_through_one_transition`,
`v2/simulator.rs::Simulation::advance` @ a9e1967fc.

The new test does not run Simulation; it calls the RequestState function directly.
In a verification copy, removing the shared-function call from `advance`, reimplementing the outstanding decrement and cursor increment separately,
and removing the row-bound check still left the simulator tests **11/11 passing**. The mutation was reverted in the copy.
So "1 model failure" cannot be reported as validation of rejection on Simulation's real arrival path.
Requirement: a malformed travelling fragment must go through the real advance and show the same rejection and state preservation,
and a mutation that bypasses the shared bound check must be detected (T17).

## Next work

After preserving the counterexamples above as formal tests, connect the issue records to atomic, idempotent settlement.
Completing the function extraction is not the same as completing the full in-flight or real-hardware results. Instead of repeating small counter-function extractions,
make "settle exactly the work that was issued, exactly once" the next implementation unit.

## Same-day document migration verification

Run on a working tree at the same base HEAD with only documents and document gates modified. No inference code changes, remote deploys or GPU runs.

- Full Rust rerun: `cargo test --workspace --no-fail-fast`, 844 passed / 0 failed / 7 ignored, exit 0.
- Harness rerun: 57 passed / 0 failed, exit 0.
- Documentation self-tests: 12 passed. 3 document-map cases (normal/missing/broken target) were added to the existing 9.
- docs-lint: passed on the 73 tracked documents and on 79 documents with `--all`, including the new ones. The new files were checked separately while not yet in the Git index.
- After the last document edit, `cargo test -p p4-agent --test docs_lint` gave 1 passed, confirming the existing cargo wiring as well.
- private-header gate: 74 source files; by include pattern, header debt 0 / source debt 5.
  This does not mean that type-level and transitive build isolation hold; the actual gaps are recorded in the [layer isolation contract](../../../../../../../docs/layer-isolation-contract.md).
- This document migration does not claim to have run or implemented any C++/GPU/remote or new I/T/K/H tests.

The deterministic T, layer isolation I, conditional store K and final real-hardware H gates required by the new documents are **gates still to be implemented and run**.
Passing the document gates does not count as passing these features. Commit/push is not part of this documentation cleanup request.

## Follow-up counterexample sealing and layer isolation hardening — working tree at the same HEAD

The 844/0/7 above is the result **before** the counterexamples were added. Do not reuse it as a green baseline after the changes below.
The production settlement fix has not been made yet. A test-only simulator injector and formal failing counterexamples were preserved.

| Real consumer path / test | New counterexample and execution scope |
| --- | --- |
| `worker_tests.rs::t10_rejected_tail_preserves_the_open_batch_and_all_request_bookkeeping` | The open batch disappears even though the over-return is rejected |
| `worker_tests.rs::t11_one_bad_request_rejects_the_whole_tail_in_either_capsule_order` | Settles A, then rejects B. Because of the first failure, the later permutation is not yet reached in this run |
| `worker_tests.rs::t11_a_late_outcome_error_cannot_settle_any_other_request` | A's cursor/outstanding and B's generated were already changed before the late outcome error |
| `worker_tests.rs::t12_old_execution_in_a_new_event_never_consumes_the_next_fragment` | The F1 duplicate consumes F2's outstanding |
| `worker_tests.rs::t13_partial_prefill_refuses_a_different_sequence_without_an_outcome` | Accepts a different sequence without an outcome |
| `worker_tests.rs::t13_partial_prefill_refuses_a_different_position_range` | Accepts a different range with the correct row count |
| `worker_tests.rs::t13_partial_prefill_refuses_an_unregistered_execution` | Accepts an unregistered execution |
| `worker_tests.rs::t13_partial_prefill_identity_is_checked_without_waiting_for_an_outcome` | Fails on key tampering. The later generation/phase/request/invocation cases are not yet verified by execution |
| `simulator_tests.rs::a_malformed_arrival_preserves_the_simulation_ledger_and_request` | On the real run→advance, rejects a 6-row arrival for a 4-row issue, but deletes the travelling entry first |
| `simulator_tests.rs::a_wrong_range_arrival_cannot_settle_the_right_number_of_rows` | On the real run→advance, accepts a [1,5) return for a [0,4) issue |

The worker file above is under `v2/node/`, and the simulator file is under `v2/`. The selected worker run gave 3 passed (existing) /
8 failed (new), and the simulator `arrival` selection gave 0 passed / 2 failed. The returns do pass through the real capsule decode, but
no test got through the full worker loop, stage I/O, continued operation after handle, and external output validation.
The fixture's issue registration is still only an ID set; expected-membership registration must be wired up together with the B1 implementation.
These are pre-fix counterexamples, so mutation completion is not claimed either.

### R-D — Boundary between output approval and whether native ran

Anchor: `v2/node/worker/drive.rs::Worker::emit_tail_results` @ a9e1967fc.
It sends TAIL_BATCH to head only after publishing all OUTER tokens. So making only head atomic cannot roll back
output that has already been published. This defect was confirmed from the code order; it was not demonstrated on the full wire path this time.
It is handled by verification protocol T11/T23 and by the isolation contract's L1 commit + L5 effect intent boundary.

native `server/server_physical.cpp::Session::handle_logical_batch` @ a9e1967fc creates the
physical execution ID after the head computation. Rust's issue ledger must distinguish the logical PreparedIssue before the native call from the
AcceptedIssue after PhysicalResult validation. A lost response is Uncertain, and there is no basis for retrying it as unissued.
One logical allocation can span several physical capsules, so request outstanding must not be
decremented for each partial capsule. Check the completed membership/contiguous range and the execution/batch completion separately.

### Current code surface of layer isolation

- `server/CMakeLists.txt`: the runtime's PUBLIC link to `llama-common` and the imported relink `_p4_inc` propagation remain.
- `compat/p4_llama_compat.hpp::LlamaPlan::impl` and the public include root leave opaque internals accessible.
- `runtime/request_options_grammar.hpp::parse_grammar_triggers` exposes a forward-declared common type in its signature.
- The raw integer codes in `v2/capsule.rs::TensorDescriptor` and `Invocation` need codec-meaning and negotiation checks.
- The existing `layers/agent/tests/stays_neutral.rs` is based on neutrality name/manifest canaries and does not replace the full I00 graph and mutation checks.

The actual allowed modification scope, the API register, target permissions, and the change classification for engine/common versus ggml/backend
are recorded solely in the [layer isolation contract](../../../../../../../docs/layer-isolation-contract.md).
This hardening is not a completed implementation of that structure; it is the contract and the test constraints for the next implementation.

### Full rerun after adding the counterexamples

- source: HEAD `a9e1967fc`; the working tree changes documents/document gates and the Rust files related to the 3 tests above. Not committed.
- `cargo test --workspace --no-fail-fast`: **844 passed / 10 failed / 7 ignored**, exit **101**.
  The final exit was confirmed and all 57 summaries were summed. The failures are the 8 new worker tests and the 2 real simulator arrival tests above.
  External fixture features that were not run are not passes in this tally.
- Local raw log: `target/layer-isolation-doc-review-workspace.log` (temporary artifact).
  Long-term reproduction relies on the repository's counterexamples and the command above; this log path alone does not support a claim of B1 completion.
- Documentation self-tests 12 passed; docs-lint including the new files passed on 79 documents. The private-header pattern gate is unchanged at 74 files,
  header debt 0 / source debt 5. This is not a full I00~I09 PASS.
- Harness/C++/GPU/remote were not run in this hardening. After the last document edit, only the document gates are rerun.
- Next: do not delete or skip the tests; close these failures by implementing the real issue record, settlement and effect ownership boundaries.

## Follow-up implementation — issue authority, settlement transaction, fake native seam

Implemented on an **uncommitted working tree** at the same HEAD. The 10 counterexamples from the previous section now pass when run.
This record does not approve the new distributed batching as a whole, B1/B2 completion, native semantic compatibility, or real-hardware performance.
The functional changes this time are confined to `layers/adapters/llamacpp/staged/adapter/`.
The payload semantics of the P4 common protocol/agent and the llama.cpp/CUDA code were not changed.

### Sources and actual implementation boundaries

| Path / symbol | Implementation / limits |
| --- | --- |
| `v2/node/state.rs::RequestState::issue_fragment`, `settle_fragment` | Shared bookkeeping for production issue/arrival and for Simulation. issued/cursor/outstanding change only after the checks finish |
| `v2/node/state.rs::AdapterState::prepare_issue`, `accept_prepared_issue` | Checks the whole candidate against resident authority; Prepared/AwaitingNative/Uncertain; the request advances only after an exact native split is approved. The cancel API has no production cancel consumer yet |
| `v2/node/flight.rs::FlightLedger` | Invocation/owner/membership authority, partial/out-of-order buffering, per-logical-fragment settlement, receipt and execution ID high-water. Verify/Replay preserve the atomic group |
| `v2/node/worker/release.rs::Worker::tail` | Pre-validates all request candidates, outcomes and effect intents; checks the counters and the identity ledger independently; executes after committing ledger/request/intent |
| `v2/node/worker/outcome.rs::apply_fragment` | Normalizes position, generated count, stop, proposal and follow-up KV state for prefill/decode/verify/replay. Does not replace the engine's actual token generation |
| `v2/node/worker/effects.rs::Worker::flush_effects` | Preserves output/forward/settle/release intents. After an external effect fails, the remaining intents and the fence are kept. Not a durable outbox and not crash exactly-once |
| `v2/node/worker/drive.rs::Worker::emit_tail_results` | The tail sends only TAIL_BATCH to head. OUTER output is published once, after head approval |
| `v2/node/worker/settlement.rs::Worker::settled` | Separates physical outstanding from pending KV ack; sampled token, position and proposal authority; pre-checks the whole event |
| `v2/node/worker/release.rs::Worker::released` | Returns the slot only after checking pending release authority, the slot and all admission candidates. It does not yet distinguish a new incarnation of the same key/slot |
| `process/core.rs::ServerControl` and `v2/node/worker/stage_tests.rs` | Box-forwarding implementation of the existing trait. Tests the real handle/drive/stage commands/returns against fake native. LOAD negotiation and the full receive loop are bypassed, so this is not a full worker E2E |

The FlightLedger completion receipt window is bounded at 64MiB/4096 entries, and expired old IDs fail closed.
These numbers are not memory bounds for active flights, edge tensors or pending queues. Nor are they receipts that survive a crash.
The existing open-batch indicator is an observed value regenerated from the ledger and is not used as a standalone completion authority.

### Regression tests on the real path

The path root is `layers/adapters/llamacpp/staged/adapter/src/`. This is evidence for the partial inputs below, not a full PASS of the IDs.

| Test group | This run / proof scope |
| --- | --- |
| `v2/node/worker_tests.rs` | 26 tests. R-A/B/D, wrong key/generation/phase/range/member, normal return after a handle rejection, partial/out-of-order settlement, duplicate/conflict, no output before head, intent contents and join after Closed/partial output, counter mismatch |
| `v2/node/issue_tests.rs` | 10 tests. Plan validation against resident, the shared issue call, no reissue after pre-native cancel/uncertain, exact split, rejection of a split atomic capsule, ID limits |
| `v2/simulator_tests.rs` | 15 tests. Injects malformed rows/ranges into the real run/issue/advance, preserves travelling/request after rejection, prevents partial commit of a wrong whole candidate, existing fairness/clock/error cap |
| `v2/node/worker/outcome.rs` tests | 12 tests. ordinary/verify/replay normal and rejection cases, max_tokens/position/stop/proposal. Not a test of the real native sampler |
| `v2/node/worker/settlement.rs` tests | 9 tests. Whole state preserved even with a late bad ack, proposal sampled-token check, rejection of settlement while physical outstanding remains |
| `v2/node/worker/release_tests.rs` | 3 tests. Atomic rejection of unowned/mixed acks and wrong admission, and return of a properly owned slot |
| `v2/node/worker/stage_tests.rs` | 9 tests. The real drive preserves the fake native split, keeps a lost native response or missing rows as Uncertain, blocks the next issue before the last physical member, and forbids further calls after a bad body from mutating native |

The fake stage only builds Frame responses; it does not reimplement the selector or settlement. However, these tests do not
connect `Worker::run`→broker→N real workers end to end. Stage counts 1/2/4/8, sustained input, cancel,
reconnect, shutdown and native conformance remain separately open. The simulator does not use all of FlightLedger/outcome/effects
either, so it is not reported as "a complete, identical state machine that differs only in the engine result".

### Mutation verification on independent copies

Mutations were not applied to the original checkout and then reverted. Each group ran on the source snapshot current when it was written,
and they are not combined into a claim that all of them ran on one final source snapshot. The final normal suite is run separately.

| Removal/fault mutation | Actual detection |
| --- | --- |
| Simulation bypasses the shared settle, range check removed, early commit on arrival | The real malformed arrival/range tests fail |
| Simulation bypasses the shared issue, outstanding increment removed | The wrong-issue-candidate test and the fragment 1/2/4 independent ledger tests fail |
| worker bypasses the shared issue call or the resident authority check | 2 issue consumer tests fail in each case |
| Early OUTER output at the tail restored | The output-before-head-approval test fails |
| effect popped before success is confirmed | 3 output rejection/partial publish preservation tests fail |
| flight committed before the late outcome check | The whole-event preservation test fails |
| Independent request-vs-flight check bypassed | The T19 real worker counterexample fails |
| KV sampled-token check removed, candidate committed before the follow-up ack check, outstanding allowed | The corresponding KV ack test fails in each case |
| release ownership check removed, partial slot returned early, old admission pop order | The release tests fail in each case |
| max-open gate bypassed, native split omission check removed | The real drive's native call / incomplete issue tests fail |
| atomic physical membership check removed | The test rejecting Verify/Replay torn into 2+2 capsules fails. An independent mutation of internal reordering alone was not run |
| Post-hoc fence removed for native SETTLE short/length/middle-proposal and RELEASE status | The corresponding fake stage test fails in each case |
| handle entry fence removed | The assertion forbidding effects from an additional error event fails. The internal stage guard still blocks the native re-call, so this is not explained by an increase in native calls |

Temporary detailed logs are in `target/tail-mutations-20260906-01/verification.txt`,
`target/release-mutations-20260906-01/verification.txt`,
`target/atomic-membership-mutation-20260906-01/verification.txt`, and `results.json` in the independent Temp copies.
These are local supporting evidence; long-term reproduction relies on the repository tests/mutation locations and commands above.
The early mutation runs in which a shared Cargo target reused the baseline binary were **excluded as invalid**.
For the post-hoc fence mutations that were adopted, the actual compile and the corresponding failure were confirmed in a separate target each.

### Defects not yet closed and constraints on the next development

- The incarnation hole: after the same load/session/key/slot is reused, an old RELEASE/RELEASED reaches the new request.
  Not only the head check but the native KV effects on every hop must be protected. This needs a wire version and operation receipts.
- An identical physical redelivery at a middle stage can call native again ahead of the head receipt.
  The duplicate terminal no-op this time does not prove downstream KV/sampler idempotence.
- The prompt/Event copy in `RequestState::clone`, the cut-set copy in effects, and the rescan of all open batches/owners.
  Do not promote this safety-first candidate implementation to a high-performance baseline. Replace it with immutable input, small progress deltas
  and a membership index while keeping the existing rejection and atomicity tests.
- A loop that drains all continuously arriving input before driving can starve issue opportunities. This is separate from selector fairness.
- The selector mutates cohort resume/decode_runs when building a plan. Making the issue counters atomic this time does not by itself show
  that fairness consumption by rejected plans is gone. Separating the candidate policy delta from the accepted commit remains to be done.
- Active row/byte/KV credit, durable/reconnect convergence, cancel/reload/graceful drain, product LOAD identity,
  common/private/transitive isolation and declared backend conformance are unfinished.

The status of this follow-up work is **B1/B2 IN_PROGRESS**. The roadmap alone owns the next order; do not turn the list above
into a new serial U/P stage table. This record contains no C++/GPU/remote/multi-machine wave or performance results.

### Final normal run and source binding

- `cargo test --workspace --no-fail-fast`: **914 passed / 0 failed / 7 ignored**, 57 summaries, final exit 0.
  The staged adapter lib had 221 passed, an increase of 70 over the initial audit's 844. The total also includes
  older runtime tests, so not all 914 count as event worker evidence. External fixture features were not run.
- Harness `node --test`: 57 passed / 0 failed / 0 skipped, exit 0.
- docs-lint self-tests: 12 passed / 0 failed. private-header pattern gate: 74 files, header 0/source 5.
- Document gate: 73 tracked and 79 with `--all` including new files passed. Mixed EOL was detected after the patch; only the affected document was
  normalized to CRLF within the file and rechecked. File counts do not certify functional or semantic correctness.
- `cargo clippy -p p4-llamacpp-staged-adapter --all-targets`: exit 0, **warnings remain**.
  They include a warning that the new `cancel_prepared_issue` has no production consumer. This is not a `-D warnings` or 0-warning pass.
- C++ CTest, the real llama library, GPU and remote were not run. No commit/push/deploy was done.
- Raw log of the normal run: `target/b1-settlement-workspace.log`; clippy: `target/b1-settlement-clippy.log`.
  The commands are in the verification protocol; these local logs alone are not required as a long-term reproduction condition.

The source is not a clean HEAD. Rust changes were frozen during verification, and `.rs`, `Cargo.toml` and `Cargo.lock` from Git's tracked plus
non-ignored untracked files were sorted by path without duplicates (353 files).
The SHA256 over the concatenation, in order, of each file's `path + LF + raw byte length + LF + SHA256(raw bytes) + LF` is
`67e7fa07aa27699dc65492934bcbdcfa07ec9f2b3bb64b668f812b517905b7b4`.
This identifies the Rust source scope of this run; it does not stand in for a document, native or full deployment image identifier.
If later sources differ from this value, do not reuse this result as a pass for the new sources.

## Follow-up execution ownership and policy candidate slice — 2026-09-06

The status is **B1/B2/B5 IN_PROGRESS**. HEAD is still `a9e1967fc`; what follows is follow-up implementation on an uncommitted
working tree. Do not use the 914/Rust-only results from the previous section as this slice's results. No commit, push or remote deploy was done.
The contract definition is owned by the execution ownership section of the [batching contract](../../../../../../../docs/adapter-batching-layers.md),
layer permissions by the [isolation contract](../../../../../../../docs/layer-isolation-contract.md), and the next order by the roadmap.

### Pre-fix failures and real consumer paths

- When the first RELEASED arrived for a second request on the same load/session/request/slot, the new pending release was erased,
  and an old RELEASE at the middle/tail deleted the new fake native KV. RED was confirmed on 3 real paths: head prefill→drive→tail→release and
  middle/tail PHYSICAL→release. Now the new incarnation is preserved and a normal new release proceeds.
- A body/kind conflict on the same operation or an earlier operation is rejected before native; an exact SETTLE/RELEASE replays the receipt.
  The stage tests pass a valid P4ID prefix and a real PHYSICAL warmup before injecting the original short/count/role faults.
  An early rejection by the new guard is not counted as detection of the existing native response fault.
- Aggregate receipt budget counterexample: each item passed its own check, but the total for one command was impossible. Before the fix, RELEASE
  deleted the first slot and then rejected; SETTLE failed to commit after two native calls/truncation. Now both directions reject before the first native
  call and preserve ownership/flight/KV/output intent. The same operation given alone runs normally.
- A head test was also added showing that an old SETTLED's incarnation/operation does not consume the new pending KV barrier.
- The policy candidate was split into `Scheduler::prepare_plan_with_physical_capacity`/`validate_prepared`/`commit_plan`.
  On the real drive, forcing the issue ID to 0 and rejecting 64 times leaves the selected revision/cohort/member, request, owner, flight and
  native call count unchanged. Once only the ID is repaired, the originally scheduled decode slot runs, and only then does it rotate to the next member.
- `Worker::bind_loaded_identity`, used by product LOAD, was verified with a real lifecycle/request + fake ServerControl.
  Missing/unknown capability, a different echo, a lost response and reuse of an earlier generation are handled by not publishing the slot / shutting down.
  **This test does not go through the LOAD JSON parser or real subprocess startup.** Native Session tests exist separately as well.

### What changed at the layer boundary

No model rules were added to the P4 common protocol/agent. Execution ownership/ledger and wire live in the concrete adapter,
and the native guard/codec in the stage shell. The guard and the shared `utf8_text.hpp` compile without llama/ggml types.
The LB/PB and control incarnation conventions are a deliberate adapter wire change, not direct exposure of the upstream API.
This does not resolve the remaining state ABI, backend layout, ggml ordinal, common public signature, or transitive include/link issues.

The canonical identity negative tests reject a wrong session prefix, an empty request, an extra NUL and corrupted UTF-8, and
accept valid Korean text and emoji. Once the encoder began rejecting invalid Rust keys up front, the existing worker
counterexamples were changed into explicit tampering of valid bytes so that they still reach the real decoder/consumer. The unknown request counterexample
also changes that other request's canonical key, so it passes the codec and is rejected by the ledger authority.

### Mutations and accounting limits

All were actually recompiled in independent copies, not in the user's checkout. These are per-source-snapshot tests, so
they are not combined into a claim that every mutation ran on the single final full source below.

| Mutation | Detection / preservation evidence |
| --- | --- |
| Move drive's policy commit ahead of issue approval | The real reject→replan test fails; after the exact failure cause/native 0, the cursor differs. Restored stage 13/13 |
| Remove native control owner/body/watermark/canonical checks | All 4 in the final native copy fail the corresponding assertion. Baseline and final CPU pass separately |
| Remove incarnation/watermark/count/receipt bounds from Rust ownership | The ownership/reuse/limit tests fail in each case. Candidate payloads are shared via Arc |
| Remove the control aggregate guard, recharge Replay, remove the old receipt deduction, pre-deduct a later shrink, remove the duplicate slot or authority check | All 6 fail (2/1/1/1/1/2 tests each). Restored ownership 16/16 |
| Remove the LOAD exact echo check or allow a missing identity revision | 1 consumer test fails in each case; restored control tests 6/6 |

Local RED/mutation supporting evidence is in `target/incarnation-red-20260906-01/verification.txt`,
`target/aggregate-control-red-20260906-01/verification.txt`, `target/drive-policy-mutation-20260906-01/verification.txt`,
`target/bind-load-mutation-20260906-01/verification.txt`,
`target/native-identity-mutations-final/build/Testing/Temporary/LastTest.log`.
The repository regression tests and the mutation definitions above are the reproduction reference; a new session does not require the ignored target directories to exist.

This is in-memory control idempotence plus a serial pre-budget check. If native fails mid-execution, it does not roll back multiple changes
atomically; an unknown result is fenced. Duplicate PHYSICAL execution computation, prefix ordering, transfer credit, sustained
input servicing, cancel/drain and crash-durable receipts are unfinished. snapshot/restore and bare legacy KV mutation are
not allowed as bypass execution in bound mode until they are integrated with this execution ownership.

### Final run results — numbers kept apart from what was not run

- `cargo test --workspace --no-fail-fast`: **956 passed / 0 failed / 7 ignored**, 57 summaries, final exit 0.
  The staged adapter lib had **263 passed**. The total also includes older runtime tests. External fixture features were not run.
- Harness `node --test`: **57 passed / 0 failed / 0 skipped**, exit 0.
- docs-lint self-tests **12/12**, official native builder wiring **4/4**. The wiring test really executes the production JS source/imported/
  no-llama target selection but substitutes the external build commands. It is not evidence of a successful real imported relink.
- **Full CPU source build succeeded** with the final native sources. The CTest surface tally is 13 executables with exit 0.
  Precisely, that is **10 with non-model bodies executed + 1 partial compile_test run + 2 with the whole body SKIPPED**.
  `request_options_test` exited at the first environment variable check, so none of its sampling/grammar assertions ran either,
  and `mtp_ownership_test` also exited at the first environment variable check. The two compile_test functions for real restore and batch rollback
  were also skipped. So there are 4 SKIP output lines, and **real-model native conformance is not approved**.
  The new authority/codec/UTF-8 tests and the Session BindLoad/legacy rejection tests are not in the skipped branches.
- The private-header pattern gate and docs-lint passed, but that does not certify semantic correctness or full isolation.
- clippy exit 0, warnings remain. There is also a type-complexity warning from this LOAD test, so we do not say "0 new warnings".
  The full `cargo fmt --all -- --check` failed on formatting in existing, unchanged entrypoint/adapter files and others.
  Those files were not reformatted arbitrarily; the changed staged Rust files were normalized separately.
- GPU, real model load/native KV effects, multi-computer waves and performance non-regression were not run.

Logs: `target/b1-incarnation-final-workspace.log`, `target/b1-incarnation-staged.log`,
`target/b1-incarnation-harness.log`, `target/b1-incarnation-clippy.log`,
`target/native-identity-cpu/Testing/Temporary/LastTest.log`.
The existing CPU CTest setup that counts a model-less early return as Passed must separate unit from model-required tests in B5/T03.
Do not fix the gate by keeping mandatory model tests as success counts.

### Frozen source identification

Same path-sorted `path + LF + raw byte length + LF + SHA256 + LF` method as the previous section.
Aggregate for the Rust scope of 357 files (`.rs`, Cargo.toml, Cargo.lock):
`1782839851ebae20e1e69d0d1b1a5eccf175b57202850c67fb90a419b8b5b41a`.
The native scope is the 79 `.cpp/.hpp/.h/.inc` and CMakeLists.txt files under staged/server, with aggregate:
`a4ea6508731bf6c7090d2df1c51b84530ad5d95e3b666b124664dde0f0f9cdf0`.
This identifies those source scopes; it is not a digest of the upstream prepared tree, the toolchain or the full deployment image.
The CPU build ran in a new source-build tree at `target/native-identity-cpu` and was not replaced by relinking the existing imported DLL.
If the sources change in later development, do not reuse this result; record a new digest and new run results.

## PHYSICAL receive receipt and opaque plan lifetime — 2026-09-07

A follow-up **uncommitted working tree** on HEAD `a9e1967fc`. These are run results for the current source and do not claim a new commit/push/deploy.
B1/B2/B5 in the roadmap are still IN_PROGRESS. The PHYSICAL identity, bound and expiry contract is owned solely by the
[batching contract](../../../../../../../docs/adapter-batching-layers.md).

### Real consumer path and pre-fix failures

- `Worker::physical` was split into its own module and wired to `PhysicalReceiveLedger` prepare/begin/complete.
  Only fresh inputs run as a native Frame, and the result is assembled with the cached responses in the original event order.
  IDs become Running only after full pre-validation, and an unknown native response turns every fresh input into Uncertain/fence.
- Before the fix, of 9 real worker tests, the 3 for normal progress and native post-hoc failure were PASS, and the 6 for duplicate/conflict/mixed/lower-ID
  redelivery were RED. The fake changes the KV record and the tail sampler nonce on every native call, so it is not an echo-only stand-in.
  The original RED is preserved in `target/physical-replay-red-20260907-01/verification.txt` and its raw output.
- After integration, the actual SESSION→handle→PHYSICAL codec→native Frame→receipt/owner commit→mailbox tests are
  **14/14 PASS**. The fixture installs the post-load state explicitly, so we do not claim that the LOAD JSON, subprocess startup, Worker::run,
  a real network, llama KV or GPU were exercised.
- An independent audit additionally found a defect in the assumption of load-global execution numbers. Each native head Session issues
  its own numbers, yet SESSION allows different first values. Authority is now distinguished by the full Endpoint of the configured first.
  The same number from a different head is two normal executions; a different session/body from the same head is a conflict.
- After a real P4ID RELEASE clears the fake live KV, running the same slot/incarnation 2 again and redelivering the old receipt
  still preserves the new owner/KV. A result that exceeds the cache expiry or limit still allows the first normal execution,
  and a later redelivery is not turned into new native work. An exact Replay has no new compute span.

### Mutations and their limits

Protections were removed and actually recompiled in a separate copy, not in the user's checkout.
The mutation definitions below and the repository test names are the reproduction basis; permanent retention of the ignored target paths is not required.

| Mutation | Actual detection / limits |
| --- | --- |
| Merge the full issuer namespace into the first namespace | `t24_distinct_head_full_endpoints...` fails. The normal test varies each of the agent/node/generation axes, but this mutation fails at the first agent difference |
| Remove the canonical body comparison for completed IDs | `t24_conflict_anywhere...` fails. Detects that [fresh, then conflict] calls native and changes state |
| Re-accept an old owner even for replay-only | `t24_cached_old_return...` fails. Even though the existing owner guard blocks takeover, this detects a regression that wrongly rejects an exact replay |
| Report a cached result as a new compute span | The exact middle replay test fails. A mutation that only removes the early return passed equally because of the empty guard inside emit, and that result is also preserved |

The ledger unit tests are **18/18 PASS**. The 11 mutations — canonical input comparison, naive max-seen rejection, prospective floor, returned owner,
oversized tombstone, input byte accounting, invocation/owner membership, issuer merging, and the Seen/issuer/cache aggregate
bounds — also failed the corresponding tests. The unit ledger tests and the actual worker tests above differ in scope.

The first round of unit mutations had only tool output, so a **new preserved round** was rerun on the same final source.
`target/physical-receive-mutations-20260907-01/` holds the baseline/11 mutations/restore raw logs with their commands, exits and
source/binary hashes, and `mutation-definitions.json` records the exact before/after code and test names.
All 13 runs were confirmed to actually be Compiling. Baseline and restore each gave 18 GREEN; each mutation was a test failure with cargo exit 101,
not a compile failure substituted for one. The original and the independent copy had the same sources, but the two normal binary
hashes differed, so this is not used as evidence of a bit-reproducible build. The 28 temporary evidence files were copied into the workspace target
and each file's SHA256 was confirmed to match. The original source was not modified in this process.

Actual worker mutation raw data, commands, exits and per-mutation source/binary SHA256:
`target/physical-replay-mutations-20260907-01/verification.txt` and the output files in the same directory.
The core receipt source SHA256 of the final original/copy is
`bfb777f715d300eb53a75b782658ec5cbc11b138cd1b5d0c2fce2f76b5f6b182`,
the worker consumer is `8a7e370ff817b525577339cef9f58e1cfee43c66ddd9e3b16cfd87d79046e374`,
and the actual worker test is `ff3d39707909e15b3286cd2dc90ebed9416519682195c815044f1626d5c92797`.

### opaque plan: keeping the ownership-transfer counterexample on a model-less path too

The options E2E read the sampling options after moving the plan into the runtime. In a model-less run it
SKIPped before that point, so it never reached this defect. Now a **test-only** shared preparation function,
`consume_request_options_plan`, builds the two sampling snapshots before the move. The real options E2E uses the same function to call the real
`runtime.load`, and the new `plan_lifetime_test`, without a model, consumes a real opaque plan inside a callback
and checks the snapshot contents, preservation on failure, and lifetime after the consumer is destroyed. The production parser was not changed, and
the 27 sampling/grammar assertions were not deleted.

In a separate native copy, reverting to the old order caused a SegFault, and an early return failed because the required CTest completion string
was missing. Injecting a Release `assert(false)` also produced an assertion failure. That is evidence that assertions are active;
it does not claim that a mutation removing the assert was detected separately.
Evidence: `target/plan-lifetime-mutations/build/Testing/Temporary/LastTest.log` and
`target/plan-lifetime-mutations/full-native-test.log`.

### Results of this run and what was not run

- `cargo test --workspace --no-fail-fast`: **988 passed / 0 failed / 7 ignored**, 57 summaries, final exit 0.
  The staged adapter lib had **295 passed**. External fixture features were not run, and the total also includes older runtime tests.
  Log: `target/b1-physical-workspace.log`. Intermediate tallies were not used as the overall result.
- Harness `node --test test/benchmarks/p4-4node/*.test.mjs`: **57 passed / 0 failed / 0 skipped**.
- Official native builder wiring **5/5**. It checks target selection by substituting the build process, so it does not demonstrate an imported relink.
- docs-lint self-tests **12/12**, 73 tracked files / 79 total files clean, private-header pattern gate passed on 80 files.
  `git diff --check` reported no errors. The pre-existing common source debt remains.
- After the final native CPU source build succeeded, CTest was rerun. **14 executables with exit 0 = 11 with non-model bodies executed
  + 1 partial compile_test run + 2 with the whole body SKIPPED**. SKIPs remain on four paths — real restore, rollback, options E2E and MTP —
  and actual model conformance is not approved. The new lifetime test is not a SKIP.
- `cargo clippy -p p4-llamacpp-staged-adapter --all-targets`: exit 0, warnings remain. There is an inspect_err suggestion for the new physical consumer
  and a test type-complexity warning, so this is not reported as 0 warnings or no new warnings.
  Log: `target/b1-physical-clippy.log`. A full-workspace fmt pass is not claimed.
- Passing the document gate's string/index checks is not evidence of functionality, atomicity or model performance. GPU/real model load,
  VRAM-only waves, RAM offloading, multi-computer runs and performance non-regression were **not run**.

The frozen sources were aggregated with the same path sorting and raw byte length/SHA256 method as the earlier records.
Rust scope, 360 files, aggregate:
`0b79236846afb8c404746efd2882ebc3845352bce1137a111738b50172274a16`.
Native scope, 81 files, aggregate:
`816c90838ca53b648fa67a3321dc8633ddbe445cf18a25c035cfce5443a3f08b`.
These do not stand in for document, model, toolchain or deployment image digests. Nor are they the result of editing sources while an A/B run was in progress.

### H0 read-only check — the execution environment and model approval are separate

The user-specified resources and expansion order are reflected in roadmap §1. Only verified facts are recorded here.

- M42-SERVER2, queried over the existing trusted SSH key path, reported two RTX3090 cards at 24,576MiB and host RAM of
  274,561,966,080 bytes. At one point in time the two GPUs used 350MiB/0% each; that is not a guarantee of reservable capacity or
  future exclusive use. The fact that both GPUs are in one physical host is not turned into multiple hosts.
- `S:\models` under the current local account held 156 GGUF files. Grouped by file name and size, there are 40 weight candidates,
  22 mmproj and 1 embedding. All pieces of the 18 split candidates were present by file name, but the GGUF headers/contents/full
  digests were not read. This does not mean 156 independent models were verified or that all 40 are supported.
- S: was not visible in the same non-interactive SSH session. The existing `remote-agent.mjs` knows about this difference and runs the agent as an interactive
  scheduled task. Access from the actual agent account is still unverified. No path changes, model copies,
  mapping creation, credential changes or remote task start/stop were done. Passwords/tokens from the access documents were not copied into the repository.
- The existing `layer-window-memory-report.mjs` fails to run in the current standalone repo because of the old apps root and missing modules.
  The single-ingress/forced-GPU settings in the existing `spec.mjs` also do not amount to a completed inventory, RAM offloading or multi-host runner.

### Remaining safety and cost

The ledger that blocks replay of retained IDs is not the same as a sequence KV frontier. Still open: stale position/gap/phase under a new ID,
freshness after a new Worker/native Session or reconnect, sustained input/output saturation, cancel and drain in the real Worker::run,
and binding of edge credit and retry periods. Arbitrary expiry of cached results does not guarantee lossless recovery.
begin/complete copies the whole bounded Seen index and Arc handles, and temporary encoding/assembly payloads also remain.
This cache bound does not certify total RSS or hot-path cost. The next order is owned by the latest roadmap record.

## 2026-09-07 follow-up — stage KV frontier, independent mutations after the source freeze

The base HEAD is still `a9e1967fc59dffa6c2e458f1b91f916b1df826c1`, and an **uncommitted working tree** was
verified. The original was not checked out or reset, and there was no commit/push, remote process change, deploy or model load.
The user scope of VRAM-only followed by RAM offloading is owned by roadmap §1 and verification protocol H0.

### Actual failures and implementation scope

Old position/gap/Prefill regressions carrying new IDs were sent to middle/tail through the real `Worker::handle`.
The fake native writes KV and increments the tail sampler nonce on every call. Before the fix, the 1 normal continuous-progress test
PASSED and the 8 that should be pre-rejected FAILED. Sending a mixed A-normal/B-fault pair in both orders still ran A first.
Raw output, source and binary hashes at the time: `target/stage-frontier-red-20260907-01/verification.txt`.

`v2/node/frontier.rs::StageFrontiers` was wired into the head's actual issue, middle/tail PHYSICAL, and SETTLE/RELEASE.
An exact receipt replay does not move the frontier. It checks position, phase, generated count, options/reply,
Verify window, authorized Replay and the tail's full proposal, and the candidate holds only the touched slots.
The body of the existing `worker/outcome.rs::validate_outcome` was moved into the single pure frontier module.
The original validation semantics were not deleted, and the sampler is not called from inside the ledger.

The normal paths include partial→final Prefill→Decode, full Verify acceptance without SETTLE, direct partial SETTLE,
and an exact Replay of the same round after checkpoint restore. The negative tests were not made to pass by simply blocking every Verify.
The wrong premise of the existing stage control fixture, which did only Prefill and then an arbitrary SETTLE, was also changed to a real PHYSICAL Verify
warmup. The original purpose of the response corruption/receipt bound/late operation rejection tests and their native call and fence assertions were kept.

While writing the new tests, 3 cases where the fake replaced a multi-row tensor with a single 4-byte value and raised `InvalidTensor` were fixture errors.
The fake was fixed to keep the tensor length and record the nonce in each word. The new pure tests also found an implementation defect that approved
result chunks of the same sequence with valid membership in reverse order. This was fixed by checking the per-slot return order.

### Independent mutations — actual recompile and restore

| Removed check | Observed failure |
| --- | --- |
| Pure old/gap position check | Unauthorized rows are appended to native KV at middle |
| Frontier wiring in the real `Worker::physical` | KV and the sampler run again at tail. The pure function merely existing is not a defense |
| Exact Replay token binding | The wrong token 999 enters native KV |
| Per-slot result chunk order | The results `[2..4),[0..2)` are approved as Ready |
| delta slot revision | A stale RELEASE candidate is approved |
| tail settlement retain check | Approves 4 instead of the retain 3 that tail committed |

For the first 3, `target/stage-frontier-mutations-20260907-01/verification.md`, and for the last 3,
`target/stage-frontier-pure-mutations-20260907-01/verification.txt` hold the raw output, commands, source/binary
SHA256 and exact mutation definitions. All compiled successfully and then failed on test assertions (cargo exit 101).
After restore, the real consumer mutations passed again at 28/28 and the pure mutations at 10/10. Where a mutation stopped at the first failure of a for-role
loop, we do not extend that to claim the later roles also ran under that mutation.
The restore hashes of the 141 consumer copy files matched, and the 11 pure evidence files were copied from temp to the in-repository location above
with every per-file hash matching. This target raw data is not a Git-tracked deliverable.

### Frozen results and exact scope

- `cargo test --workspace --no-fail-fast`: **1012 passed / 0 failed / 7 ignored**, 57 summaries, final exit 0.
  The staged adapter lib had **319 passed**. Log: `target/stage-frontier-workspace.log`.
  External fixture features being excluded and the older runtime tests are also kept apart. Not all of this tally is event full-loop tests.
- Pure frontier **10/10**, actual PHYSICAL consumer **28/28**, existing stage consumer **15/15**.
- Harness **57/57** + native builder wiring **5/5**, total **62/62**, skipped 0.
  Log: `target/stage-frontier-js.log`. This is not a count of executed native C++ bodies.
- clippy exit 0, **warnings remain**. There are also suggestions for the new `map_err`, test layout and so on.
  Log: `target/stage-frontier-clippy.log`. Not reported as warnings 0 or no new warnings.
- C++/CUDA, real models, heavy real-hardware waves, RAM offloading, multi-computer runs and TPS non-regression were **not run in this round**.
  The earlier native model SKIPs are not filled in with this Rust GREEN.

Frozen Rust, 361 files: from Git tracked+untracked (exclude-standard), `.rs` and Cargo.toml/Cargo.lock,
sorted by ordinal path; the SHA256 over the concatenated `path LF byte-length LF sha256 LF` is
`85d453ebbfc381225a66555861386ebfcbdcc00afc2eb2d454ea1caff72950a0`.
Key file SHA256:

| File (under adapter src/v2/node/) | SHA256 |
| --- | --- |
| frontier.rs | `9a12ea01693a3461e4ecd6383b0ac25a19886983207d8225a7be6bdf8a6ed196` |
| worker/physical.rs | `970ad0739377e1d6ebb85f1a7d6815ae81f741dfa824d94030912e709842ec9f` |
| worker/drive.rs | `4d2769be9b39bc9ab9dee1f14534cd171ee381ed1163d10fa15936fdff5d6572` |
| worker/settlement.rs | `1e8689f497cdf68d8e3bd4c3421b5e88891ce2cfb75330ef4a2848a935ffabb5` |
| worker/release.rs | `c207e15e6025fa4d5c08f339bbade6a0c22866436edadec2c74765ef20b10578` |
| worker/physical_replay_tests.rs | `aadd2253a7839f71ffc00d2a12c23374542390006661d844cdfd32dcee98ee7f` |

### Additional finding: two P1 paths outside GREEN

An independent audit reproduced that with `physical_capacity=2`, if native returns a well-formed proposal `[23,29,31]`,
the token budget check passes and **PHYSICAL publishes TAIL_BATCH and SETTLE publishes SETTLED**.
Both paths give `handle=Ok`, fence=false. A normal opcode and the token budget alone do not guarantee the atomic width.

In `target/proposal-cap-red-20260907-01`, the baseline 15/15 and the new rejection counterexamples **2/2 FAIL** are preserved separately.
A 138-file comparison shows the only change is one independent test file; the 137 production files are identical to the frozen original.
Evidence: `red-output.txt`, `verification.json`. These counterexamples are not included in the original suite of 1012,
and the original GREEN is not reported as full product correctness. The first next action follows the latest roadmap record.

This frontier is a **check in Rust before entering native**. Still open: direct-call-site defense in C++ itself, the full Worker::run
schedule, sustained queue saturation and shutdown, restart freshness, credit/retry, model conformance, and real-hardware proof.

## 2026-09-07 follow-up — continuation width, actual loop, duplex Full

HEAD is the same `a9e1967fc`; follow-up working tree changes were verified. No mutation/restore of the original checkout, commit/push,
remote deploy or GPU model load was done. These are not C++/real model, RAM offloading or multi-host results.

### T24 continuation width: failure after native and pre-rejection at head

The earlier independent RED was moved into a real stage consumer test. With `physical_capacity=2`, native returns `[23,29,31]`, which satisfies the normal codec
and the remaining token budget. Before the fix, PHYSICAL/SETTLE and a Fresh batch with a valid leading part plus a bad
trailing result all gave `handle=Ok`, fence=false, and published a successful TAIL/SETTLED.
The original RED was **17 passed / 3 failed**; the same tests after the fix gave **20 passed**.
`target/proposal-cap-source-red-20260907/red-output.log` with a source copy, and the post-fix
`target/proposal-cap-source-green-20260907.log`, are preserved.

- The shared `frontier::validate_continuation_width` was wired into the PHYSICAL response, the SETTLE response and head tail approval.
  It adds a physical width check without replacing the token budget/phase checks. It does not truncate the proposal.
- A bad response after native has already run is an effects fence. The whole PHYSICAL Fresh batch is Uncertain,
  with no success receipt/cache and no owner/frontier commit. SETTLE does not create a success receipt/SETTLED either.
  Sending an identical redelivery and a separate valid new request results in 0 additional native calls. No native KV rollback is claimed.
- Width 1 and exactly cap=2 keep progressing through Decode/Verify on the real head return→next drive→tail.
- A separate head regression returns a valid A and a width-exceeding B together and checks that all requests, flights and output intents are unchanged.
  The same issued identities, with B brought within the cap, each output once. The cap check on the pre-existing SETTLED was kept.

### Part of T20/T21/T23: 4 real Worker::run tests

`worker/loop_tests.rs` goes through a real OS worker thread, bounded input/completion mailboxes, and the Event/Frame/
LogicalBatch/CapsuleSet codecs. The native fake manages independent KV token/position/incarnation arrays
and does not call the production scheduler/request/flight/frontier/ownership transitions.
The fixture builds the post-LOAD state explicitly, and routing is a separate bounded pump.

1. Exact tokens, positions and termination for a 2-stage chunked prompt with max_tokens 5, one release on every stage, and head ack.
2. At 2/4/8 stages, a second wave of 2 requests is added while a first wave of 6 requests is in progress.
   The first output of a new short request arrives before the last output of an older long request, and every request's exact
   native KV history, tokens, positions, stop and release on every stage are checked.
3. At completion capacity 1, a real `completion_queue_full:waiting` and 1 computation occur first.
   When draining resumes, the run completes without losing results. This is not merely a direct test of the Full data structure.
4. At max-open=1, returning only one of the two physical pieces of a logical batch keeps the next issue from opening.
   max-open=2 is the positive control that issues twice before TAIL, and it also handles TAILs of different requests in reverse order.

**Scope limits:** N=1, starvation under sustained inflow, speculative SETTLE/Replay, and cancel/graceful drain are not included.
The native subprocess/model/GPU and the real EventNode/broker/network are not in this fixture either. The join escape of a forced teardown
is not counted as a normal drain. The fixture queue bounds are not extended to product pending/RSS/credit bounds.
The correctness of fake token strings is not reported as the response quality of a real language model.

### Part of T22/T23: bidirectional progress of the neutral EventNode

The existing `dispatch_or_wait` could not read its own input while its output was Full. Using the real broker and two EventNodes with capacity 1,
each was made to hold a completion destined for the other, and then normal input was filled in.
Before the fix, both runs gave **a=0, b=0** with a 1-second timeout. The only ready path on the first poll was made the completion,
so the test does not depend on the luck of the select branch. All events pass normal broker validation.
The raw output and the old source/exe hashes are in `target/event-node-duplex-red-20260907-01/verification.md`.

The neutral pump keeps one held input and one held output. Even when one side is Full it keeps the other side's processing opportunity,
and it does not take more from the queue of a direction it already holds. It handles only opaque Events and adds no adapter/model knowledge.
An additional bound test, with the two held items plus each next capacity-1 queue full, checks completion take=1, a third bidirectional
offer=Full and full byte preservation, and then checks that once space recovers the first two of each arrive as exact Events in order.
A Closed destination is an error, and the broker does not record the delivery failure as a Duplicate.

EventNode **7/7** and core neutrality **3/3** passed. The 1ms timer still remains; it is not a capacity waker.
Deadlock resolution, control reservation, bounded RSS and shutdown in a general cyclic network where every adapter/queue is Full are not guaranteed by this fix.

### 9 independent real consumer mutations

| Mutation | Real consumer tests that failed | Restore |
| --- | --- | --- |
| continuation helper disabled | 3 stage | stage 20/20 |
| cap call removed from PHYSICAL | 2 stage | stage 20/20 |
| cap call removed from SETTLE | 1 stage | stage 20/20 |
| cap call removed from head | 1 head | head 1/1 |
| max-open gate removed | 1 actual run-loop: native issue 2 ≠ 1 | loop 4/4 |
| worker Full discards results instead of keeping them | 1 actual run-loop: waiting/normal recovery not achieved | loop 4/4 |
| exclusive await on outbound Full restored | 1 actual broker ring: a=0,b=0 | EventNode 7/7 |
| held input guard removed | the second input is taken and the third is wrongly accepted | EventNode 7/7 |
| held output guard removed | completion take 2 ≠ 1 | EventNode 7/7 |

For each arm, an assertion failure (cargo exit 101) after an actual recompile was confirmed, and the final restored copy passed.
Raw data, exact mutations, source/binary hashes and restore comparisons are preserved in:

- `target/proposal-cap-mutations-20260907/verification.json`: 4 cap mutations, per-arm comparison of 139 source files.
- `target/worker-loop-mutations-20260907-01/verification.txt`: 2 loop mutations, hashes of the 4 key files for the final 03~06 runs.
- `target/event-node-duplex-mutations-20260907-01/verification.md`: 3 duplex mutations, 150-file snapshot comparison.

Failures excluded from the evidence are recorded too. On the first copy for the head test, preserved mtimes made Cargo use an old executable and
run **0 tests**; that run is invalid. The copy's mtime was updated, an actual compile and a 1-test run were confirmed, and then the mutation was applied.
The first loop gate mutation aborted abnormally because the fixture Drop panicked again during assertion unwind.
After fixing Drop so it does not mask the original failure, the same mutation reproduced a normal FAILED, and the old abort was not counted in the final detection count.
The target raw data is preserved locally and does not mean it is Git-tracked or deployed.

### Final frozen tally

- `cargo test --workspace --no-fail-fast`: **1026 passed / 0 failed / 7 ignored**, **57 summaries**, final exit 0.
  staged adapter lib **330**, agent core lib **149**. Log `target/proposal-duplex-loop-workspace.log`.
  This is the full tally, excluding external fixture features and including older runtime tests; not all of it is event loop tests.
- JS harness 57 + native builder wiring 5 = **62/62**, skipped 0. `target/proposal-duplex-loop-js.log`.
- clippy (staged adapter + agent core, all-targets) exit 0, warnings remain. We do not claim there are no new warnings.
  `target/proposal-duplex-loop-clippy.log`.
- After the document update: docs-lint tracked **73 clean** / `--all` **79 clean**, self-tests **12/12**,
  `cargo test -p p4-agent --test docs_lint` **1/1** passed again, default `git diff --check` exit 0.
- C++/CUDA/native models, real-hardware waves, RAM offloading, multi-computer runs and performance non-regression were **not run in this round**.

The 362 Rust files were frozen with the same ordinal `path LF byte-length LF sha256 LF` rule as before.
Aggregate SHA256: `c532a51016ba39ea97251602ecefbdbb2e9397696a75db99ce8161cdb7949f84`.
List: `target/proposal-duplex-loop-rust-source.log`. Later document-only updates are not included in this Rust seal.
Key new loop test SHA256 `e297fe44a2ebce6ced2996b30440b95f1e89fef514614859c69e5961c4a196d2`,
EventNode SHA256 `d3711058d3a4b2ca63c2c001fac097f4b700e8b29a6ad5937842cdefce24cf3b`,
EventNode tests SHA256 `9dbce3f6fd70fcc4fd752f21b1864bfb6929006fad0cfb7b8016da8fa10d284c`.

Status and next action are owned by the latest roadmap section. This GREEN is not fixed as completion of sustained input, full credit, normal shutdown,
native direct-call defense, or VRAM-only/RAM offloading real-hardware runs.

### Outside the frozen GREEN: a deterministic counterexample for sustained input drain

In an independent copy, only one observation probe was added to the existing 4 loop tests. With a runnable token request placed first,
the Tokenize fake pushes the next valid PREFILL into the worker input one at a time, causally keeping the queue nonempty.
For input chains of 0/16/256, the number of Tokenize calls executed before the first Logical was **0/16/256**.
With an infinite chain, the `try_recv` drain never ends and drive is never reached. Once the finite chain was cut,
the run completed through normal tokens, KV on every stage, release and head ack. This is not a guess that admission is the cause, nor a TPS experiment;
it is a counterexample in the processing order of the real loop. No fixed quantum value was arbitrarily added to the PASS condition.

`target/worker-ingress-drain-probe-20260907-01/verification.txt` preserves the raw output/metadata/fixture-only diff,
the probe source and the unchanged worker source. The independent run was 5 tests (the existing 4 + 1 observation), but they are **not added
to the 1026**, and the observation probe's successful exit is not counted as a PASS of the correctness gate. The original production loop was
not changed this time; the processing-opportunity contract to uphold next is owned by verification protocol T20, and the implementation order by the roadmap.

## 2026-09-07 follow-up — finite actor opportunities and a shutdown that preserves failures

After the previous section, the working tree at the same HEAD `a9e1967fc` was modified. No commit/push, deploy or model load was done.
There were no mutations of the original source; this is CPU-only Rust verification. It is not a result on physical GPUs, real language model responses or RAM offloading.

### Bidirectional progress of sustained input and issuing

The unbounded input drain in `Worker::run` was limited to at most 32 per turn, and the existing issue loop was split into one real
`drive_one_batch` call. On success, the next turn proceeds again without new input; when the gate is closed or there is no work and an external release
is needed, it waits for input. The existing `drive_first_batches` is a wrapper only for method tests and is
not called by the production run-loop. 32 is a fixed actor opportunity bound, not a performance-optimal value or a new experiment option.

The sustained input test uses the real Event/Frame codec, bounded queues, Worker::run and an independent native KV model.
It builds finite chains of 0/16/256 in which Tokenize resupplies the next valid PREFILL, checks normal completion, output, and KV/release on every stage,
and then compares **the number of Tokenize calls before the first Logical** against an independent literal 32.

| Chain | Before fix | After fix |
| --- | --- | --- |
| 0 | 0 | 0 |
| 16 | 16 | 16 |
| 256 | 256 | 30 |

The numbers are Tokenize call counts. Other handles such as SESSION and token input are included in the same 32-event budget, which is why
30 appears. This metric is not to be renamed as total head native work time, TTFT or total input processing count.
The RED is preserved in `target/worker-ingress-quantum-regression-20260907-01/00-before-fix.stdout.log`.
The final ordinary loop **5/5** also keeps the existing depth=2 positive control of an **additional issue without input**.

### 8 actual run shutdown/control tests

`worker/turn_tests.rs` is the real run/codec path for a single head. Only the native Frame is fake, and there is no tail.
A normal SESSION redelivery checks the event/ACK causal ID and payload. Independent tokens/positions per request are checked as well.

1. If SESSION is injected during the first Logical execution, exactly one corresponding ACK exists before the second Logical starts.
2. If a stop occurs during the first native execution, native runs only once and requests 2/flight 1 are preserved.
3. When input EOF is observed, native 0, and abandoned is recorded with requests 2/flight 0.
4. On idle EOF, local work is empty, but the state is closing **while** native cleanup runs. It is closed only after cleanup succeeds.
5. An unload failure with active work is a top-level cleanup failure and preserves the original abandoned state and requests 2/flight 2.
6. A lost native response plus an unload failure keeps both the original error, prepared Uncertain and the effects fence, and the cleanup error.
7. Even when idle, an unload failure is top-level failed. Merely appending a failure string to a normal closed prefix does not pass.
8. After one stale SESSION is rejected and idle EOF follows, the request rejection reason is left in previous. The test does not turn a non-fatal event rejection
   into a task fatal or create local leftovers.

The first 3 gave **0 passed / 3 failed** on the unmodified run and passed after the fix.
`target/worker-turn-red-20260907-01/verification.md` has the actual recompile, raw output, and source/exe hashes.
A further audit found that the new shutdown implementation also left a closed prefix on an idle unload failure.
It was fixed after sealing **5 passed / 2 failed** in `target/worker-cleanup-red-20260907-01`.
The final 8 keep both the existing assertions and the pre-cleanup observations.

`finish_run` separately preserves requests, pending, release/settle, prepared issues, flights, effects, owner/frontier, and
receive Running/Uncertain/fence before cleanup. The query regressions for `active_counts/active_slots/shutdown_status`,
**4/4**, use real ledger transitions. Completed receipts/Released tombstones are not unfinished work;
Stopped KV is unfinished until released, and Uncertain with active_attempt=None is also unfinished.
Passing the query functions and passing the real shutdown consumer are not merged into one test.

**Not guaranteed:** global graceful drain, delivery of a cancel terminal for each request, network/mailbox leftovers of 0,
control progress in a general saturated cyclic network, forced interruption of synchronous native work already running. A single input PHYSICAL/SETTLE/
RELEASE event can contain multiple native operations and a wait on publisher Full. The turn bound is
head's voluntary Logical issue opportunity, not a time or count bound on every native call.
Approving/preserving a native result started before the stop is not a new execution, and discarding that result is not called a rollback.

### Independent mutations and restore

| Independent copy mutation | Failure detected |
| --- | --- |
| ingress bound set to MAX | The literal 32 bound fails at chain 256 |
| unconditional recv even after a successful issue | The second issue at depth=2 before terminal cannot happen |
| drain multiple logical issues before rechecking input | The second native overtakes the SESSION ACK |
| stop/EOF/native entry guards removed together | stop native 2≠1, EOF native 2≠0 |
| unload error ignored | 2 tests fail: active leftovers / preservation of the original native failure and cleanup error |
| final closed announced before cleanup | The observation inside native.shutdown is not closing |
| previous deleted | The non-fatal stale SESSION rejection reason is lost |
| promotion of cleanup failure to top-level failure removed | closed prefix despite an idle unload failure |
| completed history included in flight/owner | Each ledger's active-0 judgement fails |
| Stopped frontier excluded | KV leftovers before release are missed |
| receive Uncertain ignored | The unfinished state without an active attempt is missed |

The flight/owner row in the table is two separate mutations. That makes **12 mutation arms** in total; the stop/EOF group is one arm that broke two tests,
not a claim that each guard was mutated separately. Per-path details and restore comparisons are in:

- `target/worker-actor-quantum-mutations-20260907-01/verification.txt`: 2 actor mutations, actual compile, 133-file comparison,
  baseline/restore loop 5/5. The shutdown in this copy predates the later cleanup fix and does not count as verification of that fix.
- `target/worker-turn-mutations-20260907-02/`: 6 actor/shutdown mutations, with per-arm actual code, exe, output and restored copy.
- `target/shutdown-count-mutations-20260907/verification.json`: 4 query mutations, each 3 passed/1 failed, restore 4/4.
  This is a separate copy with the surrounding actor frozen, so it is not added into the current full workspace tests.

The turn copy gave baseline/restore **8/8** with 143 files matching in content. On the first restore, a stale mtime made Cargo
reuse the M6 executable in 0.04 s, giving 6 passed/2 failed. That run is excluded from approval, and its raw output
is preserved. In the copy, the content was restored and the mtime updated, and then an **actual 3.43 s recompile with 8/8** was confirmed.
In the original, the four ledger files were given their final formatting; the mutation results are attributed to each sealed copy and the full tally to the
final working tree below. Matching sources alone are not taken to mean the executables were also rebuilt.

### Final working tree tally and resume criteria

- `cargo test --workspace --no-fail-fast`: **1039 passed / 0 failed / 7 ignored**, **57 summaries**, final exit 0.
  staged adapter lib **343**, agent core lib **149**. `target/worker-actor-workspace.log`.
  An increase of 13 over the previous 1026: sustained ingress 1 + actual actor/exit 8 + ledger queries 4.
  Feature exclusions and older runtime tests are kept apart, and the full 1039 is not called current event E2E.
- JS **62/62**, skipped 0: harness 57 + native builder wiring 5. `target/worker-actor-js.log`.
- staged adapter/agent core `clippy --all-targets` exit 0, warnings remain. `target/worker-actor-clippy.log`.
  The staged lib-test was recorded with 23 warnings (13 duplicates); this is not reported as warning-free or 0 new warnings.
- Aggregate SHA256 over **364 files** of Rust source/Cargo:
  `2dbdbfe4a41b2ccc190b9188c257b08b77130cd95fc33995ae2cdb7da87e66d4`.
  The list, lengths and raw hashes are preserved in `target/worker-actor-rust-source.json` and the re-verification tool.
  The sort/canonical rules are the same as the earlier seals, and later document edits are not included.
- After the later document update: tracked **73 clean** / `--all` **79 clean**, self-tests **12/12**,
  `cargo test -p p4-agent --test docs_lint` **1/1**, default `git diff --check` exit 0.
- C++/CUDA build, real models/GPU, VRAM-only/RAM offloading, multi-computer runs and performance non-regression were **not run in this round**.

The sole owner of the current status and the first next action is the latest progress section of the roadmap. This tally is not approved as full B2 completion
or as a final heavy-wave result. The target raw data is preserved locally and is not committed/pushed evidence.

## 2026-09-07 follow-up — speculative actual run and the native logits consumption boundary

HEAD `a9e1967fc` + uncommitted working tree. The previous 364-file Rust seal was rechecked at the start, and then
the actual run fixture was extended. The existing 5 ordinary tests and the oracle of 1 append per position are kept.
The native fake in `worker/loop_tests/speculative.rs` does not call production state transitions; it owns the literal
inputs, responses and KV history. Worker::run, codec, issue, return, control and output are the real production path.
It uses OS threads and an independent routing pump, and does not go through LOAD/subprocess, EventNode/broker/network or a llama model.

### Normal paths and independent oracles

Each of the 3 new tests runs at **2 and 4 stages**. Together with the 5 ordinary tests, actual run is **8/8**.

| Scenario | Results that must be observed |
| --- | --- |
| Full accept | SETTLE 0, exact 5 tokens/positions/length; while RELEASED is held the next request is not issued, and after release the same slot with a different incarnation proceeds normally |
| Direct partial | tentative KV `[10,11,12,1000,9001]` is trimmed at position 4; the correct token is re-appended from position 4; 4 tokens/positions/length |
| Checkpoint Replay | the same tentative KV is restored to **position 3**; retain 5 is the end to be filled going forward; unconfirmed Verify output 0, and the confirmed result of the output=false Replay is output normally; 4 tokens/positions/length |

Direct/Checkpoint separately hold back the last SETTLE hop and the single tail→head SETTLED.
They check whether the global Verify fence keeps logical issues at 2 even when a separate request has been tokenized
and made runnable. This is not a test that stops naturally because the target request has nothing ready.
The full append→trim/restore→reappend→release history of each stage and the identical control operation ID are compared.
The native fake's sampler_calls is the number of scripted decisions, not proof of the number of real sampler primitive calls.

In Full, the last Verify fence is also kept until RELEASED. So there is an end-to-end observation that nothing is reused before release,
but we do not call it an independent mutation proof of the free-slot return mechanism alone.

### 3 production consumer mutations

In the independent 3-crate/132-file copy recorded in `target/b2-speculative-run-mutations-20260907/verification.json`,
only the production consumer code was changed, with the tests and the fake held fixed. Each arm's actual compile and exe/source hashes are preserved.

| Mutation | actual run result |
| --- | --- |
| Remove drive's Verify fence check | 6 passed / 2 failed; with a separate ready request, native logical 3≠2 |
| Drop Replay's real output intent | 7 passed / 1 failed; the final output count falls short |
| Omit releasing the Verify fence after full acceptance | 7 passed / 1 failed; later issuing stalls |
| Recompile after restoring the original | 8 passed / 0 failed; 132-file baseline content matches |

The first mutation also produced a teardown error from fake Mutex poisoning after the primary assertion. Both the initial 3≠2 failure and
the full libtest summary are preserved, and the extra teardown error is not counted as a separate defect detection.
SHA256 of the two original test files:

- `loop_tests.rs`: `c6e2428dd06a9a2d6d359c475dade2fd520969274715bc29e79adf835613831d`
- `loop_tests/speculative.rs`: `5466a0265158dce0606147039204f80f57e5515d2cf8b00033b8edbf0238de3d`

### Keeping the full Rust suite apart from the independent native audit

`cargo test --workspace --no-fail-fast` gave final exit 0, **1042 passed / 0 failed / 7 ignored**,
57 summaries. The staged adapter lib is **346**. The raw output is preserved in `target/worker-speculative-workspace.log`.
Only the 3 actual run tests above were added since the previous 1039, and the total is not called the number of event E2E tests.
The SHA256 of the **365 files** of Rust/Cargo is
`9f1dbc577f78336bab97d088a035398c07b4799795c371dfe356913cfb93b841`.
`target/worker-speculative-rust-source.json` and `worker-speculative-seal.mjs --verify` confirmed that the content was identical before and after
the full test run. clippy --all-targets gave exit 0, but the staged lib-test still has **24 warnings
(13 duplicates)**. This includes 1 clone-style warning from the new fixture, so it is not called warning-free.

An independent code audit found a native defect that differs from what the fake's success shows. The real `execute_physical` passed the Replay
wire output=false straight into llama_batch.logits, but `sample_physical_mtp` read logits from every
Replay row. The default embeddings=false path at pin `0eadefebd3` does not produce those logits.
The same problem must also be checked in FIRST batch construction. The fake does not call the native sampler,
so the 1042 GREEN does not disprove this defect. The native consumer tests below are separate evidence.

### A separate next counterexample — busy UNLOAD, not yet fixed

`target/b2-unsafe-unload-red-20260907/verification.json` is an independent copy outside the original 1042 tally above.
In the existing actual run ordinary fixture, one tail capsule-set was held back and an UNLOAD for the valid generation was
sent. Production code was not changed; only the test file containing the new counterexample changed.

```text
UNSAFE_UNLOAD native_shutdowns=1 rejected=0 approved=1 held_tail=1 outputs=0 snapshot=unloaded
test result: FAILED. 0 passed; 1 failed
```

The actual 25.19 s recompile and the 132-file/executable hashes are preserved. The first safety assertion, native shutdown 0 while busy,
fails with 1≠0. Later in the test there are positive controls — normal output, KV and release on every stage for the original work, and
a successful idle UNLOAD — but the test stopped at the first assertion, so they **were not run**. There is no fix/restore GREEN.
This counterexample is not covered over by the current normal suite GREEN, and the first next action is owned by the latest roadmap section.

### native mask fix and real consumer regression

FIRST/downstream in `llama_stage_runtime_physical.cpp` were changed to build an internal logits mask
of their own. The original owner/logical/capsule masks are not written to. Policy, common P4, upstream patches, ABI and
common dependency permissions were not changed. The actual production change is 17 lines added / 2 lines replaced.

The new `physical_logits_consumer_test.cpp` compiles the production physical.cpp body directly. Only the model/context
setup and the native API boundary are replaced by a test-only Probe, which observes the mask, tokens and positions that reach the real
llama_decode/encode calls. The llama library itself is not linked into this executable.
The initial mixed-mask test failed with an assertion in FIRST before the fix. all-Replay was added after this RED,
so we do not claim that the initial RED ran the later 12 cases.

The final cases are first/middle/tail × decode/encode × all-Replay/mixed, **12 cases**. The all-Replay
literal mask is `[1,1]` and the mixed one is `[0,1,1,1,1,1,1]`; downstream's input/return wire masks are
unchanged. The mixed geometry and atomic preparation are fixtures; they do not verify a legitimate real recurrent shape.
A separate server consumer that restores FIRST's internal capture mask into wire owner.output was confirmed in code
but not run in this test. These 12 cases do not approve real logits values, the sampler,
checkpoint bytes, or model or backend conformance.

Production consumer mutations in an independent 81-file copy: FIRST uses the old row.output; downstream discards the computed mask
and forwards the old input.output; the native mask contaminates the return wire — **1 CTest failed in each case**.
Between mutations, the actual recompile of the test TU and the executable hash were confirmed; after the exact restore, **1 passed**,
with 0 content mismatches across the 81 files. Details are in `target/replay-logits-consumer-mutations/verification.md`,
and the initial RED is preserved in `target/replay-logits-consumer-evidence/red-*`.

- Final production source: `E90AF312E7489DCA02564BCEA6E884606B9FE4C8E31872A2D798CC221F547C7E`
- Final consumer test: `D5DF40C0D51BA68BE3119EC36D66F88A5822B40308CAAA50D54415A7C00EC0C8`

### Official native build, kept apart from bodies that did not run

The official no-llama build initially hit C1083 and **ran 0 tests**. The compat include in `server_hello.cpp`
was unconditional, unlike its use sites, and was fixed by adding the same P4_STAGED_WITH_LLAMA guard.
It was not made to pass by widening the include path/link. After the fix, the **5/5** contract tests actually ran.
Both modes below were run again after the final CMake/builder/native freeze.

```powershell
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --backend cpu --build-dir F:/dev/p4/target/native-identity-cpu --config Release --parallel 4
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --no-llama --backend cpu --build-dir F:/dev/p4/target/worker-spec-native-contract --config Release --parallel 4
```

| Final mode | Run result and scope |
| --- | --- |
| llama-linked CPU source build | exit 0, CTest executables **15/15**; model-free bodies **12**, partial run **1**, whole body SKIP **2** |
| no-llama build | exit 0, contract executables **5/5**, SKIPped bodies 0 |

Of the 15/15, the compile test skipped the two parts for real KV restore/HOP rollback, and the request_options/MTP
executables skipped their whole bodies because there was no model. The **4 lines** of raw SKIP output are tallied separately. The 15 are
not called a pass of model correctness. The separate model-required run of upstream recurrent rollback was not done either.
For the new consumer test, the real TU was recompiled with a target-only Rebuild, and the 12-case completion
message was confirmed in the official CTest. The /UNDEBUG assertions in Release are also active. The 5 no-llama and 15 linked executables include duplicate
targets, so they are not summed as 20 independent conformance tests.

`target/worker-spec-native-contract-evidence/final-linked.*` and `final-no-llama.*` hold the commands, exits,
LastTest and executable hashes. The **111 input files** of native/CMake/builder/prepare/manifest/patch were
identical before and after each run and between the two modes, and a final rehash of the originals also found 0 mismatches. Real production runtime
object/link success is kept apart from the scope of the Probe consumer test above.

Before and after the official prepare, pin `0eadefebd3f8f92a86d634a0e5b8fffc9dc792c0`, patch diff
`3cfc636181e4ee1033249b8f7ca4d50156174138bf07c2797a476f4c42ab8c47`, recomputed tree
`c81ecd4fff7c93a1f637d63733c0387f0b6bf157`, and the byte hashes of the 24 patches matched the manifest.
Model env was explicitly excluded from the child builds/tests, and there were no model/GPU/remote runs.

### Other final gates and limits for this slice

- JS **63/63**, skipped 0: harness 57 + official build target wiring 6. A mutation that leaves the new target as dead code only
  is also detected by the builder's actual command capture test. `target/worker-speculative-js.log`.
- private-header **81 files clean**, common debt **0 header / 5 source**; compat manifest valid 24.
  The string/list gates are not extended to mean full transitive semantic isolation is complete.
- Documents tracked **73 clean** / all **79 clean**, self-tests **12/12**, cargo document gate **1/1**.
  Default git diff --check exit 0; checked after consistent per-file EOL normalization.
- The Rust sources were rechecked at the end against the 365-file seal above and are identical. Outside the current whole-suite 1042 GREEN,
  1 independent busy UNLOAD RED remains, and it was not erased.
- CUDA build, MTP/Replay on real models, VRAM-only/RAM offloading, multi-physical-host waves and quality/TPS were
  **not run** in this slice. No remote deploy or commit/push was done either. The raw data target is preserved locally.

## 2026-09-07 follow-up record — busy UNLOAD and native shutdown failure

The baseline is the same HEAD `a9e1967fc` and an uncommitted working tree. The busy UNLOAD RED from the previous section was moved into the original actual
Worker::run, and normal rejection was separated from post-hoc native cleanup failure. The P4 neutral core, policy and native
ABI were not changed in this slice. The semantic contract is owned by the explicit UNLOAD section of the batching contract, and the verdict by T25.

### Pre-fix failures and real consumer scope

The two ordinary tests in `target/b2-busy-unload-original-red-20260907.log` and
the two speculative tests in `target/speculative-unload-red/red.log` sent the existing command into a real run.
Each failed at its first safety assertion, with native shutdown at 1 instead of 0 and UNLOADED at 1 instead of 0. The RED is
not counted as having run the later normal completion and idle positive controls. Source/exe hashes and the raw recompile output are
preserved in each evidence directory. A second ordinary RED with stronger helper diagnostics is also in separate raw output.

| Added actual-run regression | Real work held back | Checks after the fix |
| --- | --- | --- |
| ordinary head (N=2/4) | tail capsule-set; flight batch 1, executions 2 | 1 busy error, native/output unchanged; idle succeeds after the same request completes |
| ordinary middle (N=4) | requests/pending/flight 0, but active owner/frontier 1 each | KV preserved even without a head request ledger; then normal completion / idle success |
| speculative head (N=2/4) | final SETTLED; flight/open view 0, pending settlement 1, Verify fence | wrong FIRST/LAST generation rejected; after settlement resumes, literal output/release/idle success on every stage |
| speculative middle (N=4) | tentative Verify KV not yet SETTLEd; requests/pending/flight 0 | native KV and append/restore history preserved; checkpoint Replay completes normally / idle success on every stage |

The common helper accepts only errors with the exact source/correlation. It compares full snapshots of native calls, sampler, KV, write/release history and
speculative history, plus the request outputs, and the pump was not changed to swallow every error.
Instead of calling a hand-built RequestState or a shared census function as the oracle, it compares diagnostics of state produced by real events.
When only success receipts/Released history remain, idle UNLOAD must be possible.
The existing ordinary 5/speculative 3 plus the 4 added make the actual pipeline loop **12/12**.

The production `control.rs::Worker::unload` goes through `shutdown.rs::Worker::require_idle_unload` after the identity check.
It shares the existing local leftover tally used for shutdown, but this guard does not block failure cleanup. The fatal/Drop cleanup of an
already fenced worker may close native resources. So native 0 is a promise of healthy busy pre-rejection,
not a promise for every kind of shutdown. This test does not go through LOAD/subprocess, a real llama/model/backend,
a network broker or global drain. It does not cancel tokens already delivered or prove a user output ACK.

### Is the guard really needed — independent mutations

`target/b2-busy-unload-mutations-20260907/verification.json`: in an independent 3-crate/133-source copy,
the tests/fake were held fixed and only production code was changed. Every arm preserves the actual recompile and the contemporaneous executable/raw output hashes.

| Condition | loop passed / failed |
| --- | --- |
| Baseline | 12 / 0 |
| UNLOAD guard call removed | 8 / 4 |
| requests-only busy judgement | 10 / 2; only ordinary/speculative middle KV fails |
| unconditional busy rejection | 8 / 4; fails in the idle positive control after the original request completes |
| exact restore | 12 / 0 |

Each mutation differed in only one file, control.rs or shutdown.rs. The restored copy, the baseline and the current original matched in content
across 133 files. No mutation was made to pass by reducing test totals, relaxing bounds or changing goldens.

### A separate finding — SESSION approved after an idle native UNLOAD failure

A real run was fed SESSION → idle UNLOAD → an already queued SESSION, with only the native shutdown made to fail.
`target/worker-unload-failure-regression-20260907-01/01-before-fix-compiled.stdout.log` shows, after an actual
recompile, **0 passed / 1 failed**. Even with an error after 1 cleanup, the following SESSION was ACKed, and
the final state was `closed:local_work_empty`, `effects_fenced:false`. The earlier 00 log is a supporting observation that reused concurrent build outputs,
so it was not used in place of the fresh compile evidence in 01.

On a native cleanup Err, the effects fence is now raised so the real handle shuts down after the error response. The original error and
the uncertainty boundary are preserved, and UNLOADED and follow-up SESSION ACKs are 0. The existing 8 turn tests + the new regression give **9/9**.
In an independent copy, removing just the one fence line gives **0/1 failed** on the same queued-SESSION assertion, and the restore gives **9/9**.
The baseline/restore/original/evidence copies of the 135 input files (Cargo included) match, and each step has actual compile and exe hashes.
`target/worker-unload-failure-regression-20260907-01/verification.txt` records the commands and the full scope.
The fake shutdown count is not proof of real OS process termination or destructor retries, and failure lifecycle reload was not implemented.

### Final sources and full tally

- control.rs: `50046B4A9B973D265833B3DEC65347F05ECADCC6916B5354290BB15E74D031DD`
- shutdown.rs: `3A673D5B1929DB19AE906E666525570D4DE0868650B966A610FCD0652811EB2C`
- Rust/Cargo **366 files** seal: `7e3257ed5f97f9ce586d6c3871f3e3621e5619f0208e1a1e960f2aa143478e23`.
  `target/worker-unload-rust-source.json` and `worker-unload-seal.mjs --verify` confirmed identity before and after the full run.

The first `--workspace --no-fail-fast` run gave **1046 passed / 1 failed / 7 ignored**, exit 101.
The only failure in `target/worker-unload-workspace.log` was the document gate for mixed EOL in the new batching contract paragraph.
After normalizing it and rerunning everything, `target/worker-unload-workspace-final.log` shows final exit 0,
57 summaries, **1047 passed / 0 failed / 7 ignored**. The staged adapter lib is **351**; relative to the previous 1042,
only busy 4 + native failure 1 were added. The 1047 are not called a count of real-hardware/E2E tests.

JS is **63 passed / 0 failed / 0 skipped** (harness 57 + build wiring 6), with raw output in
`target/worker-unload-js.log`. clippy staged/agent-core --all-targets gave exit 0, but the staged lib-test
still has **26 warnings (13 duplicates)**. This includes 2 clone-style warnings from the new speculative fixture.
private-header is **81 clean**, common debt **0 header / 5 source**, and the compat manifest is valid 24.
C++/CUDA were not rerun in this Rust-only slice. The model-less CPU15/no-llama5 from the previous slice are not
relabelled as current real-model conformance or as new runs.

Cancel/Drain, OUTER consumption ACK, restart freshness and edge credit are still unfinished. Real models/GPU, remote deploy,
VRAM-only/RAM offloading waves, performance non-regression and commit/push were **not run** in this slice. The raw data target is
preserved locally and is not a published immutable evidence bundle. The next work and the approved real-hardware order follow the latest roadmap.

### The next P1 outside the full GREEN — real head output and OUTER's tail assumption

In the independent copy recorded in `target/head-output-consumer-red-20260907-01/verification.md`, the output
mailbox of a real run was observed. The existing ordinary 2-stage test gave **1 passed** with 5 outputs; the existing checkpoint Replay 2/4-stage
test gave **1 passed** with 10 outputs. The existing token/position/KV/release oracles are unchanged. These 15 Events were
preserved as-is via `p4_protocol::event::encode` and fed into the real event-drive `InferenceIdentity::output`.

The ordinary consumer gave **1 failed (5/5 rejected)**, and the checkpoint consumer **1 failed (10/10 rejected)**. All errors were
`inference event source or correlation is incorrect`. A separate control group that kept correlation/body/target/load/session fixed and
changed only the source to the configured tail gave **1 passed** each, with all 15 approved. This is a control group using the wrong past source to isolate
the cause, not a conclusion that tail publishing should be allowed again.

The producer fresh compile of 27.02 s, the consumer fresh compile of 19.65 s, the EXE/367-input-file hashes, the 15 real encoded
outputs, and the raw output of both consumer REDs are preserved. The only changes are the observation/consumption probes in two test files of the copy;
the production algorithm and the original are unchanged. We do not claim that this new copy test is included in the full original 1047 GREEN.
The producer's network and model computation are fake, and the full `inference::drive` or a real GPU wave was not run.
The current strict unit test actually expects head to be rejected, so the fix must not be to make only the consumer accept both head and tail.

The final document checks were also run: tracked **73 clean**, all **79 clean**, self-tests **12/12**; the document gate inside the full Rust suite
**1/1**. After adding the final evidence paragraph, lint and the default diff --check are rechecked, and the Rust 366-file seal
is not changed. Git's warning about future LF→CRLF conversion remains, but there are no whitespace errors or exit failures.

## 2026-09-07 follow-up implementation — the real production/consumption contract for head OUTPUT

The baseline is `a9e1967fc` + this document's later uncommitted changes. The independent RED from the previous section was moved into the original default tests.
`tools/event-drive/src/run/inference_identity.rs::InferenceIdentity::output` requires the configured head's
full endpoint. It distinguishes the tail, which is the sampling location, from the approved OUTPUT publisher, and the earlier load/session/
request/route/position checks and the rejection of duplicates and post-terminal output were not removed. The adapter producer already
implemented head publishing, so this production fix is one OUTER consumer file. The P4 neutral transport and the llama ABI are unchanged.

### Persistent regressions and evidence scope

- `adapter/test-fixtures/head-approved-output-v1.json`: 15 encoded OUTPUTs captured from the actual Worker::run
  (5 from ordinary 2-stage, 10 from checkpoint Replay 2/4-stage). The test-only `head_approved_output.rs` is
  read by both the adapter and drive. No new public production API or optional feature was added.
- The real producer default loop test compares the current output against the semantic projection of this wire. It compares the full source/target/reply route,
  protocol/class/content/adapter/correlation/deadline, and the entire Outcome JSON. The existing literal
  token/text/position/KV/settlement/release oracles were kept. loop **12/12**, with the comparison added to the two existing tests.
- The per-run values of event_id/causation_id/sequence are excluded from the semantic projection. ID uniqueness, Event
  validity, causation presence, per-source sequence increase and per-request position order in the current run are checked separately. **Causation presence is not proof
  that the ID matches the exact causing terminal.** The capture file is a set, not an ordered log, so on consumption it is sorted by per-request
  position. The producer's live order check runs before sorting.
- The real `InferenceIdentity` and `inference::drive` consume the same 15. They go through an in-memory framed EventWire and the real
  state loop, and also test negative inputs for source/generation/route/load/session/request/position, duplicate identical Events, and output after terminal.
  The RELEASED used for termination is **synthetic**. It is not proof of the real release set or of network/LOAD/native/GPU;
  the default tests of the two consumer modules are **7/7**, and event-drive as a whole is **22/22**.

The fresh compile RED before the original fix remains in `target/head-output-consumer-original-red.log` and the snapshot of the same name as
**0 passed / 1 failed**. If only the previous tail is allowed, the head positive fails. The previous section's run that rejected all 15
is kept apart from this persistent default regression.

### Independent mutations — each side of the boundary must fail on its own

| Mutation | Baseline | Faulty change | Restore |
| --- | --- | --- | --- |
| producer publishes from base's tail source instead of head | 12/0 | 10/2 | 12/0 |
| fixture response text corrupted at the same length | 12/0 | 11/1 | 12/0 |
| consumer tail-only regression | 7/0 | 3/4 | 7/0 |
| consumer accepts every configured node | 7/0 | 5/2 | 7/0 |
| consumer route check removed | 7/0 | 5/2 | 7/0 |

The table shows passed/failed. `target/head-output-producer-mutations-20260907-01/verification.md` preserves each fresh
compile, EXE, the 370 input files, and restore/original mismatch 0. The producer's 370 also include the existing deployment's
`fixtures.json`. The checkpoint mutation fails first in the 2-stage iteration, so that failing run is not counted as having
gone through 4-stage. The normal and restore runs cover both sizes.

The first producer restore copied file mtimes too, so Cargo reused the mutated EXE. That log is preserved as `INVALID-*`
and excluded from the valid restore. The actual recompile with updated mtimes and 12 PASS is recorded separately. The original checkout was
not used for mutations. `target/head-output-consumer-mutations-20260907/verification.json` preserves the match of the consumer-side
161-file baseline/restore/current original and the actual compile, EXE and raw log of the three mutations.

### Final tally and sources

- consumer production SHA256: `6DF744B8F0321FBCD007AB683FB8ED8F453C4E56F825078BBBBB852D1519FDFB`.
- shared OUTPUT JSON: `5C566577E988048335BD34CA10ECA94F8E96F7A448D7BBE126CBF7A221F40D21`.
- shared test helper: `6CCC6AC81EBD535BAA6E8D881610BCC5C4B514A8309E9E2A91E32DFFDE99CA2F`.
- Seal of Rust/Cargo + the new OUTPUT JSON, **369 files**, before and after the full run:
  `b6c164b21e3131f421a195cc0d4a6c0f3d673a7b548aaa862a2aab7f367d0dff`.
  `target/head-output-rust-source.json` and `head-output-seal.mjs --verify` were used. This 369-file aggregate
  does not mean it covers every build input, including the existing deployment JSON, documents and compiler/registry sources.

`target/head-output-workspace.log`: `cargo test --workspace --no-fail-fast`, final exit **0**, 57 summaries,
**1051 passed / 0 failed / 7 ignored**. That is the previous 1047 + 4 real consumer default regressions; producer parity strengthened two existing
tests. `target/head-output-js.log`: harness57 + build wiring6 = **63 passed**, failed/skipped0.
`target/head-output-clippy.log`: staged/agent-core/event-drive --all-targets exit0, but warnings remain.
staged lib-test 26 (13 duplicates), event-drive bin-test 7 (6 duplicates); this is not reported as a warning-free completion.
private-header **81 clean**, common **0 header / 5 source**, current compat manifest **valid 24**.
C++/CUDA, remote, real models, VRAM-only/RAM offloading waves, performance non-regression and commit/push were not run in this slice.

### The next three consumer REDs outside GREEN

`target/outer-inference-consumer-probes-20260907-01/verification.txt` and `consumer_probes.rs` record
1 normal PASS / **3 RED** for the rejection rules. The authoritative run is `03-final-identity-counterexamples.*`.
The fixed head source and final formatting were copied and recompiled. This is a copy test not included in the original 1051.

An independent peer receives and validates the PREFILL sent by the real drive, and then returns BATCH_OBSERVATION,
OUTPUT and RELEASED for the current head/route. It goes through drive's real state→execute, the same observation aggregation, and the real acceptance.
The normal control is prompts [4,7], first positions [4,7], two tokens each, exact readable responses, and different release requests.

1. With only A in the real peer release set, sending a fresh-ID RELEASED(A,count=1) twice is approved as completed2/released2.
   This does not prove that a normal producer makes this mistake; it shows that the consumer cannot prove the release set.
2. For a max_tokens=1 submission, sending two contiguous OUTPUTs with the second ending in length is approved even though sampled2.
3. With prompt observation [4,7], shifting all of B's positions by +1 and sending first positions [4,8] is approved when there is no optional common bound.

The before/after/copy match of the final 151 sources, EXE `06AA14DEDDC06EC9B158805618EE445746286E1437D9534FF5FDB6F5D65AD3D5`,
and cargo exit101 are preserved. The original was not fixed. The peer's release set is independent of its own payload count but
is not a real native KV observation. RPCs for CREATE/LOAD/UNLOAD/DELETE and a real GPU wave are not included either.
The mandatory completion-verdict counterexamples were registered in T20, and the concrete next order is owned solely by the latest roadmap.

## 2026-09-07 follow-up implementation — real consumption of the OUTPUT budget and fresh-prefill observations

The baseline is `a9e1967fc` + later uncommitted changes. Of the three consumer counterexamples from the previous section, the output budget and the per-request first position
were fixed. **Release membership has not been fixed yet.** Checks were added on both the real drive and final acceptance so that completion is not approved
merely because the source is approved and normal text exists. The P4 neutral transport and native ABI are unchanged.

### Implemented boundaries and proof still missing

- `tools/event-drive/src/run/output_budget.rs::validate_output`: validates budget/terminal using the sampled count already received and
  the one incoming OUTPUT. An empty EOS also counts as 1, and `length` is only possible at the end of the budget.
  `stop`/`eos` may end early; an unknown reason, or a non-terminal after the bound is reached, is rejected. The real drive
  calls it before changing response/outcomes/completion. acceptance rechecks all preserved outcomes.
- `inference_evidence.rs::apply_observations`: after validating the whole candidate, it installs the per-request counters once.
  A redelivery of the same body under the same observation ID is deduplicated on the existing insert path. If a different observation ID
  reuses the same physical execution, or creates an unknown request or a sum overflow, it is rejected.
  Every completed request must have a positive prefill observation, and the first OUTPUT position must equal that request's sum.
  This includes a regression showing that when a later request fails, the earlier request's counters are also unchanged.
- Validation happens at the current drive's completed/released final boundary. A positive case in which the observation comes after OUTPUT and
  arrives before the last release is allowed. **Reconciliation that waits for late observations after that boundary is not implemented.**
  Missing or mismatched data is an error, not a success report. The duplicate post-processing tally in execute was removed; the current fake-peer
  tests go through drive and acceptance but not through the full CREATE/LOAD/UNLOAD/DELETE RPC orchestration.
- This position check relates a fresh position 0 submission to head-reported prefill evidence. It is not a position proof from an independent native tokenizer,
  real KV, or Restore/LCP. Protocol-valid for an empty EOS is also kept apart from a nonempty/minimum response
  quality pass. The existing full response / minimum length / judge checks were not relaxed.

### Default tests and the real production path

The **15 tests** in `consumer_budget_boundary_tests.rs` have the peer first read and check the PREFILL sent by the real drive,
and then respond through a framed EventWire over a bounded duplex. They use independent prompt row counts [4,7] and exact readable responses.
They cover budget overrun, changed first position, missing observation, exact redelivery, ID reuse, altered body, reverse order, late observation, empty EOS,
early stop/eos, early length, unknown stop, and output after terminal. The tests do not fill in the request counters themselves.
A rejection must be a **real drive Err**, not just final acceptance=false. The release notification is synthetic.

The existing 15 actual producer OUTPUT JSON entries were not modified:
`5C566577E988048335BD34CA10ECA94F8E96F7A448D7BBE126CBF7A221F40D21`.
The shared helper reads the request/physical prefill from the BATCH_OBSERVATION among all events received by the actual loop,
and compares them with independent workload constants ordinary=7, partial=3, fence-probe=1, which are not back-computed from the OUTPUT position.
The existing token/text/position/KV/release oracles for ordinary 2-stage and checkpoint Replay 2/4-stage are kept.
The producer is fake native, so this is not an independent tokenizer proof. The observations/RELEASED newly wrapped around the existing fixture consumer
are synthetic, and only the original OUTPUT is an actual capture. The real producer loop stays at **12/12**.

The initial empty-EOS consumer test was blocked by the config's ban on `exact_response=""`, giving **14/15** and full drive **46/47**.
The input itself was an illegal fixture, so the expectation was changed to None. The EOS token/text, min length=1, and empty-response
quality failure assertions were kept. This initial RED is counted neither as a production code defect nor as a success.
The raw output is in `target/outer-budget-boundary-initial.log` and the independent record, and the final drive is **47/47**.

### Real calls and independent mutations

| Target / faulty change | Baseline | Mutation | Exact restore |
| --- | --- | --- | --- |
| Remove only the budget check call in the actual drive | 15/0 | 12/3 | 15/0 |
| Remove only the observation apply call in the actual drive | 15/0 | 6/9 | 15/0 |
| Remove the budget early-length check | 47/0 | 44/3 | 47/0 |
| Allow an empty-EOS budget exception in budget | 47/0 | 46/1 | 47/0 |
| Remove the acceptance first-position check | 47/0 | 46/1 | 47/0 |
| Misclassify the producer's prefill statistics as decode for both aggregate and request | 12/0 | 10/2 | 12/0 |

The table shows passed/failed. The empty-EOS mutation failed only the pure helper test, and the acceptance mutation failed only the corresponding final check test.
Other layers' defenses remain, so we do not claim that all of them failed as far as the actual drive. In contrast, removing the budget call
makes 3 tests fail because drive wrongly completes even though acceptance still rejects. The production misclassification does not change the physical row count, native behavior
or output; it changes only the two observation counters. The existing OUTPUT/KV oracles pass, but the new prefill check fails.
The checkpoint mutation failure stops at 2-stage, while baseline/restore run both 2/4-stage.

- `target/consumer-budget-boundary-regressions-20260907-01/verification.txt`: actual consumer mutations,
  raw output, source/EXE. A fresh compile with the same tests in the preserved earlier consumer copy (not HEAD itself) gives **6/9**.
  6 are wrong stream approvals, and 3 fail the new final counters postcondition. Those 3 are not called rejections of
  formerly valid input. The authoritative runs 04/05/06/10/11 all record actual Compiling and input before=after.
  03 is **invalid** because the copied mtime made it reuse the old EXE. 07 has different EOL and is excluded from the byte-exact restore
  evidence. 10 is an exact restore of the 154-file baseline, and 11 is 15/0 after the final producer fixture change, matching the corresponding
  153 original files. The copy's remaining 1 file is an old probe not registered as a module and is not in the execution coverage.
- `target/output-budget-mutations-20260907/verification.json`: the three pure/acceptance mutations and the final
  47/0, 167 inputs (164 corresponding to the original + independent workspace/Cargo settings), current corresponding files matching, EXE/compile raw output.
  The pre-fix acceptance cap RED is kept separately in `target/output-budget-acceptance-original-red.log`.
- `target/prefill-producer-mutations-20260907-01/verification.md`: producer baseline/mutation/restore all
  recompiled, 372 input sources/EXE, restore/current mismatch 0. This copy scope excludes the existing
  deployment JSON in the full seal below, so the count differs from 373. The original was not used for mutations.

### Final sources and tally

`target/outer-budget-source.json` and `outer-budget-seal.mjs --verify` seal Rust/Cargo plus the two literal embedded
JSON fixtures, **373 files**. Documents and compiler/registry package sources are outside this scope.
SHA256 `d234508fd86df5caff045e63994a42a5f943407d36bec67d5eb9fd32567c2b71`.

- `target/outer-budget-workspace.log`: `cargo test --workspace --no-fail-fast`, final exit0, **57 summaries,
  1076 passed / 0 failed / 7 ignored**. Previous1051 + consumer15 + budget3 + acceptance5 + evidence2.
  Strengthening the two existing producer tests is not added to the new test count.
- `target/outer-budget-js.log`: harness57 + build wiring6 = **63/0**, skipped0.
- `target/outer-budget-boundary-final-focused.log`: event-drive **47/0**.
- `target/outer-budget-clippy.log`: staged/agent-core/event-drive all-targets exit0. Warnings remain, including staged lib-test26
  (13 duplicates) and event-drive bin-test7 (6 duplicates). Not reported as warning-free.

C++/CUDA, a real network, remote deployment, models/GPU, VRAM-only/RAM offload waves, performance and commit/push were
not run in this slice. The target raw output and copies are preserved locally and are not a published immutable bundle.
The request set of a scalar RELEASED, per-owner release notifications to multiple OUTERs, and the notification intent after a publish failure are
the next audit scope outside the current GREEN. The concrete work order and stage promotion are owned only by the latest roadmap.

### Release boundary REDs outside GREEN — real production and broker consumption checked separately

The new tests below exist only in independent copies and are not included in the original 1076 tally above. The original production fix
has not been done yet. Source, raw output, inputs/EXE and scope are each preserved, and the counterexamples are not called fake GPU performance.

**Owner routing** — `target/release-owner-routing-red-20260907-01/verification.md`.
In a 2-stage actual Worker::run, the test first asserts that A and B are both in the same execution=1 terminal capsule.
After passing the original token/text/position/stop oracle and the one-KV-release-per-stage oracle, the new notification check
fails. A's ingress42991/channel owner-a/connection11/correlation-a gets released=2, while B's independent
ingress42992/channel owner-b/connection12/correlation-b gets 0. The single-OUTER positive passes with count1.

baseline12/0 (fresh compile 26.08 s), **13/1** after the two new tests (6.15 s), and **14/1** after adding a separate observation capture test
(6.83 s). Final EXE `6EDB6F3AE20DB1653DCA92793F57E35BADA2C165C8D124D9C728A4B8186835B5`,
copy loop test SHA `95DEBB54EB02E91886896A2FFBE959D841B91EE36609E634F27DC4759B64DA3A`.
372-input seal / original changes 0, and only loop_tests.rs in the copy changed. This is an actual
run using post-LOAD fake native, not LOAD negotiation, EventBroker/network, or real llama/GPU.

The real observations were also captured separately. The first independent correlation differed from event-drive's request_id=correlation premise,
so a separate actual run published the correlation of the telemetry-owner-a/b inputs as each request ID. **The generated wire
was not edited afterwards.** The SHA of `captures/request-correlations.json` is
`01C3E73F1CFE4AFB8E504B79B6CE1766544BF2A37D4A722249A14A3F1B8BB4F7`. The two real observations
go to their respective routes, but each body contains the full A/B requests, and the two stage spans go only to the A route. This production observation and
whether the real consumer approves it are recorded as separate execution scopes.

**Real consumption of the captured observations** — `target/captured-mixed-outer-consumer-red-20260907-01/verification.txt`.
The request-correlations capture above was replayed, byte-for-byte unchanged, through protocol decode→the current InferenceIdentity::observation.
A's and B's independent submission sets each contain only their own request. Both routes are rejected with an unknown-request error.
Putting the same wire into a diagnostic control that explicitly allows both requests passes. This control isolates the foreign member problem from
source/route/load/session/correlation/number format errors; it is not a fix that allows every request.

fresh compile 2.35 s, **1 PASS/1 RED**, 155 input files before=after with the raw capture unchanged, and the executed consumer/
protocol 6 files identical between original and copy. EXE `9B0A7EFD3C830859F5FACD076366E4CE7D4B9AC476E3AEE7B490F945D2AE8346`.
The same function is called in the actual drive, but this run itself only goes through the identity consumption point, not a full drive/bootstrap/
network run. The missing StageSpan B in the capture is a fact of production observation/code, not a coverage assertion of this consumer test.

**ACK source authority** — `target/released-source-authority-red-20260907/verification.json`.
An independent head→middle→tail topology and exactly two pending releases were prepared, and encoded Events were fed into the real
EventBroker::dispatch→node receiver→Worker::handle→released. tail gives **1 PASS**; the same body from
middle and from a Node outside the pipeline gives **2 RED**. Both REDs are Enqueued with ERROR0/RELEASED1, and cause deletion of the two pending entries,
re-admission of the waiting request in slot0, remaining free=[1], and release of the Verify fence. `next` is middle in this test.

The existing release tests' baseline3/0 was a 32.67 s fresh compile, and the new source test **1/2** a 17.66 s fresh compile.
With the RED EXE `c6e7754b24a53e79653af8bd22c2c0d9d3524a975b397e47255b150daf672a0e` and the same sources/executable,
`--test-threads=1` was rerun to keep complete before/after raw output. The SHA of `red-serial-output.log` is
`0744604305919a7b2a51758eba5181f16fc7125a811c7de1795c7310d47ba509`. 242 inputs preserved, with production difference0 across the 239
corresponding to the original. The independent workspace, test dev-dependency and reduced lock are stated explicitly, and the shared registry versions and checksums are the same.

This test is real broker/handler consumption after injecting pending state. Generating the full original native RELEASE chain,
EventNode async run, TCP authentication and GPU execution were not run. "External" means a Node envelope outside the pipeline; it does not prove
an intrusion from an external network. The model meaning of the source role belongs in the adapter's SESSION contract, and a fix that puts llama knowledge into the neutral
broker is not proposed.

**A separate surface where only the code audit is complete**: release.rs changes pending/slot/admission after ACK validation and emits directly.
There is no actual regression yet that preserves the notification intent in effects after Closed or event ID exhaustion.
This is not mixed with the two REDs above into a claim that "three defects were actually reproduced". Release group/attempt/terminal expectations and
restart freshness also cannot be closed by a scalar replacement alone, without a new wire contract and tests.

The document gates are tracked73/all79 clean, self-tests12/12, cargo document gate1/1. private-header81 clean,
common0 header/5 source, and current0eadefebd manifest valid24 were also rerun. This is not full preparation, semantic compatibility of a new pin,
or native model re-verification. After the document change, the source373 seal and the default diff check are rechecked.

## 2026-09-07 follow-up — SESSION authority and the release ACK sender boundary

HEAD `a9e1967fc` + uncommitted working tree. The internal ACK source, left as an out-of-original RED in the previous section, was fixed.
SESSION v4 semantics and constraints are owned solely by the batching contract. The P4 neutral protocol/broker and native/llama/backend production code were
not changed in this slice. The staged Cargo dependency on agent-core is a dev-dependency for broker tests.

### Real consumption and the normal paths kept

- `worker/release_tests.rs`: after installing a real SESSION, goes through codec→EventBroker::dispatch→Worker::handle and compares
  the normal terminal, the next middle, a Node outside the pipeline, a stale terminal node generation, and a same-named terminal from another
  agent. On rejection, pending/slot/reserved waiters/Verify fence/effects are preserved, and resending the same
  body from the normal terminal succeeds. Including the 3 existing release/admission tests, **8/8**.
- `worker/loop_tests.rs`: keeps the existing ordinary 2/4/8 and speculative 2/4 token/text/position/stop and per-stage
  native KV/release oracles. A new 3-stage test injects a fresh-ID ACK from middle while the real ACKs are
  held back. The 9th request, waiting behind 8 requests, was issued to native early under the old code;
  after the fix, the native state is preserved. After normal ACKs resume, all 9 complete normally. **13/13**.
  The 15 shared OUTPUT JSON entries and the prefill expectations were not modified.
- `worker/session_tests.rs`: checks the three installed roles and a repeat of the same declaration, malformed/rebind before and after installation,
  and local endpoint/index/target and old wire. Negative cases before installation are included too, so that the post-install immutable comparison does not hide
  a missing check on the first declaration. For the six stage message families, wrong codec inputs for source and target each are fed into
  the real handler to confirm the exact route rejection. **5/5**. This matrix on an empty state is not counted as a native effect test.
- The valid metadata of the earlier 5 fixture files was aligned with the new SESSION. The body/ledger/uncertainty assertions of the existing worker27/incarnation3/turn9/
  physical-replay28/stage20 = **87 tests** were kept. The fixture wrapper that only aligns target to the real receiver
  does not correct source, and the new explicit tests are responsible for the wrong-target proof.
- OUTER `session_events` is the production function that execute actually uses. The three-node order from independent constants and each
  target/index, the v4 JSON and the existing Sender sequence are pinned. The reverse-order ACK positive and the source/causation negatives
  for the real ExpectedReply/receive_exact use a bounded in-memory EventWire. New **3/3**,
  event-drive overall **50/50**. These tests do not run a real TCP CREATE/LOAD bootstrap.

### Old-code counterexample and independent mutations

1. `target/release-source-authority-red-20260907-01/verification.md`: only the new actual run test was moved into an independent old source
   identical to the previous 372-file seal, with only the SESSION literal adjusted to the old schema. fresh compile
   26.85 s, **12 PASS/1 RED**. `waiting_started=true`, head logical2→3; the current original gives false/2→2.
   Old RED EXE `80AFD2034067AF7BE27F618BB8ED8764231F349E961CD610AA9D8441E389FCBA`.
   The RED copy differed only in one test file, and the originally preserved old source was not changed.
2. `target/release-source-guard-mutations-20260907/verification.json`: results before formatting and the final byte results are
   kept apart. The **final-** 5 arms: baseline8/0 → guard allows all 4/4 → restore8/0 → denies all 2/6 → restore8/0.
   Fresh Compiling took 6.84/6.76/4.27/3.99/4.00 s respectively. Since both wrong sources and valid retries are checked,
   a permanent rejection also fails. final-restored-deny EXE
   `e05eb7d0c0b7f00df55385cc4cf76e69ca74ad8cc3fff06e11784d17d1d7eb4d`.
   Of the 243 inputs, the 239 files corresponding to the repository are final original = restore, and the original was unchanged during the final arms.
   The independent reduced workspace/lock and the original declarations/lock were preserved separately, and the shared registry package checksums were compared.
3. `target/session-v4-builder-mutations-20260907-01/verification.txt`: lowering the real producer argument to v3
   gives **2/1**, changing it to a head/tail array without middle gives **2/1**, and the exact restore gives **3/0**.
   The 154 selected inputs and the executed EXE were sealed, and an actual compile was confirmed for every arm. final-restored EXE
   `1DB93BA9FBD2E46B9C9C1AC94201447BB170E3F85B104FE0097E3B77F2F5CDA3`.
   This is a regression test for v4 production that was already in effect before the extraction, not a claim that the builder extraction itself fixed an old-code defect.

All mutations ran only in independent copies. The initial failures of the new SESSION tests in the current original were a missing
`tcp://` in the test address and a missing tensor in a non-terminal capsule. They were fixed with fixtures in the real wire format,
without weakening the validator. Those compile/failure runs are not added to the success count.

### Final sources, tally and exclusions

`target/session-authority-source.json`, `session-authority-seal.mjs --verify`: Rust/Cargo and the two literal
JSON fixtures, **374 files**, SHA256
`2af41250c3a3b1b05604fd7c1be7363f6f20bd3816b88613785105b32d4e1d0f`.
Documents and compiler/registry package sources are outside this seal. The earlier 373 seal file was not overwritten.

- `target/session-authority-workspace.log`: `cargo test --workspace --no-fail-fast` final exit0,
  **57 summaries, 1090 passed / 0 failed / 7 ignored**. Previous1076 + SESSION5 + release5 + actual loop1
  + OUTER builder3. The 87 migrated fixtures are not added to the new test count.
- `target/session-authority-js.log`: harness57 + build wiring6 = **63 passed / 0 failed**, skipped0.
- Document changes after the source freeze are checked by the separate lint/cargo document gates. The strings and indexes they check are not the same as execution semantics.

The actual Worker::run used a post-LOAD fake native Frame. Real model load, CPU/CUDA conformance,
network authentication, GPU/VRAM-only/RAM offloading, performance/multi-computer runs, deployment and commit/push were not run in this slice.
The target material is preserved locally and is not a published immutable evidence bundle.

The request set of a scalar RELEASED, release routing and observation for multiple OUTERs, preservation of the notification intent after commit,
freshness across a new OUTER/Worker restart, and fleet-wide topology agreement are unresolved. The SESSION source check is not promoted
to full release completion or to authentication. Subsequent actions and stage status are owned only by the latest roadmap.

Separate read-only audit: `event_runtime/transport.rs::serve` and `EventBroker::dispatch` do not bind the peer identity to
the envelope source through authentication. The first SESSION installation does not bind the configurer's authority, and
SESSION_READY does not attest the full ordered topology. These three are code-only open boundaries,
not effect tests of the source/target counterexamples above or reproductions of a network intrusion. Responsibilities and constraints follow the isolation contract.

The final document gates are tracked73/all79 clean, self-tests12/12, cargo docs_lint1/1. The first lint failure, which detected mixed EOL
during editing, was fixed by CRLF normalization of the same six documents. private-header81 clean/common0 header,
5 source, and current pin0eadefebd manifest valid24 were rechecked. `target/session-authority-clippy.log` shows
staged/event-drive all-targets exit0, but warnings such as staged lib-test26 (13 duplicates) remain.
This is not a new pin replay or proof of native semantic compatibility. After the document update, the source374 seal is again confirmed unchanged.

## 2026-09-07 follow-up — per-request release proof and owner notification

### Sources and real execution boundaries

HEAD is a9e1967fc59dffa6c2e458f1b91f916b1df826c1, and a later uncommitted working tree was verified. The new
`completion.rs` and `worker/release.rs`/`effects.rs`/`node/state.rs`, plus OUTER `run/inference.rs`/
`release_ledger.rs`, are the real consumer paths. Wire semantics are owned only by the batching contract. Native C++/llama/backend and
the P4 neutral protocol were not changed in this slice. The existing SESSION source authority tests are kept as well.

In the actual `Worker::run` with post-LOAD fake native, a normal single OUTER and A/B from different OUTERs were mixed into one
physical terminal. The expected values are checked separately against the original PREFILL's sender ID/full route, the
slot/incarnation of the PHYSICAL issued by head, head's RELEASE command, and the raw P4ID received by the fake native. The received
receipt is not copied and used as its own expected value. At 2/4 stages, the OUTPUT/receipt owner, correlation/
deadline, the receipt order after terminal, event uniqueness and head sequence progress are checked. The first and subsequent Full go through the
real worker loop and mailbox, and the original 1 native release and normal tokens/text/position/stop are also kept.
The actual loop is **16/16** (existing13 + new3). No real network, model, tokenization or GPU was run.

The **7/7** for direct `Worker::released` + the real completion mailbox check Closed after the first/1 item, MAX/MAX-1 event
numbers, Full recovery, different correlation/deadline from the same OUTER, and bad provenance later in the list.
For **8 kinds of valid-value mismatch × A/B in both orders** of a wrong origin/ReplySpec, slots, pending, effects and native calls
are preserved. The scope of the direct handler/method tests and the actual run above are not merged and called one full path.
A separate DTO4 and real PREFILL source/target consumption1 were added, making SESSION overall 6/6.

The new `head-approved-output-v2.json` captures together the **real PREFILL5, OUTPUT15 and
receipt5** of ordinary2 and checkpoint Replay2/4. The old `head-approved-output-v1.json` was preserved, and the legacy semantic projection
checks that the existing full tokens/text/position/stop/route are the same. The new projection compares the entire
body including the new fields. Capture mode does not skip the checks, so the first migration was intentionally RED against the empty new fixture.
Real consumption goes through EventWire→drive→acceptance over a bounded duplex. The envelope of the real OUTER send is compared with
the captured PREFILL, and the raw OUTPUT/receipt bytes are not edited afterwards. However, the worker input is explicit
tokens and the OUTER input is a prompt, so **this is not tokenization equivalence**. The observations around the capture remain
synthetic, and they are not used to hide that real multi-OUTER observation production is unresolved.

The full OUTER **67/67** is existing50 + new17. It includes actual drive budget/boundary/member27, pure release
ledger2, real send_wave failure/registration order1, and shared capture consumption5. The member/approval bool
comparison in the final artifact is a consistency check of preserved results, not independent authentication/replay of raw receipts.

### Counterexamples and independent mutations

| Material | Pinned tests and results | Limits of interpretation |
| --- | --- | --- |
| `target/release-notification-provenance-red-20260907/` | Before the fix, a different valid ingress was accepted: 0/1 RED after an actual fresh compile. Original, test and EXE preserved | A RED that stopped at the first wrong value; not an independent old-code run for each of the 8 axes |
| `target/release-receipt-producer-mutations-20260907/verification.json` | baseline13/0 → provenance removed12/1 → restore13/0 → failed front deleted10/3 → restore13/0 → PREFILL source/target removed12/1 → restore13/0 | notification7+SESSION6. Method/handler proof; not the full actual run/native KV |
| `target/release-notification-producer-mutations-20260907-01/verification.md` | actual run16/0 → collapsed to one owner group14/2 → restore16/0 → notify the ACK route14/2 → restore16/0 → terminal operation tampered7/9 → restore16/0 | The route mutation fails in the first 2-stage/first Full sub-case. No additional claim of separate mutation runs for 4-stage/subsequent Full |
| `target/outer-release-membership-mutations-20260907-01/verification.txt` | 67/0 → terminal member check removed61/6 → duplicate recount64/3 → partial commit66/1 → OUTPUT attempt check removed65/2 → send first66/1 → exact restore67/0 | The 1 refutation of state invariance under partial commit is the pure ledger. It does not observe the actual drive's private ledger after the error |

All mutations were run in independent copies. Each arm recorded actual Compiling, source before/after, and EXE/raw log hashes,
and a simple copied mtime or reuse of the original process's shared EXE was not counted as a fresh result. The source closures are
247 inputs/243 original (+ isolated workspace/lock), 379 inputs and 167 inputs respectively, with exact restore and an unchanged original confirmed.
The dependency closure of the reduced workspace is kept apart from full workspace tests. Each mutation starts from the baseline, and
expectations and test bodies were not weakened. A faulty implementation failing is not the same as mutating every predicate of the guard
one by one. The current grouped provenance/PREFILL mutations fail at the first negative case.

Final restored EXE SHA256:

- method/handler: `0c169ad85cd6ae5cb17ef71953f839eb93d6cd840a758b3ea07a9e8f8fe55525`
- actual worker: `E100B55F026F25E9A1D2EAA644811C27B18186A2EF03449EADF756E261A0B3F3`
- actual OUTER: `BDF5237888734C843E3FB194EE53DB25AAF463210B14025FB6D77D1FA77D0611`

### Failed intermediate runs and tally

- Valid PREFILLs whose old fixture source differed from return_route were rejected by the new check, so the initial full adapter run was
  **354/23**. Only the valid input generation in `stage_tests`20 and `incarnation_tests`3 was fixed; the negative inputs and the
  native/body/slot/KV assertions were not changed. The existing 23 are not added to the new test count.
- The stale `completion_queue_full:waiting` snapshot in the actual Full test did not prove the next saturation.
  The test was changed to wait for a new Full after a test-only observation baseline taken at the point where the recv wait before the genuine ACK and an Empty mailbox
  were confirmed. This does not change request/ledger/native state. The initial failure log was preserved.
- Attempts whose compile failed while the files using the new capture were being written in parallel — missing module, function arguments not migrated,
  Envelope serialization test error — are not PASS. Envelope was not changed into a P4 Serialize type for test convenience.
- An attempt in which another job relinked the EXE in the shared build directory, so that the EXE hash right after a producer run
  could not be pinned, is recorded separately. Only the EXEs of the independent final runs above are bound exactly.

`target/release-receipt-source.json` and `release-receipt-seal.mjs --verify`: Rust/Cargo and **the three literal
JSON fixtures, 379 files**, SHA256 `604a008d2e8b66bcf746494802fbe2382d56b133c1451271da7de84615a62dc2`.
The earlier source374 file was not overwritten. Documents, JS and compiler/registry package sources are outside this seal.

- `target/release-receipt-workspace.log`: full `cargo test --workspace --no-fail-fast` final exit0,
  **57 summaries / 1122 passed / 0 failed / 7 ignored**. That is 1090 + adapter15 + event-drive17.
- `target/release-receipt-js-final.log`: harness57+build wiring6+event config3+four-node config9 =
  **75/0**, skipped0. The first run of the widened scope gave 66/1 because the old `apps/p4/` import path was missing. The actual module in the same repository
  was located and only one test import line was changed. The results of the earlier selected scope of 63 are not rewritten as a past pass of 75.
- `target/release-receipt-clippy.log`: staged/event-drive all-targets exit0. Warnings remain, including staged lib14/lib-test26
  (13 duplicates) and event-drive bin6/test8 (6 duplicates). This is not a warning-free claim.
- private-header81 clean/common0 header, 5 source, and current pin0eadefebd manifest valid24 were rechecked.
  This is not a native build, a new pin replay, or CPU/CUDA semantic conformance.

With the new root source, C++/CUDA/model/GPU/remote network/deploy/commit/push were not run. The target evidence is
locally preserved material and not a published immutable bundle. VRAM-only sufficiency and the later RAM offloading remain real-hardware gates.
The proof of the current normal shutdown is not extended to Cancel without output, multi-OUTER observation, restart freshness, reconnect/durable delivery,
fully bounded queues/credit, or graceful drain. Stage status and the first next action are owned only by the roadmap.

In the final document review, the capture's "all fields" wording was narrowed to the whole payload. This distinguishes the exclusion of the 3 volatile envelope fields
from semantic equality (checked separately) from the exact check against the original PREFILL; it does not weaken fixtures or expectations.
The document gates are tracked73/all79 clean, self-tests12/12, cargo docs_lint1/1. After EOL normalization of the documents and the JS import,
JS75/0 was rerun, and the Rust379 source seal was confirmed unchanged again.

## 2026-09-07 follow-up — separating report metrics and auditing observation completeness

### Scope run this time

`test/benchmarks/p4-4node/run.mjs::buildReport` is the artifact consumer path shared by the real main and the model-less report tests.
The existing inline report assembly was moved there, and main no longer starts resources on import.
The direct CLI's no-argument usage failure is also tested separately. This does not mean these tests exercise model/worker/native/network
execution. This is an uncommitted tree at the same HEAD; the production change this time is run.mjs, and the new test is
report-metrics.test.mjs. Rust/native/C++ and the P4 neutral core were not changed in this slice.

report metrics v2 is aligned with the rule by which Rust acceptance counts preserved OUTPUTs. It distinguishes the earlier decode row rate from
the approved generated token rate; the exact fields, empty EOS, denominators and legacy migration are described in the harness README.
Prefill+Verify/Replay is also counted as mixed. Physical rows/fill/pacing are the original values, and earlier report files were not
modified. This metric is not H1 quality approval, nor H4's last-terminal time window / useful TPS. The computation, ownership and completeness of Rust per-request
logical_generation_tps and stage spans have not been migrated.

### Old-code counterexample and mutations

`target/report-token-metrics-20260907-01/verification.txt` and the raw output/sources/metadata in the same directory:

| Run | Result | Scope |
| --- | --- | --- |
| `01-before-fix.stdout.log` | 0 passed / 2 failed, exit1 | Consumes report assembly with the original formulas unchanged: mixed0≠2; speculative output3 in 2 s, yet generation TPS0≠1.5 |
| `03-complete-tests.stdout.log` | 11/0, exit0 | Final original regressions, including EOS/empty fragment/first token/denominator/physical width/missing evidence/CLI |
| `04-copy-baseline.stdout.log` | 11/0, exit0 | Independent copy with the real import closure of 10 files |
| `05-copy-decode-numerator-mutant.stdout.log` | 6/5, exit1 | Only the production generation numerator reverted from generated→decode; test bytes unchanged |
| `06-copy-restored.stdout.log` | 11/0, exit0 | Only the copy restored exactly; 10/10 matching the original |

Before the old-code RED, only the function extraction and import guard for report assembly existed; the wrong formulas were unchanged.
The intermediate 8 GREEN are not reported as the final tally of 11. The mutation is evidence that JavaScript was interpreted in a fresh Node process each time,
not a compile or EXE rebuild. All three copy runs confirmed before/after for the 10 input files and a change of 0 in the original's
before/after. No original checkout/reset was used. Only 1 generation-numerator mutation was
run this time, and we do not claim an independent mutation that separately removes the mixed predicate.

- Final run.mjs SHA256: `A7685DAB587992F18DDF6C7CDAD5137E566562041297B9B9720CFE8D2E51837E`
- Final test SHA256: `E7B71CD85E8179C556721BE0D31915630D993BB74FD1EE5BF9DE31F7F9BFADAB`
- verification.txt SHA256: `FAC6E72AE330673680AFA0584CFDF02BA466ECDD9B96648B13FD3B9D5993E1CD`
- Node v26.4.0 SHA256: `3193D7F751B8A07BD4ACC70E81946AE9C6EFDEE83E07AD1C8D0E4089DF7C5CEF`

### Final rerun and seal

- `target/report-metrics-workspace.log`: `cargo test --workspace --no-fail-fast` final exit0,
  **57 summaries /1122 passed /0 failed /7 ignored**. No new Rust tests were added. The earlier Rust379 input
  matches the SHA from `release-receipt-seal.mjs --verify`, `604a008d2e8b66bcf746494802fbe2382d56b133c1451271da7de84615a62dc2`.
- `target/report-metrics-js-final.log`: harness68+build wiring6+event config3+four-node config9 =
  **86/0**, skipped0. This is the selected scope of the existing 75 plus the new report11, not every JavaScript test in the repository.
- `target/report-metrics-source.json`: the harness .mjs files and selected build/config inputs, **25 files**,
  SHA `41dbff05c72d143191348766d6802924611e36fdc6f1006ec9a5240f1d0a26b8`. This is not a hermetic seal of
  Node/toolchain/OS/models. The first attempt with the seal tool failed because of a mistyped event-config path; it was fixed to the real
  p4-event-gate/config path and then the seal above was made. That failure is not included in the test GREEN.
- Documents tracked73/all79 clean, self-tests12/12. After the final document EOL cleanup, cargo docs_lint1/1 was also rerun separately.
  The string gates do not prove the semantics of the unimplemented observation contract.

### Additional observation boundary audit — what was not implemented

The full owner copy in `observe.rs::emit_batch_observation`, the first-owner send in `emit_stage_span`, and
the exit right after terminal/receipt in `inference.rs::drive` were reread. If some observations/spans are lost as a whole batch,
a check that requires coverage only for the executions received cannot know the expected set itself. Substituting a different execution/position
while keeping only the count/phase sums is a separate problem. This is a **code-only audit**, not a new execution RED.
Fixed-size per-request evidence at issue approval and its binding to terminal, owner projection, deferred collection, failure atomicity and cost counterexamples are
recorded as goals in the batching contract / verification protocol. SHA dependencies, a new OUTPUT, observation wire and the completion barrier are not implemented yet.

The existing user order of VRAM-only → larger RAM offloading was also kept. H0 now makes clear that even deliberate CPU computation
is not VRAM-only if the real model computation/weights/KV depend on the host. CPU tokenizer/sampler,
staging, mmap and the marking of non-owned layers are not mistaken for model offload. The existing runner's actual RAM
layout/budget is still unimplemented, and this time there was no model inventory/load, GPU, offloading, remote deploy or commit/push.
The current stage and the first next action are owned only by the latest observation audit section of the roadmap.

## 2026-09-07 follow-up — internal witness of actually approved issues

### First seal and production path

On the uncommitted working tree at the same HEAD a9e1967fc, internal issue evidence was wired into `issue_witness.rs` and `accept_prepared_issue`.
The exact encoding, layer attribution and exclusions follow the internal issued-work v1 section of the batching contract.
The P4 neutral core, native stage ABI and llama/backend are not part of this production change. The witness is
evidence of approved membership, not proof of token semantics, durability, signatures, observation completeness or a KV stop point.

The first full original run, `target/issue-witness-workspace.log`, gave final exit0, **57 summaries /1146 passed /
0 failed /7 ignored**. That is the previous1122 plus primitive12 + real L1 API9 + real worker loop3.
The SHA of the 385 input files in `target/issue-witness-source.json` is
`d8189658968bae94ec4181a33dba4eeb8b95cd8b8a9ce27fd1173cb3d000d94d`. It includes Rust/Cargo, the existing3 JSON fixtures,
the independent vector JSON and the generator script. Compiler/registry sources, documents and models are outside the seal.
This is the source before the submission preflight fix below, and it is not reused as a full pass of the final source.

The primitive tests compare against the bytes/digests of the 5 input kinds and 13 chain steps declared by `generate_vectors.mjs` without Rust.
The L1 tests go through the real prepare/begin/approve APIs and the committed, prepared and flight snapshots. The actual run tests go through
post-LOAD fake native, the real Frame/Capsule/EventWire, and Worker::run. They include ordinary 2/4/8-stage,
multi-OUTER 2/4-stage, a real completion Full, a duplicate terminal under a new EventID, and a second native failure / ID reuse.
They check that the last approved witness is preserved even when native fails after actually touching KV.
Besides the independent digest, the existing output token/text/position/stop and KV/release oracles are kept.

### Mutations that did not touch the original

Preserved in `target/issued-work-mutations-20260907-01/verification.json` and each arm's raw output/manifest/source/EXE.
The selected tests are primitive12 + L1 API9 + actual loop19 = **40**. This does not mean all 19 are new tests.

| arm | Result | Change and detection |
| --- | --- | --- |
| baseline | 40/0, exit0 | Independent copy identical to the 385 original files |
| omitted-witness | 35/5, exit101 | Only the witness installation after approval set to None. 2 L1 and 3 actual worker tests fail |
| restored-omission | 40/0, exit0 | Copy restored to the original bytes |
| premature-commit | 35/5, exit101 | The current request's witness changed before the lower-priority validation/flight registration. The rejection-invariance tests fail |
| missing-execution-identity | 33/7, exit101 | Execution ID removed only from the hash input. The independent literal and the real approval/worker comparisons fail |
| restored-final | 40/0, exit0 | Both production files identical to the 385 original files |

Every arm confirmed **Compiling→new EXE** of the real staged crate, with input before/after change0 and
original before/after change0. On restore, too, only the copy's crate-root mtime was updated so the previous EXE could not
be reused. There was no original checkout/reset, no mutation of the original production code and no golden edits. Each arm's exact EXE/hash is preserved in the manifest,
and the EXE was copied after each run so the next arm could not overwrite it. Original sources/EXEs from a different point in time are not claimed to be
the same run. The first source seal was reconfirmed as identical after the last mutation as well.

This evidence does not cover detection of missing observations across the OUTPUT wire / actual OUTER. At the time the getters were
consumed only by tests, so production dead_code warnings remained. The warnings were not hidden, and completion of the downstream wire was not claimed.

### An entry mismatch revealed by a further code audit

The P4 envelope/wire allows a submission event ID and OUTER channel/host containing NUL, but the approved OUTPUT and
the internal witness do not. If `Worker::prefill` does not check this, the request can be rejected at the first approval after native.
This is an admission/downstream contract mismatch found by an independent code audit, and it is separate from the limitation that the hash evidence does not
provide network authentication. The later real counterexample and fix, and the final source rerun, are kept apart in the records below.

### Submission preflight RED → GREEN

The first original run in `target/submission-identity-red-20260907-01/verification.md` and `red.log` gave
**1 passed /6 failed**. Explicit tokens and a prompt were used for each of NUL in the event ID, channel and ingress host.
After a round trip through the original EventWire, head had already made native logical call1, written KV, and created slot0/incarnation1 and the
session_key that would be rejected, ending in Uncertain and worker shutdown. The 3 prompt variants also had Tokenize1.
RED raw SHA `FF5AB0D25B3AD80BF99C247C120EAFF1A18541EAF15BAE930C1E62D9C92DA08A`, preserved EXE SHA
`68E666889636069371FD7CD71B6C94EC550A61EE0A77B29EE617208A6E05E7A6`. The before/after hashes of the 4 files of interest in this run
and the exact executable are in the agent evidence, and this is not extended to a seal of all workspace inputs.

The common `validate_submission_identity` is now called right after PREFILL's original source/target check.
The authority check for settlement reuses the same common format. No new slot/incarnation or witness is created
with fake values first. The canonical bytes/digest goldens were not changed. After the fix, a bad request returns the exact adapter error without Tokenize,
native or ledger effects. Retrying the same request_id as a valid submission with a different session_key
completes with slot0/incarnation1 and the existing exact output/KV/release. The valid Unicode case and a separate
correlation positive are also kept. While writing the tests, the OUTER sequence of the valid resubmission was set explicitly to2, one unnecessary clone in the new witness
fixture was removed, and then the final full source below was sealed again.

### Independent mutations of the final source and full rerun

`target/issue-witness-final-source.json` is **386 files**,
SHA `ac955f7e2ce1ca7f5c73748ea7835a96a10730f430850d57faf3e46ec1cb02a2`. The first 385 seal was not overwritten.
Everything was recompiled in the final copy for `target/issued-work-mutations-20260907-02/verification.json`.
The selection is primitive12 + L1 API9 + actual loop26 = **47**, and the test/golden bytes are fixed across all arms.

| Final arm | Result | Detection |
| --- | --- | --- |
| baseline | 47/0, exit0 | Identical to the 386 original files |
| omitted-witness | 42/5, exit101 | L1 2 + real worker 3 |
| premature-commit | 42/5, exit101 | State preservation on lower-priority error/overflow/registration failure |
| missing-execution-identity | 40/7, exit101 | Golden, equal-count substitution, real approval/worker digest |
| missing-submission-guard | 41/6, exit101 | The 6 NUL variants of the real PREFILL regress to failure after native |
| restored-final | 47/0, exit0 | copy386 files identical to the original, Compiling again with a new EXE |

For every arm, input before/after and original before/after change0, the run logs and the preserved EXE hashes were verified.
Final restored EXE SHA `dc9020d7ef5e117320bc9b50b712953f0fbbd7ab21876fe465b2228aa7416f86`.
This EXE is the selected 47 tests of the independent copy and does not stand in for every test binary of the original full workspace.
No additional mutation removing the whole common check was done, and removal of the prefill consumer call is kept apart from input membership mutations.

- `target/issue-witness-final-workspace.log`: `cargo test --workspace --no-fail-fast` final exit0,
  **57 summaries /1153 passed /0 failed /7 ignored**. That is the previous1122 + internal witness24 + entry7.
- `target/issue-witness-js-final.log`: harness68+build wiring6+event config3+four-node config9 =
  **86/0**, skipped0. The 25 input files are identical to the earlier `report-metrics-source.json`
  `41dbff05c72d143191348766d6802924611e36fdc6f1006ec9a5240f1d0a26b8`.
- `target/issue-witness-vectors-verified.json`: the Node independent generator output and the literal JSON have identical structure,
  authority5/chain13. Node26.4.0 and the generator/literal hashes are recorded too. The goldens were not regenerated from Rust results.
- `target/issue-witness-final-clippy.log`: staged/event-drive all-targets exit0. Warnings remain: staged lib15/lib-test26
  (12 duplicates), drive bin6/test8 (6 duplicates). The production-unused warnings for the 4 witness getters
  remain because OUTPUT is not migrated; this is not a warning-free claim.
- private-header81 clean/common0 header, 5 source were rechecked. This is an include string gate, not proof of native
  recompilation, link closure, upstream semantic compatibility or backend conformance.

The final Rust386 and JS25 seals were identical after the mutations. Document edits are outside those code seals. The final document gates are
tracked73/all79 files clean and self-tests12/12, and `cargo test -p p4-agent --test docs_lint` is also 1/1.
The raw output is preserved in `target/issue-witness-docs-{tracked,all,self}.log` and `target/issue-witness-final-cargo-docs.log`.
The target files are locally preserved evidence, not an immutable deployment bundle. With the new sources, C++/CUDA,
model load, remote network, heavy GPU waves, VRAM-only/RAM offloading, deploy and commit/push were not run.

### Boundaries still open

The internal witness does not go out in OUTPUT yet. The wire, owner projection and effect fan-out by which the actual OUTER checks late/missing observations for completeness
are not migrated either, and this GREEN does not approve them. The earlier full RequestState
prompt clone and the cost of ReplySpec parse / authority rehash / current row sorting on every issue remain separately.
The missing serialized ReplySpec>4096 preflight found by a further read-only audit is an open surface distinct from this NUL counterexample.
We do not claim that this maximum-length counterexample was run. The first next action and the order after it are owned only by the latest roadmap section.

## 2026-09-07 follow-up — the real byte boundary of submission strings

The code-only ReplySpec size surface from the previous section was reproduced this time with a real Worker::run and fixed. HEAD is
`a9e1967fc59dffa6c2e458f1b91f916b1df826c1`, with an uncommitted working tree. No commit/push of the original or remote deploy was done.
The current input limits are owned solely by the submission boundary of the batching contract, and the next action / stage status follow the latest roadmap section.

### Cause of the counterexample and scope of the fix

`worker.rs::prefill` remembered the session key and accepted the request without checking the ReplySpec/options limits.
When `LogicalBatch::encode` later returned InvalidRow, the result was a worker shutdown, not a request rejection.
Prompt input had already run Tokenize. In this counterexample, encode fails before logical/native KV execution, so
we do not claim KV was written. This is a different path from the previous NUL counterexample's post-native Uncertain.

The shared `capsule.rs::validate_reply_options` is applied to the actual serialized ReplySpec and the raw options.
Logical/Physical keep the same check, and the original 4096-byte limit and the other identity/position/speculative checks were
not changed. The PREFILL call comes before record/remember_session_key, Tokenize and slot/incarnation acceptance.
The JSON semantics of options are not reimplemented in Rust, and options are not stripped or reserialized. There are no generic P4 or llama/backend changes.

- `submission_limits.rs`: 16 valid and 8 over-limit cases within 12 libtests. The valid inputs of 4095/4096 cover ASCII, escapes, UTF-8, and
  tokens/prompt, and complete with the existing exact token/text/position/stop, KV and release oracles.
- An over-limit 4097 is rejected with explicit error1 before native/tokenize/recording/acceptance, and resubmitting the same request ID with a different session key
  to the original worker completes normally. incarnation1/slot0 is not polluted.
- In an independent literal ReplySpec, an escaped correlation of 1982bytes raw becomes reply4097, and a Unicode
  correlation of 3962bytes/1322scalars becomes reply4097. External JSON payloads of 4276/4326bytes carrying options4096 are
  valid. Substituting checks on external payload size, character count or trimmed strings is forbidden.
- `logical.rs::reply_and_options_wire_limits_preserve_exact_utf8_bytes` is a separate codec regression1.
  It is not proof of a real model tokenizer/options parser, a global admission budget, or atomicity under other failures.

### Sealing the pre-fix run and correcting an error

The first run in `target/submission-limits-20260907-01/red.log` gave 4P/8F, but at the same time root was adding the logical codec regression,
so source before/after differed. It is preserved only as an **unsealed historical observation** and was not used as definitive pre-fix evidence.
After editing stopped, `sealed-red.log` reproduced 4P/8F again after an actual 5.44s recompile. The 7 files of interest were identical
before/after. This is a seal of that source list, not of the whole workspace.

- sealed raw SHA256 `d23d1b10bdc9efc9dbee8cc9920593b473f8da206489f057e2238e69565a74e9`
- sealed RED EXE SHA256 `00c5fcc34c4826c23e41c7a14dfb86d94e800349ea67e811e3cdae5529a70a2b`
- All 8 over-limit cases: `LLAMA_LOGICAL_BATCH_ENCODE_FAILED/InvalidRow`, requests1, session key registered, incarnation1/
  next2, shutdown1. The 4 prompt variants had Tokenize1; in every case logical/physical native0 and KV0.
- After the fix, `target/submission-limits-green.log`: actual compile 5.58s, entry12+codec1 =13/13.
  The later full seal, mutations and full suite reinforce this partial run.

### Final Rust source and independent mutations

`node target/submission-limits-proof.mjs seal` copied and sealed Rust/Cargo, adapter fixtures and the independent issue vectors
as **387 files**. The SHA256 of `target/submission-limits-mutations-20260907-01/source-baseline.json`
is `5a60dbd09c9af3f508b0294670e640170098774ca8d309646983055a54219c38`.
Docs, C++, compiler/registry, models and the JS harness are not included in this seal. The earlier 386 witness seal was preserved.

Each arm ran the full staged adapter suite of 421 tests in a **separate copy** via `node target/submission-limits-proof.mjs run <arm>`.
It compares original before/after and copy input before/after, and preserves per arm the actual Compiling, new EXE
mtime, raw log, input sources and EXE hash. No original checkout/reset or original mutation was done.

| arm | Result | What it refutes |
| --- | --- | --- |
| baseline |421/0, exit0|The sealed valid source|
| missing-ingress-guard |413/8, exit101|Removing the real PREFILL call fails even if the helper exists|
| late-ingress-guard |413/8, exit101|A check after session-key recording pollutes resubmission state even if it raises an explicit error|
| character-count |418/3, exit101|A wrong fix that counts chars instead of UTF-8 bytes|
| exclusive-boundary |416/5, exit101|An over-fix that also rejects exactly 4096|
| restored-final |421/0, exit0|All 387 source bytes restored and matching|

`node target/submission-limits-proof.mjs verify` verified the preserved inputs/logs/EXEs and the expected exits.
`check-source` also confirmed the same 387. Tests and goldens were not changed to accommodate mutations.

### Real C++ codec boundary — no model

The only original change is `runtime/physical_wire_test.cpp`. The final test SHA256 is
`395d6293f9bb90d4492b0f836a54c8f99a404fbc8304c9e458c984c3de3ed921`. The existing assertions were kept and
**12 cases** of reply/options × ASCII/3-byte UTF-8 × 4095/4096/4097 were added. The LB/PB literal inputs are
built without the production encoder, and the limits and exact bytes of the two real decoders and the PB encoder are checked.
Empty options are allowed too. This independently tests the same nominal boundary in Rust and C++; it is not called
full cross-language conformance that replays the same external fixture directly on both sides.

With MSVC14.44.35207, the **4 TUs** test/physical_wire_decode/physical_wire_encode/physical_authority were
freshly compiled for each arm. `/std:c++17 /EHsc /O2 /UNDEBUG /MD`; only the public llama/ggml headers are needed, and
no llama library, model or GPU was used. The 152 source/header files from the actual MSVC dependencies, the compiler,
commands, EXE and before/after inputs are preserved. This did not run a CMake target, a full build or CTest.

- `target/native-row-string-boundaries-final-20260907/verification.{json,md}`: baseline0 → false assert3 in a separate
  copy → exact restore0. This confirms that Release asserts actually run. The first mixed-EOL version in
  `target/native-row-string-boundaries-20260907` is separate historical material and was not used in place of the final source evidence.
- `target/native-row-string-limit-drift-20260907/verification.{json,md}`: baseline0 → changing only
  `kMaxString=4097` in the copy makes the real LB decoder boundary assert fail (exit3221226505) → restore0.
  It stops at the first assertion, so we do not claim this mutation proves independent failure of every PB branch.
- Final restored native drift EXE SHA256
  `b8b6c792835bc061a5d7dd38e1194f1d1cd9b39f3188836199a715cac12150b5`.

### Final tally and scope

- `target/submission-limits-workspace.log`: `cargo test --workspace --no-fail-fast` final exit0,
  **57 summaries /1166 passed /0 failed /7 ignored** = previous1153 + this13. Run completion was confirmed.
- `target/submission-limits-js.log`: harness68+build wiring6+event config3+four-node config9 = **86/0**, skipped0.
  The 25 input files are identical to the existing JS seal `41dbff05c72d143191348766d6802924611e36fdc6f1006ec9a5240f1d0a26b8`.
- `target/submission-limits-clippy.log`: staged/event-drive all-targets exit0. Existing warnings such as staged lib15/lib-test26 and
  drive bin6/test8 remain; this is not a warning-free claim.

After the final document edit, the document gates are tracked73/all79 clean, self-tests12/12, cargo docs_lint1/1.
The raw output is preserved in `target/submission-limits-docs-{tracked,all,self,cargo}.log`. The private-header gate is
81 clean/common0 header, 5 source in `target/submission-limits-private-headers.log`, within the scope of an include string
check. The Rust387 and JS25 seals are identical at the end as well. The target material is locally preserved evidence, not a long-term deployment bundle.
OUTPUT v4, the observation wire, owner projection and completion conditions are still unchanged. Model tokenizer, native options semantics, CUDA/
engine CTest, real heavy GPU waves, VRAM-only/RAM offloading, multi-computer runs and deploy/commit/push were not run.

## 2026-09-07 follow-up — OUTPUT issue evidence and per-owner observation completeness

The target is the uncommitted working tree on HEAD `a9e1967fc59dffa6c2e458f1b91f916b1df826c1`. The internal
witness and input limits from the previous sections were kept. The implementation and tests below are limited to this source scope and do not
re-certify past GPU runs. The contract definition is owned by the batching contract, and the next stage / promotion status by the latest roadmap record.

### Implemented consumer boundaries and what was kept

- `completion.rs::ApprovedOutputPayload` validates the strict flat DTO of OUTPUT v5 and the terminal-only proof.
  `release.rs::tail` copies the actually approved witness before discarding RequestState and rechecks the original submission's
  authority digest. The witness is not built from a raw flight registration or a returned capsule.
- OBS v4/SPAN v4 in `commands.rs` and `observe.rs` bind the recipient with the full OuterEndpoint.
  Different correlations on the same route are bundled into one delivery, but the carrier uses the actual original ReplySpec.
  The exact rows, phase, position and issue index of the recipient's own requests are kept apart from the full physical size. Downstream
  does not invent a submission ID it does not have. Pre-native recipient validation and Fresh-only spans are kept.
- head builds the observation candidate before approval and checks it after approval against the real witness count/ordinal installed by
  `accept_prepared_issue`. The issue count is not increased just because an observation was sent.
- `effects.rs::flush_effects` pins the time once, right after putting Forward into the real completion mailbox, and
  preserves the following Telemetry intent. Full recovery, Closed and event number exhaustion were tested. This is the local delivery
  acceptance time; it does not add network arrival time, durable retransmission or automatic fence recovery.
- The real send path of `event-drive` registers `SubmittedAuthority`. OUTPUT checks the budget and release candidates,
  approves the evidence candidate, and then commits the request output / release expectations. Observations hash each newly contiguous per-request issue
  once, and compare the terminal evidence with the per-configured-stage execution membership and global size.
  Reversed arrival order and exact body redelivery are allowed, and conflicts are rejected before any of the candidate is applied.
- The real drive, if still Missing after terminal+release, waits within the original overall deadline. It also ends when the next
  wave's scheduled time is later than the deadline. `elapsed_ms` is latched at the release boundary, and
  `telemetry_complete_elapsed_ms` is kept separately. The new wait time is not added to the existing TPS denominator.
- RequestArtifact preserves the actual submission authority and terminal proof. Stage indexes are interpreted together with the
  config.json of the same run. Offline `acceptance::evaluate` does not independently rerun the online hash checks.
  Invalid/Missing EOF and timeout are Err/nonzero, and saving a partial artifact on failure is not implemented yet.

Only adapter-owned DTO/evidence types were made public. The generic P4 event/NodeAdapter, native opcode/capsule, C++ and llama/
backend were not changed in this slice. Policy was not given native private types, and the SHA evidence is not promoted
to authentication or proof of a KV stop point. Explicit Cancel/Drain and credit are not implemented.

### Real production capture and independent expected counts

The existing `head-approved-output-v1.json` (OUTPUT v3) and `v2.json` (OUTPUT v4) are unchanged. The new
`head-approved-output-v3.json` was built from the raw EventWire received in the actual Worker::run.
The existing approval fields, original submission, release and token/text/position/stop oracles are kept. Cross-version comparison projects only the new proof and
the explicit version, and the current v5 full payload is also compared with the new capture. Duplication/increase of real live event IDs and
causation presence are checked, but cross-run event ID/causation values, sequence and timing/pacing are not required to be equal.
So this is not extended into a separate proof that causation points exactly to one specific terminal event.

| Real case | Original submissions | OUTPUT | receipt | OBS | SPAN |
| --- | ---: | ---: | ---: | ---: | ---: |
| ordinary-2 |1|5|1|6|12|
| checkpoint-2 |2|5|2|5|10|
| checkpoint-4 |2|5|2|5|20|
| mixed-owner-2 |2|2|2|2|4|
| mixed-owner-4 |2|2|2|2|8|
| mixed-consumer-2 |2|2|2|2|4|
| mixed-consumer-4 |2|2|2|2|8|
| Total |13|23|13|24|66|

The expected counts in `loop_tests/observation_contract.rs` are a read-only record of **the real issue approval callback, not the received OBS**.
It records the successful native result capsule bytes, the original submission Event, the approval witness and the configured stages,
and then compares them against the separately received wire. The finish barrier is applied together with the old exact token/KV/release checks.
The existing independent Node literal digests are also kept for plain 2/4/8-stage and mixed-owner. This does not mean the hash algorithm
was independently reimplemented for every speculative scenario.

The existing mixed-owner test keeps its positive case in which correlation and request ID differ. The added mixed-consumer test
submits in the real event-drive Sender style from the start and delivers each route's **full captured Events** to two real drives.
Foreign owners are not erased from the body just before consumption to make it fit. The checkpoint partial request
uses logical ordinals 1/2/3/5 to distinguish count4 from last ordinal5. Checkpoint requests from the same OUTER are
separate issues, so whole Events can be selected for consumption; we do not claim this single-request capture consumption proves the case where several requests
from the same OUTER share one physical (production-side multi-request tests are separate).

These tests use scripted native tokens. They compare the envelope/config actually sent by event-drive with the capture,
but this does not prove that a prompt body different from the explicit token input tokenizes identically on a real model.
This is a regression connecting the actual worker's own command/capsule codecs and local routing with the actual drive's EventWire consumption,
not a full real-hardware run that connects a real broker, network and native model end to end.

### A completion error newly caught in review

Adding only execution999, `owned_requests=[]` and rows+1 to a valid first span still left the existing consumer candidate Complete.
`SpanRows` held back unknown1, but `ExecutionEvidence::missing` counted the empty-owned unknown as0,
so it dropped out of the completion condition. The actual drive approved normal output5, issue count6, completed1/released1 unchanged.

`target/owner-evidence-consumer-20260907-01/04-unresolved-global-red.log` preserves the real 0P/1F and the full result that was
wrongly returned as success. A received global execution is now Missing until head size is confirmed.
Unseen B-only spans are not required, and the positive case where the span arrives first and head confirms later is kept.
The extra foreign execution in that positive case is an **explicit metamorphic fixture**, not an unmodified native capture.
The old-missing mutation of the sealed source below reproduces this error again.

The other real consumer counterexamples are missing/revision/count/ordinal/authority/digest in the proof, swapped middle position/attempt/
issue index, per-stage omissions, width/owner/timestamp conflicts, legacy v4, and a carrier for a different request.
Deleting only the last decode OBS was separated from **deleting that OBS together with all related stage spans**.
In the latter, even though the received execution set disappears as well, the terminal expected counts remain, giving Missing requests1/stage0.
Normal reorder, exact replay, late telemetry and the mixed-owner positive cases were run together.

### Seal and execution scope

`node target/observation-proof.mjs seal` copied the workspace Rust/Cargo, shared captures and independent issue vectors as
**391 files**. The SHA256 of `target/observation-migration-20260907-01/source-baseline.json` is
`af4249da69e7447caade7182807b9a9bc970ce01b39a700ffd12280e3e5c75dc`. docs/C++/JS/compiler/registry/
models are outside this seal. Each arm preserves original before/after and independent copy before/after, the actual recompile, new EXE
mtime, source bytes, raw log and EXE hash. No original checkout/reset or original mutation was done.

`cargo test --workspace --no-fail-fast` in `target/observation-migration-workspace.log` gave final exit0,
**57 summaries /1196 passed /0 failed /7 ignored**. Compared with the previous1166, staged adapter15 and drive15 were added.
Old adapter/deployment tests were not hidden, and goldens/bounds were not lowered. Per-stage runs and mutation details follow below.

`target/observation-migration-js.log`: harness72+build wiring6+event config3+four-node config9 =
**90/0**, skipped0. The SHA256 of the separate JS25 inputs is
`039b98f651c5154823a37f2c9b9ea018f8cc1cc5efd1b0d45c937882f3703b3d`.
`target/observation-js-20260907-01/verification.json` preserves, in an independent copy, report baseline15/0 → allowing a conflicting
span14/1 → using telemetry as the TPS denominator14/1 → exact restore15/0. This verifies report
assembly, not the worker, model or network. Nor is it a mode that sums the projections of other OUTERs in the fleet.

clippy gives exit0 in `target/observation-migration-clippy.log`, but it is not warning-free. staged lib16/
test28, drive bin8/test9 and others remain. They include the `approve_output` convenience that production does not use and new type
complexity, and the source was not quietly cleaned up after the evidence was sealed. Cleanup will be re-verified with the next source change.

Final document checks and independent mutation checks follow the record below. Models/GPU, full CTest, remote deploy, heavy real waves,
VRAM-only/RAM offloading, multi-computer runs and commit/push were not run in this slice. Real tokenizer/KV values,
cross-host clock bounds, durable delivery, Cancel/Drain, restart freshness, bounded RSS and optimal batching performance remain.

### Mutations in an independent copy and exact restore

The crate in `node target/observation-proof.mjs run <label> [crate] [filter]` is the staged adapter or
event-drive. baseline/restore run the whole crate; the two terminal/approval full-loop arms use the
`b2_two_stage_run_loop_completes_one_request_through_real_event_and_native_codecs` filter, the production observation arm uses
`observe_tests`, and the consumer arms use `output_contract`. Tests excluded by the filter are not added to the passes.

| arm | Run passed/failed | Verdict scope |
| --- | ---: | --- |
| baseline-adapter |436/0|Whole staged adapter in the independent copy|
| baseline-drive |82/0|All real drive tests in the independent copy|
| terminal-proof-missing |0/1|Removing the terminal copy keeps the real run from completing normally|
| accepted-witness-missing |0/1|The read-only expected counts in the approval hook detect the missing witness installation|
| accepted-witness-missing-observe |6/2|The real head path without the read hook also fails with `accepted observation has no witness`|
| span-recipient-loss |3/5|Keeping only the first recipient fails the real fan-out/saturation preservation checks|
| telemetry-suffix-loss |3/5|Clearing the observation intent after a successful Forward fails the preservation/delivery checks|
| drive-stage-coverage-bypass |13/4|Treating missing stage evidence as complete fails the real consumer regressions|
| drive-digest-bypass |15/2|Removing the explicit conflict check turns Invalid into Missing and fails|
| drive-unresolved-global-bypass |16/1|Reproduces the existing counterexample that ignores an unconfirmed empty-owner execution|
| restored-adapter |436/0|New compile/EXE after restoring the 391 source files byte-for-byte|
| restored-drive |82/0|New compile/EXE with the same restored source|

The cargo exit for every RED is101, and for baseline/restore0. In the digest arm, the final equality check still remained, so it
ended in Missing/EOF rather than a false success; this is not reported as "approval of corruption after mutation". In the first missing-approval
arm, the test-only observer panics first, so the real production guard was also confirmed with the separate observe arm.
The fact that several gates overlap was not hidden, and nothing was refuted by also removing test-side safeguards.

`node target/observation-proof.mjs verify` confirmed exactly12 arms, the expected exits, actual run counts, failure counts, original
before/after, 1 changed file per arm, the preserved input/log/EXE hashes, and exact restore of the final copy.
`verification.json` and each `manifest.json`/`output.log`/source/EXE are in the same local directory.
`check-source` on the current root source also gives 391/identical SHA, and the JS25 inputs were compared separately. The new proof runner is
a local verification tool under target and is not claimed to be wired automatically into the official repository gates.

The final document checks are preserved in `target/observation-migration-final-gates.json` and the individual raw logs.
Tracked73/all79 documents clean, docs self-tests12/12, cargo docs_lint1/1, private-header81 clean/common0 header,
5 source. EOL within the documents was normalized consistently. These string/index gates are not called proof of semantic correctness or of native full/relink
isolation. The final Rust391 and JS25 input hashes match the seals above.

## 2026-09-07 follow-up — RED for a genuine ACK starved by completion Full, and neutral notification

### Pre-fix counterexample in the real worker

A later uncommitted tree at the same HEAD. The OUTPUT/observation source391 seal from the previous section was not modified. The actual Worker::run counterexample
was first compiled and run with 392 inputs: those inputs plus `loop_tests/effect_backpressure.rs`, its module registration,
and a read-only test hook after the `released()` commit. There is no effect pump fix yet.

`cargo test -p p4-llamacpp-staged-adapter completion_full_cannot_starve_a_genuine_release_acknowledgement -- --nocapture`
gave **0 passed / 1 failed / 436 filtered**, exit101. Tests excluded by the filter were not added to the passes.
The build actually recompiled the staged adapter, and the existing recovery positives passed before the last new assertion was reached.

| Observation | Pre-fix run |
| --- | --- |
| A's native release | Completed on both stages; the exact final RELEASED raw event held back |
| B's completion Full | capacity1; the real B OUTPUT occupies the only queue slot; a new Full snapshot confirmed |
| Input acceptance of A's ACK | try_send on the real sync input queue succeeded; raw event identical after an event wire round trip |
| A settlement before space is opened | none; pending/slot remain |
| After space recovers | outputs2, release receipts2; A/B native release 1 each per stage |
| Existing oracles | token/text/position/stop, KV, release provenance, observation completeness, event ID uniqueness kept |

The failure string is `completion Full starved a genuine RELEASED already accepted at input`.
200ms is the observation window for the new progress assertion, not the basis for saturation itself. The test recovers space, first checks the existing
normal completion, and fails at the last assertion. It does not prove real EventNode/broker/network, Cancel/Drain,
progress across the whole cyclic network, or CPU utilization.

Preserved in `target/effect-backpressure-red-20260907-01/`: source-before/after.json,
red.log, seal.txt, verification.md and the test EXE. The SHA-256 of the input manifest is
`cf97679e268e3938cd287c8dfedbda28f338e131259b06bfe2af55245788830f` on both sides, and the executable is
`69c71e97b0494c7800ccfba8ff1a270697d9fee09d997511d4aa9380a90ec748`.
Re-review confirmed change0 in the 392 inputs during the build and 3 changes relative to the earlier 391 seal. Later mailbox edits are
a different source state and are not applied retroactively to this RED run. target is local evidence, not a persistent deployment bundle.

### Safety boundaries confirmed before the change

`emit.rs` consumes next_event on every call and also performs direct LOAD/SESSION/UNLOAD/error sends.
Changing only `effects.rs` to Pending could cause ID reissue, a separate unbounded queue and loss of fatal ERRORs.
The pending installation in `tail()` precedes the native Release/Settle and Forward, so in a yielding pump
an early ACK must not be approved merely because pending exists. The read check in `validate_control_batch`
also assumes non-preemption within the same native command. These are code audit findings, not claims of
separate defects executed by this RED. The target contract is owned by the batching document, and the test obligations by the verification protocol.

In the neutral mailbox too, a registered reader not waking on the last publisher's shutdown, and publish
invoking callbacks inside the waker lock, were reproduced with separate tests of the existing API. On the original implementation
both of the 2 tests failed, and the actual recompile log is preserved in
`target/mailbox-capacity-notification-20260907-01/01-before-fix.log`.
These two counterexamples and the additive capacity notification are kept apart from resolving the actor's ACK starvation.

### Neutral mailbox implementation and verification boundaries

`CompletionPublisher::capacity_listener` and RAII registration were added to `node_adapter/mailbox.rs`.
Concurrent registrations are limited to64, and Closed/Exhausted are returned explicitly. This number bounds listener resources, not
node count, events, bytes or total RSS. Registration does not reserve space; it only provides a reason to try_publish again after a drain or receiver
shutdown. The default completion capacity and the semantics of returning the Event on Full/Closed
were not changed. The current consumer of this API is tests; it is not yet wired into the staged worker.

The last sender wakes readers after the actual disconnect, and the last receiver wakes capacity
waiters after disconnect. Clone/drop/wake of user Wakers run outside the internal mutex. When the receiving object
goes away or poll ends Ready, any remaining reader reference is also removed. During review, the latter lifetime surface was
additionally checked; the case where a publisher holds on to a reader after receiver shutdown was reproduced as 0P/1F and then fixed.
That log is `02-reader-lifetime-red.log`. Caller callbacks must be short and nonblocking/nonpanicking,
and panics are not hidden. A test showing the internal mutex is not poisoned is not extended into a guarantee of full panic recovery or event
delivery.

`04-final-green.log`, run with the three files of the 1st freeze, gives p4-adapter **61 passed /0 failed** (existing47 + new14).
The new regressions check close order, buffered Event and sender clone lifetime, registration/recheck races, exact Event delivery
with multiple drainers, the listener bound/release/ID exhaustion, reentrant self-release/re-registration, reader reference release,
and the lock boundaries of Waker clone/drop/wake. A 5-second bounded rendezvous and a
join completion watchdog are used instead of an unbounded Barrier. This is not a guarantee of real-hardware latency, overall scheduling determinism or worker ACK handling.

The SHA-256 values of that 1st-freeze implementation/test/re-export are:

| File | SHA-256 |
| --- | --- |
| `node_adapter/mailbox.rs` | `09a0a2b870d19311c29d911fcdc249e7216cc7d8563f7f0f57610da758bd1c2f` |
| `node_adapter/mailbox_tests.rs` | `6ae33daa0488b540de68885b36d3638a7c15ae64066a2cf439d438d0991bb830` |
| `node_adapter/mod.rs` | `16d2f658244ce0e6b929bf6ab570d3e03cde42b0d9003af0f12a9d2cbadbdc15` |

The root of the three paths is `layers/adapters/adapter/src/`. The protocol envelope, NodeAdapter trait and llama/native
wire were not changed. The component README now also separates the current event boundary from the old Work/hop description,
and the statement "no p4-protocol dependency", which contradicted Cargo.toml, was corrected to the actual neutral dependency.

The full original run in that state is preserved in `target/capacity-slice-20260907-01/`. The SHA of the 393 inputs is
`6d77de2ab50560cd4c9cd3d12801d1b37b78a9fe21f1625b5932660c27da9ff3`, and it matched before and after the run.
The total across the 57 summaries is **1210 passed /1 failed /7 ignored**, cargo exit101. The only failure is 1 test, the actor
ACK progress test above. The proof runner also records suite_passed=false, and this tally is not turned into green.

While preparing mutations, it was found that if the reentrant test callback unwraps an already-dropped mailbox during cleanup, a second panic
can mask the original failure. After keeping the original frozen until the full run finished, only that test's Weak
upgrade was changed to a cleanup-safe guard. Production code and the core assertions are unchanged. After 61P/0F in `06-teardown-safe-green.log`,
the final test file SHA is
`64ac910f32097dbd59165fb91a8b5b1261425fa4a4215005c08d671e7c47b9d4`, and the other two SHAs are the same.
This later state and the 1st freeze state are not merged as the same source.

### Final full tally of the original

The **final393 inputs**, including the test cleanup guard, are preserved in `target/capacity-slice-20260907-02/source/`.
The manifest SHA-256 is `87e1a42d9071865b2f25850e521d76bebd472a178e3e540131f7416845a36c92`, and
the file set and each byte hash matched before and after the full run. This seal covers Rust/Cargo and the listed model-free fixtures.
It is not the full build provenance of documents, third-party registry/toolchain, native, models or JS.

`cargo test --workspace --no-fail-fast --locked` ran to the final exit and gave **57 summaries,
1210 passed /1 failed /7 ignored**, cargo exit101. The only failure is
`completion_full_cannot_starve_a_genuine_release_acknowledgement`, 1 test only. While adding the 14 new neutral tests,
the actor's mandatory RED was left in the normal run. It was not hidden with a runtime skip/ignore/feature, and the current wait
was not turned into a normal expectation. This is a **full suite failure**, not "product approval except for the known failure".

The SHA-256 of `workspace.log` is `ba3b211a257dfcf83b3b770c69d22e26a6e8e961ae963a2b4662ef4d1073c9d1`, and
`workspace-result.json` holds the actual cargo exit and suite_passed=false. The local runner is
`node target/capacity-slice-verify.mjs workspace capacity-slice-20260907-02`, and a copy of that runner is also preserved
in that directory. This command does not overwrite existing logs, so a rerun uses a new proof name.

The current staged worker's 1ms `publish_or_wait` wait, EventNode input retries, Cancel/Drain, effect budget /
ACK send stages and real broker integration are unfinished. Having a neutral API is kept apart from a real consumer using
the notification. The C++/JS real-hardware harness, models/GPU, remote deploy and commit/push were not done this time.

The disabled-by-default `cross-wire-fixture` targets agent_relay/agent_relay_full/broker_registry/cross_wire/
reconnect_delivery — **5 targets with 1 test each — are excluded from build and run** and are not counted in the passed/ignored above.
The existing external-dependency feature settings were not changed. `cargo clippy -p p4-adapter --all-targets --locked` gives
exit0, but 4 warnings about Event enum/Err size remain. This result is
preserved in `target/capacity-slice-20260907-02/clippy-neutral.log` and is not called warning-free.

### Mutations in an independent copy, kept apart from cleanup failures

`target/mailbox-capacity-notification-20260907-01/copy/` is a minimal workspace that copies, unchanged, the
59 project sources/dependency declarations/fixtures of the real p4-protocol/p4-adapter. It is not the whole root workspace;
the workspace membership and the copy's lockfile were trimmed to fit those two crates. The corresponding 59 inputs of the original and the copy
were compared before and after via closure-manifest.json/closure-after.json and matched through the final restore.
This is not extended into a hermetic build proof that includes the registry/toolchain.

Below are the results with the final test SHA `64ac910f...` pinned and **only the copy's production source** changed.
For every valid arm, the actual p4-adapter recompile, the final libtest summary and the source/EXE hashes were confirmed.
The original and copy sources being identical is a separate matter from the MSVC-linked binaries being bit-identical.

| arm | Run passed/failed | Detection condition |
| --- | ---: | --- |
| Final copy baseline |14/0|Same mailbox regressions as the original|
| M1 drain notification removed |8/6|Original Event preservation/retry and actual wake missing|
| M2 wake before sender disconnect |13/1|Closed cannot be observed inside the callback|
| M3 notification before receiver disconnect |13/1|Reentrant publish gets Full instead of Closed|
| M4 recheck after registration removed |12/2|Pending in the controlled arrival/close race|
| M5 reader reference reclamation removed |11/3|Weak still alive after Ready/receiver Drop|
| M6 reader wake moved inside the lock |13/1|Existing lock reentrancy counterexample|
| M7b only the capacity callback moved inside the lock |12/2|Violates the self-release/re-registration and mutex non-poisoning conditions|
| M8 listener bound check removed |13/1|The 65th registration returns Ok instead of Exhausted|
| Exact restore |14/0|New compile, final source/test bytes restored|

All 8 valid mutations are exit101, and baseline/restore are0. The 47 existing adapter tests were excluded from this filtered run
and are not mixed with the original full61 / full workspace1210 tallies. The full source of the intermediate lifetime RED could not be sealed separately,
so we do not claim it can be reconstructed from that log alone. The final M5 reproduces and pins the same lifetime condition
on an independent source.

The broader M7 left not only the callback but also the last Waker destructor inside the lock and deadlocked during cleanup.
It is preserved separately as **1 INVALID/HANG, force-terminated without a FAILED summary**, and excluded from the 8 above.
Only the copy's test process, after confirming its path, was terminated. Without lowering the original expectations, it was narrowed to
M7b, which changes only the callback locking, to obtain two failures that terminate normally. A failing assertion being printed once is not by itself
reported as a completed mutation test.

Commands, each source/EXE SHA, raw logs, tool versions and the final closure comparison follow verification.txt and
each meta.log in the same directory. The SHA of verification.txt is
`eb8ee9456ac4855aaa99b0f0115e366af3281617887cd20a9d854f371e5f2556`.

The final document/scanner runs are preserved in `target/capacity-slice-20260907-02/gates.json`.
Tracked73/all79 documents clean, docs self-tests12/12, cargo docs1/1, private-header81 clean/common0 header,
5 source. These string/index/dependency pattern checks are not proof of semantic correctness, of the unimplemented actor progress, or of native full/relink
isolation. The final source check also matches 393/`87e1a42d...`. After this result paragraph was added,
document lint is rechecked with docs-final-tracked.log/docs-final-all.log.

## 2026-09-07 follow-up — application and transmission authority for head control

An uncommitted implementation on base HEAD `a9e1967fc`. The scope of this change is the dispatch state in `node/state.rs`,
head-only validation in `worker/control_dispatch.rs`, and the actual success hooks in `effects.rs` plus the RELEASED/SETTLED
consumption preconditions. The neutral P4 envelope/core, native wire and llama/backend were not changed in this slice.
The current plan/next action is owned by the roadmap, stage semantics and ticket lifetime by the batching contract, and test obligations by verification protocol T23.

### Pre-fix ACK consumption counterexample

`target/control-progress-red-20260907-01/` preserves the first two tests, the sources at the time, the executable and the raw output.
After an actual staged/agent-core recompile, `cargo test -p p4-llamacpp-staged-adapter control_progress_tests
-- --nocapture` gave **0 passed/2 failed/437 filtered**, exit101. Well-formed encoded ACKs were fed into a prepared pending state
through the actual codec→Worker::handle. RELEASED returned two unapplied slots
as free, and SETTLED resumed two unapplied direct Proposal/Replay entries. The ledger, requests, native calls and
output effects were all compared. This test is an approval precondition for a future yielding seam; it is not a reproduction of an attack
in which the same intermediate state is exposed externally in the existing synchronous run-loop. It is not merged with the real Full ACK starvation RED.

The manifest SHA of the 405 RED source files is `e7418f942945455a8e859a57c268bb0caab96096341f3c1163fcf42e3dd08ce0`,
and the executed EXE is `bcd325c2882da2308c532bb06dd389f67abc35275064bdb9326f0f37b89ae44c`.
The 405 original/preserved files matched before and after. This closure uses a Rust/Cargo/JSON glob scope, so its file
selection differs from the 396 below. A different count or hash alone is not used to estimate the amount of source change.

### Real consumer boundaries and limits of proof

The 6 follow-up `control_progress_tests` reject, as whole events, both orders of unfinished members, Queued/LocalApplied, and load/session mismatches,
and check that the same ACK, with only dispatch repaired, resumes normal release/direct Proposal/Replay.
Only the diagnostic next_event may increase, by exactly1; every other business snapshot and the native calls must be preserved.
Placing a handcrafted ForwardAccepted in the existing consumer tests makes that consumption precondition explicit; it does not mean native or
sending was tested. The production boundary was checked separately with new effect tests.

The 9 `control_dispatch_effect_tests` inject the initial load/session/KV state and go through the real flush_effects→
native Frame/P4ID→owner receipt/frontier→completion. The fake engine applies native changes first with an independent byte parser
and produces normal, tampered and lost responses. The new file was added after the production fix, so
no old-version RED is claimed. The full run and the independent mutations below pin down that the regressions can fail.

| Check | Required result |
| --- | --- |
| RELEASE/SETTLE native success | LocalApplied after exact receipt/frontier application; no Forward authority |
| Loss/tampering after a native change | Stays Queued, no state commit, fence, intent preserved, no native retry |
| cached native replay | Additional native 0 calls, no promotion/regression of LocalApplied/ForwardAccepted |
| Exact whole-command forward | All members ForwardAccepted after the exact Event/body/target is accepted |
| Closed/ID exhaustion | LocalApplied, pending, native and unsent intent preserved, fence |
| stale scope/identity, wrong route/class, last member in error | No partial promotion/send/native execution of the leading members |

The current local and forward tickets are different private types. The key is valid only on the premise that there is no
actor yield between prepare→synchronous effect→success callback. This is not an asynchronous reservation. Keeping the ticket after Full,
yielding in the middle of a command's native group, and reserving the ACK receipt budget are not yet implemented or proven. These 9 are
not tests of the full Worker::run/virtual network, real llama or GPU either.

### Final full run of the original

The **396 Rust/Cargo/listed model-free fixture files** in `target/capacity-slice-20260907-03/source/` were frozen.
The manifest SHA-256 is `db6f4968083b76573a0b7cda139bcaf06d913198d96055d614d30d323d61041a`, and
the file set and byte hashes matched before and after the run. This is not the full provenance of native/toolchain/registry/documents/JS.
The final **57 summaries of `cargo test --workspace --no-fail-fast --locked` are 1225 passed/1 failed/7 ignored**,
cargo exit101. This includes the 15 new tests and the existing normal paths. The only failure is the existing
`completion_full_cannot_starve_a_genuine_release_acknowledgement`, 1 test, and the oracle/default run were not changed.

The `workspace.log` SHA is `c173cc0bb4cc90fc75e765edeb9dd2432d4ff54a8eb75f93a21fcc1347238c9a`.
The actual command, start/end and source are in workspace-result.json, with **suite_passed=false**. The local runner copy and
the command `node target/capacity-slice-verify.mjs workspace capacity-slice-20260907-03` are preserved too.
Existing logs are not overwritten, and a new rerun uses a new proof name. The earlier preliminary narrow tests 6/9 and the full staged
442P1F are not added to this final run as separate additional samples.

The 5 external `cross-wire-fixture` targets, 1 test each, are excluded from the default build/run and are not counted in passed/ignored.
The full C++/JS harness, models/GPU, remote deploy and commit/push were not done in this slice. Production consumption of the capacity API,
a bounded outbox / future OUTPUT, receipt reservation and Cancel/Drain remain, so this result is
not evidence for promoting a performance stage or a real VRAM-only/RAM offloading wave.

### 8 phase mutations in an independent copy

`target/control-dispatch-mutations-20260907-01/` copied the same 396 inputs and the workspace declarations/Cargo.lock
verbatim. Only the 15 new tests are run with `--locked --offline`, and for every arm the actual staged recompile,
an EXE modification time after that start, the actual final summary/exit and the preserved EXE hash were confirmed. Tests and expectations
were not changed; only one production file was changed. The 396 original inputs were identical before and after every arm, and the copy was restored exactly.

| arm | passed/failed | Detection |
| --- | ---: | --- |
| baseline |15/0|The two real consumer boundaries on the injected initial state|
| ack-phase-bypass |11/4|An unapplied ACK consumes state|
| local-hook-omitted |9/6|LocalApplied transition missing after native success|
| forward-hook-omitted |13/2|ForwardAccepted transition missing after send acceptance|
| forward-before-send |14/1|Early promotion before Release's ID exhaustion|
| replay-phase-downgrade |14/1|native replay regresses the send-complete stage|
| forward-route-bypass |14/1|Sending to a target that is not the declared next stage|
| pending-member-bypass |13/2|Changes to the original control identity are not pre-validated|
| queued-forward-bypass |13/2|Skips to the send stage without native application|
| exact restore |15/0|New actual compile and full source bytes restored|

All 8 valid mutations are cargo101, and baseline/restore are0. No arm counted a compile failure or hang as a detection.
forward-before-send failed at the first Release/ID exhaustion assertion, so we do not extend this to claim its Closed/Settle branches
were detected. The 9 original positives did exercise those branches. Of the 2 failures in pending-member-bypass,
one directly detected that sending the last op999 was allowed, and the other is a secondary failure in which StageOwners rejected later and the expected error
layer changed. This is not a count of 2 independent defects found.
Each arm's source copy, source manifest, raw output, EXE and actual command are in the arm directory.
`verification.json` is the result of rechecking every source/log/EXE. A difference in MSVC-linked EXE hashes from the same source
is not read as a source mismatch. The existing real Full ACK test is outside this 15-test filter and still fails
in the full original run. So these 8 mutations do not mean the saturated actor problem has been solved.
The verification.json SHA-256 is `88060ec41b72444d68a5000fe3a7018adc8a046dae72a0ec7a6bc514e2cf0eeb`.

### Static checks and document gates

`cargo clippy -p p4-llamacpp-staged-adapter --all-targets --locked` on the same frozen source gives exit0.
Log: `target/capacity-slice-20260907-03/clippy-staged.log`. Warnings remain — staged lib16, lib test28 (14 duplicates),
dependency adapter4/agent-core1 — and it is not called warning-free. Source cleanup was not mixed into the frozen run.
The document/dependency scanners are preserved in `gates.json` in the same directory: tracked73/all79 clean, docs self-tests12/12,
cargo docs1/1, private81/common0 header, 5 source. The number/link/pattern checks are not proof of semantics, native link isolation,
the unfinished actor, or real-hardware promotion. After this result was added, document lint is rechecked with a separate final log.

### SESSION counterexample for the next reservation slice — outside the original full tally

`target/session-emission-reservation-red-20260907-01/source/` is an independent pristine copy of the 396 inputs above.
The workspace declarations/lock/fixtures are unchanged, and an actual `--locked --offline` recompile into a new target (41.04 s) gave
the existing SESSION baseline of 6 tests / 0 failures. After that, 3 tests were added only to the copy's `worker/session_tests.rs`.
The original production/test bytes were not changed. After a 12.06 s recompile, **7 passed/2 failed/446 filtered**,
cargo101. This probe is not added to the original full1225 tally or to the 15 phase mutation tests.

At `next_event=u64::MAX`, the actual `Worker::session` and the actual `Worker::handle` each reject, but
sessions changes from empty to one declared-pipeline. next_event, effects=[], effects_fenced=false,
Lifecycle::Empty and has_server=false are preserved, and there are0 completion Events. In other words, this counterexample is a **state commit before the response ID
is secured**, with generation/route input state injected and no model/native
installed. The direct error is actually mislabelled as `completion queue is full`. ID exhaustion happens before entering publish_or_wait,
so this run is not evidence of observing a real Full/Closed.

The added normal handle positive compares sequence41→42, the SESSION_READY v4 JSON, and the 340-byte full Event encoding with its
round trip and single delivery. It is not made to pass by removing only the ID-failure input. The raw output is 02-probe.log,
its SHA-256 is `dcb7d7b0a4140c027a157dadc78153d86bd8c54950814b4619dcc9ddf04babf0`, and
the preserved executed EXE SHA is `6784d3c8dcb0e9c87efda33641feae0a782a263440fc9c3a67a7233c26246b1c`.
The before/after source manifests and the added test diff are preserved in the same directory. The original is not yet fixed or complete, and
this is not extended into proof of preserving native results during Full, a full count/byte budget, capacity wake or actor progress.

## 2026-09-07 follow-up — SESSION response preparation

The ID exhaustion counterexample above was moved into the original default tests, and the SESSION consumer boundary was fixed. The production/test
changes in this slice are only three files: `worker/emit.rs`, `control.rs` and `session_tests.rs`. The existing phase396 sources and
the remaining inputs are the same, and the neutral protocol/native/llama/backend were not modified.

### An additional independent RED and the consumer semantics

`target/session-envelope-red-20260907-01/` is an independent copy that uses the production code of the earlier phase396/db6f4968 seal
unchanged. Adding only 1 probe to session_tests and compiling fresh (39.90 s) gave **0 passed/1 failed**,
cargo101. The normal response positive ran first, and then the real wire round trip of an input with an original ID of 140,000 bytes was confirmed to
succeed. That input envelope was 140,232 bytes, but the response duplicates the original ID in both the ID and causation, so it
becomes 280,253 bytes. The actual session() gave Ok, sessions0→1, ID1→2, but the emitted Event encoded successfully and
failed to decode. This is a real consumer counterexample showing that per-field checks differ from the aggregate envelope check.

The raw SHA of this RED is `3e54bc6915945187a3b6c09bd1e77e935ddce7650ff8805245702eedc2d7bd99`,
and the preserved EXE SHA is `aa8c621f3f0446fdc819713ec94cf5ef4218387d2245201f4c4167828cc7458b`.
verification.json/md, the source and the raw log are preserved, and the original production bytes were not changed. This is a handler test with the session generation and
route injected, not a real LOAD/Worker::run/network/native/GPU test.

The current prepare_json_emission completes, on &self, JSON serialization → a checked ID candidate → exact Event construction → a real
encode/decode identity check. After that comes session installation → 1 next_event commit → the existing synchronous publish.
Only SESSION consumes this helper. The private prepared data is only for the synchronous section; it is not a reservation that other emitters or an await/yield
could interleave with. The temporary encode/decode copies check whether the response is representable; they do not secure a memory budget.

The 12 formal SESSION tests are existing6 + new6: ID exhaustion direct/handle2, the existing 340-byte response1, aggregate envelope
direct/handle2, and Unicode metadata/body1. The exact payload, Event route, ID and raw bytes, and single delivery are kept.
A direct preparation failure preserves session/ID/effects. handle's normal rejection loop may consume 1 separate diagnostic
ID, and the generic ERROR fallback for a huge ID can still produce an envelope that cannot be received.
If IDs are completely exhausted, the diagnostic cannot be sent either. This is not claimed as successful delivery of a normal error response.
The existing state/ID commit on Closed after preparation was not rolled back either. That failure lifetime and a fixed outbox are unfinished.

### Final original run and sources

`target/capacity-slice-20260907-04/source/` preserves the final 396 Rust/Cargo/listed model-free fixture files.
The SHA-256 is `9394a953b28063add0e5919c641fab6f872f8449d15bc980d3022504ee3fe8eb`, and
the file set and byte hashes matched before and after the full run. The final 57 summaries of `cargo test --workspace --no-fail-fast --locked`
are **1231 passed/1 failed/7 ignored**, cargo101. The failure is still the existing
`completion_full_cannot_starve_a_genuine_release_acknowledgement`. The original full suite fails.

The workspace.log SHA is `f5aee2f6d4efd51d752a3c390777ea3d137b80fc2bf7bffcf78a1df8bec5b544`;
the actual exit/command/source are in workspace-result.json, with suite_passed=false. This is not mixed with the earlier 1225 tally
or its source. The external cross-wire-fixture 5 targets / 1 test each are still excluded from the default build/run.
clippy staged/all-targets/locked gives exit0, with staged lib16/test28 (14 duplicates) and adapter4 warnings remaining.
This is not warning-free and is preserved in `clippy-staged.log`. The C++/JS harness, model/GPU waves, remote deploy and
commit/push were not done this time. The document and model file stat survey is not part of this Rust source seal either.

### Preliminary list of local model paths

The user-specified S:\models was read under the current local account. `target/model-file-inventory-20260907-01.json` holds
the path/size/mtime of 156 GGUF files, which group into 63 groups by file name. The provisional name classification is 40 model candidates,
1 embedding and 22 projectors, with no missing split numbers. The metadata manifest SHA is
`a83dda1d0ca046d3be91bccbffd7ecf0a40984b3c46ab22888ff620da306fc2e`. This is not a hash of the actual contents, and
matching file name/size/mtime is not evidence of model identity or loadability. Non-GGUF files, header metadata,
memory family, remote account access and a full model/variant audit are unfinished. No model was read, loaded or run.
The real-hardware resource stages and approval criteria follow roadmap §1 and verification protocol H0.

### Independent SESSION mutations and final gates

`target/session-preparation-mutations-20260907-01/` is an independent copy that preserves the final 396/9394a953 pristine source, workspace
declarations and Cargo.lock. Only the current 12 SESSION tests were checked with `--locked --offline`.
Each arm preserves the actual staged recompile, new EXE time/hash, full source, raw log and final summary, and
the 396 original inputs and the test expectations were the same before and after every run.

| arm | passed/failed | Detection |
| --- | ---: | --- |
| baseline |12/0|The current real SESSION consumer boundary|
| install-before-prepare |8/4|Session authority installed before the preparation failure|
| checked-id-bypass |10/2|Approves an ID-exhausted input|
| id-commit-omitted |10/2|ID stays at 41 after a normal response|
| id-commit-twice |10/2|Double increment to ID43 after a normal response|
| decode-preflight-omitted |10/2|encode kept, but the receiver rejects the aggregate envelope|
| exact restore |12/0|Full source restored exactly, new recompile|

The 5 mutations detected real differences in state/approval/ID, and no failure was counted based only on a different error string.
baseline/restore are0 and the mutations are101; compile failures/timeouts were not counted as detections. Passing this 12-test filter
does not stand in for the separate original full 1231P/1F/7ignored or for a pass of the saturated actor. verification.json/md
record the actual run commands, tool versions, each source/EXE/log hash, and the scope.

The final document gates are in `target/capacity-slice-20260907-04/gates.json`: tracked73/all79 clean,
docs self-tests12/12, cargo docs1/1, private81/common0 header, 5 source. After this table is added, the final document
logs and the source check are rechecked. B1/B2/B5 or real-hardware promotion is not approved. The remaining first action is
owned by the last progress record in the roadmap.

## 2026-09-07 follow-up — effect-preserving representation and pre-allocation checks

### Implementation and proof scope

The code basis is uncommitted changes on HEAD `a9e1967fc59dffa6c2e458f1b91f916b1df826c1`, and the final
397-file source seal below is the actual code of this round. Do not read it as already implemented in the starting HEAD.

- The base of both `worker/effects.rs::CommittedEffect` and `observe.rs::PreparedTelemetry` is an Envelope.
  The TAIL payload is not copied per OUTPUT, and Forward does not keep a separate copy of the earlier physical input body.
  The producers `release/settlement/physical/drive` and the existing consumer tests were migrated too.
- `Worker::flush_effects` owns the original intent via pop instead of cloning the whole front, and on failure restores the same
  original to the front. The Clone derive on `CommittedEffect` was also removed. Forward's Vec moves into the mailbox
  and returns to the original intent on Closed/ID exhaustion/shutdown failure. The small native SETTLE candidate copy is
  kept so that the response proposal does not change the original intent. The observation time is pinned only after a successful forward, and
  telemetry is moved to the front. A later observation failure does not cause forwarding to happen again.
- The target of this migration is the **synchronous consumer boundary**. It still blocks during Full, and what remains after a failure is
  Envelope+DTO/body, not a whole fixed Event that can be resumed. The ID consumption and fence semantics of normal sends are
  kept. There is still no RSS budget that covers active inputs, parsed objects, JSON serialization and in-flight effects.
- `capsule/decode.rs::read_capsule` checks **before reserving** that the 24-byte outcome header, the generated minimum of 12 bytes,
  and the sum of the remaining outcome headers and proposal/replay i32 arrays fit within the cursor's remaining bytes.
  It checks the range with division before multiplying to avoid overflow. The existing valid wire count bound was not reduced.
  This is a necessary wire-size condition, not a bound on total parsed heap/allocator/RSS or validation of native semantics.

### Real consumer regressions and the pre-fix counterexample

The 6 tests in `worker/effect_representation_tests.rs` call the actual prepare_outputs/flush_effects/mailbox.
They do not stand in for native/model/run-loop. They compare small/large causal payloads and output counts1/8, and
check that the original allocation of a 32KiB nonempty Vec arrives in the real mailbox. For normal Unicode output, they confirm
Event ID/sequence/causation/source/target/return route/correlation/deadline, token/text/position/stop/
completion fields, order, and duplicates0. On Closed, ID exhaustion and Full-at-shutdown, they preserve body/nested telemetry/queue suffix
and the original allocation; on observation ID exhaustion after a successful forward, they check the pinned time, order and forward re-execution0.
The pointer check is limited to the ownership move of a nonempty allocation and is not a TPS or RSS figure.

The existing 9 head native dispatch tests and 8 observation tests, and the output/KV oracles of release notification and the actual loop, were
kept in the full run. The existing ReleaseReceipt assertion `base.payload.is_empty()` was migrated into the type constraint of an Envelope
that has no payload field, and the original submission provenance/recipient/payload assertions were kept.

`target/capsule-capacity-red-20260907-01/` attached only a test-only capacity observation to the existing decoder and
reproduced **0 passed/2 failed** with a small invalid input declaring count=3. The actual compile took 9.58 s, and
the capacity trace requested by the code is the counterexample. The 3 decoder/cursor/capsule inputs were the same before and after the run;
manifest SHA `42d5595aefb6afce02f0312ec41dfa43c8f1a3db192441bb66ad5a0380ef2a3e`, actual EXE
`054649a151630c0b77f315282b78fe45013974f1976e726eb8d00038aee03cfb`. This is a partial input seal, not
evidence for the whole workspace transition. No huge allocation or mutation of the original production code was performed.

The 7 new decoder regressions check, in the actual CapsuleSet::decode, zero/exact minimum/one byte short, Unicode,
mixed capsules, checkpoint/proposal arrays, and an impossible MAX-u32 declaration. The active test observation
panics on a capacity over 1024 before it reaches the allocator, so mutations do not exhaust the development host either.
Normal production has no such test ceiling. The compile failure during the Envelope migration in the intermediate `green.log` was excluded
from the execution evidence. After a separate focused7/0, it passed again in the full run of the final 397 inputs below.

### Final original run

`target/capacity-slice-20260907-05/source/` preserves the 397 Rust/Cargo/listed fixture inputs.
The SHA-256 is `ce33c532f53e8a8c50a454303d1aa672d8883c6fb619fa3915cbd332958d7e07`, and
the file set and byte hashes were the same before and after the full run. The final 57 summaries of `cargo test --workspace --no-fail-fast --locked`
are **1244 passed/1 failed/7 ignored**, cargo101, suite_passed=false.
The workspace.log SHA is `11760b25b4821842881cd2d570c7c3d85d1151d73f3f9a2a47662ba880deeda0`.
The existing `completion_full_cannot_starve_a_genuine_release_acknowledgement` is the only failure, and
the test/expectations were not modified. The 5 cross-wire-fixture targets excluded from the default features were not run.

clippy staged/all-targets/locked gives exit0, with warnings staged lib17/test29 (15 duplicates), adapter4 and
agent-core1. The owned Event return of `publish_or_retain` **added 1 result_large_err warning**.
The warning was not hidden, and boxing or interface changes were not made at the last minute in a way that would diverge from the verified source. This is not
warning-free overall. The clippy log SHA is `871406ac44416c5f7d7dbbbdd20e859306594c2c4249c8df7bfc05840313afd0`.

This round did not run the C++/JS real-hardware harness, models/GPU, remote deploy or commit/push. It is not a VRAM-only or
RAM offloading wave or a multi-computer result, and it does not approve stage promotion. Future lifetime/budget/ID
reservation decisions are owned by the target section of the batching contract, and the current first action by the last roadmap record.

### Independent mutations of the representation migration and an EOL correction

`target/effect-storage-mutations-20260907-01/verification.json` is an independent run that copied all 397 files of source05.
baseline/restored are13/0, and on the same 13 tests it detected real assertion failures for front clone (2), missing restore of a failed effect (3),
forward body clone (3), missing telemetry promotion (1), removed outcome length check (2), and removed generated sum check
(5). Every arm confirmed an actual fresh compile, EXE and the same test membership, and the parser's test-only allocation watch was kept.
Compile errors/timeouts were not counted as detections.
This is evidence for source05 and is not cited as applying to later sources.

The cursor.rs line-ending normalization command failed at first, so source06 was rerun with the same bytes as05
(1244/1/7). After the actual normalization, source07 has the 397-file SHA
`94c9f1d4ce95a069ade5fb88ae14374fc3e630a02ab1803f97a8420092f89e07`, and its full 57 summaries were
1244/1/7, cargo101. The only code difference from 05→07 is the EOL of cursor.rs, and the LF-normalized hash
`644abc79f5a2219648706fde961b04288dabd1b84e576e76a064bca8a086cac6` is the same.
The 07 workspace.log SHA is `410bb928ca4bcf9bcc30b54556e6479a5237e9ab3b36e69f83594e94e681cb17`.

## 2026-09-07 full WIP checkpoint — bounded ACK service integration

### Design re-evaluation and implementation scope

At the user's instruction, the long-running accumulated changes are committed as a full checkpoint instead of keeping only part of them.
Temporary builds, raw run output and independent copies keep the existing target ignore policy. The commit that includes this record
is **an intermediate restore point, not a completion/promotion commit**. Afterwards, the first git show will be used to compare this record against the sources.

The 3 independent read-only reviews and the actual code reached the same conclusion. In the current Full counterexample, the blocked sender is not the B OUTPUT
but the **B RELEASE forward**, and the same worker that would apply A's ACK is waiting for send space. The full RSS/
native HELLO resource model was not a serial prerequisite for fixing this one counterexample. The change is a single boundary that keeps ownership
of the original outgoing Event and handles reachable ACKs with side-effect-free prepare/commit.

- `ack_service.rs`: 1 input at a time. Only RELEASED/SETTLED are allowed, with 0 recursive calls into handle/flush/native/drive.
  A non-ACK is held as 1 original input; the first bad ACK is held as 1 diagnostic, and a second error is
  held as the original input with no further reads. It does not guarantee progress for an ACK trapped behind the FIFO.
- `release.rs`: RELEASED prepare and the pure commit were separated. The original full validation, per-reply grouping,
  slot/admission order and settlement observer are preserved, and notifications are registered behind the existing FIFO.
- `obligations.rs`: sums the number of future receipts of pending releases, the unmaterialized suffix of queued and active effects,
  and the diagnostic ID obligations. The actual Event sequence is issued only when an outgoing item is materialized.
  This is a count/ID invariant, not preallocation of future receipt bytes or an implementation of a full RSS budget.
- `emit/effects`: a Full retry keeps the same Event, and authority is rechecked right before each head forward
  offer. No ACK is processed between a successful offer and its callback. Active sends,
  held inputs and diagnostics were added to the shutdown observation. native call groups remain non-preemptive.

### Current verification and what is unfinished

The SHA of the 399 Rust/Cargo/fixture inputs in `target/capacity-slice-20260907-08/source/` is
`7ba8cf32852e4d8820200ef3d6cd14a4098710b95b3326eb9a7496db51c2ab7d`. The full
`cargo test --workspace --no-fail-fast --locked` gave **1236 passed/9 failed/7 ignored**, cargo101,
across the final 57 summaries, with source before/after identical. The log SHA is
`ce5bb371a1dfae71e35c46480c1e688620b510552c1b97107460edfb3a3bf578`.

The existing actual Worker Full ACK counterexample passed. However, this is neither full green nor a completed fix.
The 9 failures are as follows, and they are not hidden or turned into ignored/feature exclusions.

| Failure group | Count | Next judgement |
| --- | ---: | --- |
| 3 head forward rejections |3|Keep both the initial preflight that preserves the specific cause/unchanged ID and the recheck on every offer|
| SESSION ID error reason |1|Preserve the existing diagnostic contract|
| 3 exhaustion after observation/forward |3|Separate the consumer boundaries so pre-reservation is not confused with delivery failure of already committed effects|
| 2 receipt/OUTPUT exhaustion |2|Add pre-commit rejection counterexamples, and inject the existing post-commit failure preservation tests at that actual point|

The additional real-loop tests for bad ACK→valid ACK and non-ACK FIFO recovery were not yet finished at this checkpoint.
Only the shared fixture preparation/recovery helpers were extracted. The existing assertions are preserved, and new tests that were not run are not
counted as passes. Additional design tests, independent mutations, capacity wake, a general byte budget, EventNode credit,
Cancel/Drain and GPU waves are unfinished. The C++/JS harness, models, remote runs and push were not done in this integration.

## Second full checkpoint — verification of bounded ACK progress (2026-09-07)

The first intermediate commit `2e9451a5cb349740982db3e7478b6c9beb1440d3` preserved all 196 accumulated files, and
right after the commit, non-ignored changes/untracked files were confirmed to be 0. This record covers the fixes and verification since then, and does not claim retroactively that the
9 regressions at that time passed. All source, test and document changes are again included in the next checkpoint.

### Rationale and boundaries of the fix

1. The initial head control preflight is responsible for the specific error and ID preservation, and the check right before each offer is responsible for
   the current authority that an ACK changed during Full. Neither replaces the other. A historical control replay
   retired by an ACK restores the original body/intent and is fenced. It is not absorbed as a harmless success.
2. The obligation pre-check is **before commit**, and already committed effects consume their own share. The latter
   does not re-require the whole share at each serialization. The checked_add on the actual ID is still kept, so if an ID failure or corruption occurs
   after commit, the undelivered intent is preserved. Direct responses cannot borrow future shares.
3. When N pending RELEASED entries are converted into receipts for G original owners, `G <= N`. The check covers the sum of queued effects,
   the not-yet-materialized observations of active ForwardObserved, future receipts, and held diagnostics.
   The actual Event sequence is consumed only when FIFO outgoing items are materialized. This formula is an ID count, not a RAM reservation.
4. All TAIL return candidates and effects are checked in full before commit. The existing OUTPUT/receipt exhaustion tests now
   inject the fault after the actual commit and keep the original assertions on intent preservation / no re-settlement. Separate pre-commit
   shortage tests check preservation of all requests, ledger, slots, effects, IDs and mailbox/native effects, and success with exactly enough room.
5. The obligation check on head native results is also **before** prepare_issue. Retrying twice while short does not change the prepared
   issue/flight/request/owner/frontier/native, and with exactly 3 IDs of room it runs with the first logical ordinal1.

### 8 new real consumer tests

All tests are in the default staged lib set and are not hidden with ignored/feature exclusions.

| Test name | Real consumption and guarantee |
| --- | --- |
| `completion_full_defers_one_bad_ack_error_without_blocking_the_genuine_ack` | Worker::run, Full caused by a real B OUTPUT; while keeping 1 diagnostic with the bad ACK's original provenance/JSON, the valid ACK commits before space recovers, native increase0; normal OUTPUT/KV/receipt kept after recovery |
| `completion_full_holds_a_non_ack_without_reading_past_it_then_recovers_fifo` | Worker::run, holds the order C PREFILL→valid ACK, with C execution/ACK overtaking0 during Full; after recovery C is accepted before the ACK and everything completes normally |
| `b2_completion_full_settles_both_speculative_continuations_without_native_reentry` | Full caused by the ACKs held after real SETTLE on every stage and two real SESSION_READY events; Direct/Checkpoint each apply only settlement first with native history unchanged; existing literal token/position/KV oracles kept |
| `full_control_replay_revalidates_its_ticket_after_ack_retirement` | Real flush/native Frame/mailbox; if an ACK removes authority during a RELEASE/SETTLE replay Full, stale redelivery0, additional native0, original body allocation/intent preserved. The initial KV and ACK echo are single-worker injections, not proof of multi-stage ACK generation |
| `head_id_shortage_precedes_prepared_issue_and_exact_room_still_runs` | Real head handle/drive/codec. State preserved across 2 pre-shortage attempts, and a positive run with exactly 3 IDs |
| `receipt_id_shortage_before_commit_preserves_ack_and_slot_authority` | Real RELEASED consumer. Whole pre-rejection, native/notification0, pending/slot/ID preserved |
| `direct_responses_cannot_spend_ids_owed_to_pending_receipts` | A direct ERROR cannot consume the share of 2 pending entries; the subsequent real ACK then publishes 2 per-owner receipts from its own share |
| `output_id_obligations_refuse_whole_return_before_commit_and_accept_exact_room` | Real TAIL decoder/flight consumer. Atomic pre-shortage rejection of both OUTPUTs (2) and success with exactly 2 IDs |

The positive recovery, output and settlement assertions of the existing `completion_full_cannot_starve_a_genuine_release_acknowledgement`
were kept. The Full/ACK service does not issue new native computation. ACK progress in front of an existing non-ACK FIFO and
after a second error is out of scope. Fenced convergence of a stale replay is not a complete reconnect/drain either.

### Sealed full run

- Original: `target/capacity-slice-20260907-09/source/` and `source.json`, 399 Rust/Cargo/fixture files.
- Input SHA-256: `b50af68ed3760f10b04b1cb88eb6e5ccfaf0d3d7a4f1c079d082c9a6079e2ea6`.
- Command: `cargo test --workspace --no-fail-fast --locked`.
- Result: **1253 passed /0 failed /7 ignored**, final 57 summaries, exit0. staged lib479/0.
- Sources identical before and after the run. Raw output: `target/capacity-slice-20260907-09/workspace.log`.
- Raw output SHA-256: `54525d9de589023a710b47894e7f458a833c739991bc4ad0c235a3c2bf5a4715`.
- The full run took place 2026-09-07 03:01:47~03:04:09 UTC. This is not a C++/JS real-hardware harness, model or GPU run.
- Later document gates: tracked/all 79 files each clean, self-tests12/12, cargo docs1/1.
  private-header string gate 81 files clean, common debt 0 header/5 source (existing debt kept).
  This is not a C++ rebuild or proof of semantic compatibility. The raw output is `gates.json` and the individual logs in the same proof.

### 5 mutations in an independent copy

The raw data is `target/ack-service-mutations-20260907-01/verification.json` and each arm's source/log/manifest/EXE.
All 399 inputs plus Cargo.lock/fixtures were copied, and each arm checked an actual staged recompile, a new EXE hash, and
identical names for the 25 tests. Original changes0, exact restore of the final copy, compile failures/timeouts not counted
as detections. Per-arm sources and EXEs are preserved. The runner is `runner.mjs` inside that proof.

| arm | passed/failed | Invariant removed |
| --- | --- | --- |
| baseline |25/0|none|
| ack-service-omitted |20/5|ACK consumption during Full|
| id-check-after-issue |24/1|ID rejection precedes issue preparation|
| future-receipt-omitted |24/1|Future share of pending receipts|
| diagnostic-blocks-valid-ack |24/1|A valid ACK is processed even while 1 diagnostic is held|
| head-recheck-omitted |24/1|Current authority check right before each offer|
| restored |25/0|Restored to the sealed original|

The default regressions are reproduced with `cargo test -p p4-llamacpp-staged-adapter --lib --locked`.
Each mutation applies only the one change above to an independent copy and keeps the same 25 tests. The filters are `completion_full_`,
`full_control_replay_revalidates_`, `head_id_shortage_`, `v2::node::worker::release_notification_tests`,
`v2::node::worker_tests::t23_`, `v2::node::worker_tests::output_id_obligations_`,
`v2::node::worker::effect_representation_tests`. Reverting the original with checkout/reset is forbidden.

### What is not promoted

This only closes the local ACK starvation and these ID/effect consumption regressions; it is not full completion of B1/B2/B5. Reservation of byte/RSS/
native result space, integrated capacity wake, the return route behind a non-ACK, EventNode/broker credit and
graceful Cancel/Drain remain. This evidence contains no final output quality, TPS, GPU utilization or multi-computer real-hardware results.
The first next action and the overall order are owned only by the latest progress record of the execution roadmap.

## Cross-check against the external review and Git inclusion review (2026-09-07)

### Timing and verification scope

The external review's 11:43~11:45 snapshot matches the first WIP's 1236 passed/9 failed/7 ignored.
Do not confuse it with the later `96c90f99e` result of 1253/0/7 and the 399-file sealed input comparison. The local GREEN of the ACK service
and the 5 mutations are recorded in the previous section, but that is not completion of ResourceBudget/byte/RSS.
The current `native_calls` and `requests` are used in the assertions for native invariance during Full and FIFO recovery of the held PREFILL,
so they were not removed on the basis of an old remark that they were unused.

There are two code differences this time. The unreachable Full(_) branch after a single WorkerInput::Event has already been handled
was removed. And `cancel_prepared_issue` was restricted to test builds, with the 4 existing test calls
kept. There is no yield or general cancel branch between a successful prepare_issue and begin_native_issue.
This is neither an operational Cancel implementation nor a fix for misclassifying a truly reachable Full as Closed.
No behavior was added that turns an unknown state after a native attempt back into a cancel.

The intermediate run `target/capacity-slice-20260907-10` was 1253/0/7. After refining comment wording, the
final Rust 399-input SHA256 is `c56ff070e61aec2a857d6b0a6113842ab04ee650b69d3855d74be6efb90a4d23`.
The full run in `target/capacity-slice-20260907-11` gave **1252/1/7**, 57 summaries, cargo101.
The failure was mixed EOL in 2 documents, a procedural error of starting the full tests before the document format cleanup.
The raw output SHA256 is `e5a43fb4b2071801bb8ca7d9f4f61f2b9ece2364eddc7cced104be9eab39f148`.
The harness passed the actual file list of `test/benchmarks/p4-4node/**/*.test.mjs` to `node --test` and
confirmed **72 passed/0 failed/0 skipped**. C++, GPU, remote deploy and push were not run.
No rerun of the 5 mutations is claimed for this cleanup. That is separate evidence from the previous checkpoint.

After the document format cleanup, **1 additional round out of the requested cap of 3 rounds** was run
as `target/capacity-slice-20260907-12`. On the same Rust 399 inputs as above,
`cargo test --workspace --no-fail-fast --locked` gave **1253 passed/0 failed/7 ignored**,
57 summaries, cargo0 (04:00:53~04:02:58 UTC). The raw output SHA256 is
`276c791ccdeb3fa4ca85efc5bd76e9462a5c12cf3f0743857ce0be7641a68ad1`.
The same batch confirmed docs-lint default/all 79 files, self-tests12/12, cargo docs1/1,
private-header 81 files with common debt 0header/5source, and the harness 72/0/0skipped.
The contents of the Rust inputs were identical before and after the run. The inputs were not edited during the run, and after it ended
only this result record was added and the document format/index rechecked. This non-behavioral cleanup passed in the first round,
so no second or third iteration was run. The earlier raw output of the document failure is preserved. This 1 round does not mean that the still unimplemented
cyclic wait fix or distributed batching as a whole is complete.

### Retention decision

A generated bundle of about 1.06MB / 36 files and a 104-line retention tool specific to one commit were built, but **their inclusion in Git was withdrawn**.
Most of it repeats the 399-file source ledger, and checking raw output hashes does not recover an independent rerun of the EXE, environment and paths of that time.
This is not extended into a new shared tool for batching development. Without deleting anything, it was moved to
`target/unpublished-ack-archive-20260907-01/`, and the existing `/target/` ignore rule was confirmed.
The SHA256 of the preserved `bundle/manifest.json` is
`35fc1d91f0405f6f8c69c52209ec6a4d755477efbc3565a2dace2fe66581563e`.
The paths of the original run results and mutation raw data are as in the previous section. They are **only preserved locally, not long-term evidence**.
Even though comparison of the 35 raw files / 399 Git contents and rejection of a tampered copy were confirmed from another path, that is not a test rerun,
so the verification protocol's item for re-inspection/reproduction on another machine is not met. Nothing was uploaded to an external store.
Git keeps the actual sources, regression tests and this concise record. New JSON, retention tools and raw output copies are not added.

### Code-level candidate for the next counterexample — not an executed RED

The basis is the completion receive limit after held output in `layers/agent/src/event_node/mod.rs::EventNode::run` @ 96c90f99e, and
the receive limit after holding a nonACK in `layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/ack_service.rs::Worker::service_blocked_ack` @ 96c90f99e.
`entrypoints/agent/src/event_runtime/control.rs::create` @ 96c90f99e creates the broker input and the worker input
each with the same declared capacity. In the existing duplex test,
`layers/agent/src/event_node/tests.rs::DuplexProbeAdapter::try_offer` @ 96c90f99e always succeeds.

The candidate input is two nodes H/T, each queue with capacity1, a RELEASE for valid request R, a PHYSICAL for valid request Q,
additional valid PREFILLs H1~H4, and SESSION redeliveries C1~C6 with identical installed content. During R's finite native
RELEASE, T's input is filled with C1~C3 so that Q's send is held, and H's input is filled with H1~H4.
R's genuine RELEASED is also held in front of H. The candidate is the finite order in which T fills the completion queue with the C1 response,
becomes Full on the C2 response, holds C3, and fills the remaining input space with C4~C6.

| Space | H | T |
| --- | --- | --- |
| EventNode held output | PHYSICAL(Q)→T | RELEASED(R)→H |
| broker input / EventNode held input | H4 / H3 | C6 / C5 |
| worker input / worker held input | H2 / H1 | C4 / C3 |
| completion queue / Full send | Q observation / Q StageSpan | C1 SESSION_READY / C2 SESSION_READY |

The question to test is whether space can be created internally even after normal OUTER consumption and fair resumption of every task.
A forced shutdown, or the test granting room from outside, does not substitute for normal release completion.
This order has not yet been run on the real EventNode, broker and Worker, and it has not been proven that a pure PREFILL wave alone
reaches the same state. Before implementing, pin down reachability and the normal-progress oracle first.

## Real actor cycle counterexample — sealed before the fix (2026-09-07)

### Scope and oracles pinned before the run

Only a test-only actor_ring.rs and Cargo dev wiring are added on base HEAD `f13e2560b`. Production code
is not changed. Both tests deliver the same 14 original inputs (8 SESSION, 6 normal inference) to the real broker/node/
adapter/worker. Only native Frame handling and finite delays are fake, and they start from the post-LOAD state.
This is not a natural-language/llama/GPU/remote real-hardware run, nor a proof of deadlock freedom for every schedule.

| Required test | Expected before the fix | Independent verdict |
| --- | --- | --- |
| `event_actor_ring_saturated_normal_ingress_must_progress_without_external_dequeue` | RED on the last normal-progress assertion | cap1 real held owner/Full, match with the genuine RELEASED's pending operation, OUTER keeps draining, native progress0 |
| `event_actor_ring_same_normal_ingress_completes_with_capacity_eight` | GREEN | Same inputs and same output oracle, completes without external completion dequeue |

The cap1 held-owner table and the finite input order are exactly those of the candidate section just before. The test compares full Event equality
of the real acceptance/Full return, source/target/load/session/slot/incarnation/operation, R's slot not returned, and H1~H4
not accepted. It does not check only completion counts. After the separate external recovery, it checks that all 14 inputs
are accepted exactly once by the real adapter, token1000/position1/text/stop=length per request, native input
[(0,10)] once, one KV release on each of the two stages with leftovers0, and 8 SESSION_READY.

**Normal progress and external recovery are kept apart.** Whether normal completion happened is pinned first, after 100 fair node polls.
Real OS workers and a 1ms timer are used, so the 100 polls are not a logical-clock-independent proof. The owner observations of the actual wait cycle,
native state invariance, OUTER draining and the normal control are read together. After that, only the known C1~C6 SESSION_READY events,
at most 6, may be taken out externally and forwarded to the broker unchanged. PHYSICAL/TAIL/observations of other correlations
are not taken out to reorder them. Even if this recovery succeeds, it does not turn the normal-progress RED into GREEN.
The external review's "recovery with a single C1" from before the hardening is not cited as an observation on this source.

The native gate's 10-second expiry is a sticky failure and is checked with a separate fixture-expired assertion at each poll/completion.
A setup/finish over 3 seconds, a gate expiry, EventNode shutdown or a compile failure is not the expected liveness RED.
Drop first opens all native gates and joins the real adapter workers, but this is not a graceful distributed Drain.

### Verification batch and review of trial and error

Of the requested cap of 3 rounds, the earlier run12 was the first and this run13 is the second. Before the run, the normal, saturation and recovery oracles above,
the inputs and the document format are pinned, and the Rust/Cargo/fixture and document sources are sealed. The full run command is
`cargo test --workspace --no-fail-fast --locked`, and both actor tests are in the default list.
Unexpected failures are recorded separately, and the same batch is not rerun with changed expectations. This time there is no production fix,
so no fix-removal mutation is claimed. The raw data is kept as local evidence in an ignored path, and Git receives only
the regression tests, required wiring, contracts and this record. Long-term external re-inspection is still not met.

### Actual results of the second round — the one expected RED

400 Rust/Cargo/fixture inputs were sealed in `target/capacity-slice-20260907-13/`. The source SHA256 is
`4a353e02335162a53371df334f8bd55b53742745392178cfb99ec1dc8ddb49eb`. The input list and
all bytes/hashes were identical before and after the run, and the SHA256 of the 4 changed documents read alongside were also identical before and after. After the run ended, only this result
record and the index are added. The log is `workspace.log`, and the tally is `workspace-result.json`.

- Command: `cargo test --workspace --no-fail-fast --locked`.
- Time: 2026-09-07 04:29:42~04:32:06 UTC. cargo exit101, 57 summaries.
- Overall **1254 passed/1 failed/7 ignored**. staged lib480/1, run time 2.21 s.
- The only failure is the last `normal_progress` assertion in `actor_ring.rs` for the cap1 test above. The cap8 control PASSED.
- There was no native gate expiry, setup/finish timeout, EventNode shutdown or compile error. After recovery, the full 14-input/
  6-result/native release oracles all passed before the last assertion. The external recovery actually took C1~C6, **6 events**.
- Log SHA256: `a2889eef868a1b68ab732df88c7afd46ff4aa38416b0ad31848886eb318f9ce7`.
- The actual staged lib recompile log was compared with the executed EXE `p4_llamacpp_staged_adapter-60c4f56e88389385.exe`.
  EXE SHA256: `78aa35288537e0960ebe0a6dbe1e85ca3a9dd9ef325e916f81288cdfcf08c26d`.
  The EXE at verification time is preserved separately in the proof directory. Source, logs and EXE are all local evidence, not long-term retention.

The local tally tool's `expected_red_only` is false because that field looks for **the name of the earlier local ACK test**. This time
`workspace-any` preserved the full failure/exit as-is, and the actual single failure name and assertion above were compared directly.
false was not turned into PASS, and the failing test was not ignored. The tally tool is not being fixed or rerun.
Harness, C++, GPU, remote and mutation reruns are not part of this full Rust result.

### Code verdict and checkpoint

`EventNode::run` stops receiving completions after held_output and stops receiving from the broker after held_input.
`Worker::service_blocked_ack` stops receiving after a held non-ACK. With this ownership, if both directions
are saturated, no consumer can create space even if only timer wakes repeat. The run above is a counterexample that, with finite normal input,
observed reaching that state, stalling, and recovery with preservation; it is not a global proof of losslessness or deadlock freedom.

The fix belongs in the existing B2/B3 goal of guaranteeing follow-up space per causing work item. Extending the single-ACK exception is not chosen.
The distinction between ID pre-rejection and post-hoc intent preservation is stated in the batching contract's existing ownership section, and the dev-only neutral API/
tokio usage scope of the actor tests in the isolation contract. Production normal/build dependencies did not change.
**The required RED is preserved in a separate full commit without a production fix**, and the follow-up production fix starts after this commit.
The three rounds are not reset under a new name, and the final candidate check happens only after the redesign is closed.

## Reservation foundation for local completion storage — pre-run WIP (2026-09-07)

The base HEAD is `393a6c23e`. This is an **implementation/static review record**, not a source/binary seal or new run results.
This checkpoint preserves progress before verification and is not a report that RED was turned into GREEN.

### Changes and static verdict

- The neutral `node_adapter/event_cost.rs` destructures Event/Envelope/Endpoint/Address exhaustively.
  It is a checked sum of inline and independent String/Vec capacity, with no serialization or clone.
- `node_adapter/mailbox.rs` connects the real preallocated queue and the ordinary/reserved/owned count/bytes to
  the same ledger. It implements move-only reservations, returning the original + reservation on rejection, keeping the claim after dequeue,
  transferring responsibility after acceptance into a new real store, and returning the claim after the Event is discarded first.
- A single cost overrun is TooLarge, a single arithmetic overflow is CostOverflow, and a current shortage caused by other
  holdings is Full. The real worker publication match also does not retry permanent errors and
  returns the original Event to the caller. The failure/preservation limits of existing callers beyond that point were not solved in general this time.
- The lock order is Storage→Budget, and reserve releases Budget before entering Storage.
  Waker calls/destruction and Event/claim destruction happen outside the locks. This is a static path check by two reviewers, not an execution proof.
- Oracles for pre-native ID exhaustion on RELEASE, the existing prefix obligation, normal delivery of the last ID, and native partial failure were
  added to the existing real handle/native fixture. The RELEASE production code itself was not changed this time.

### Tests written but not run

| Scope | Written | Contract to judge |
| --- | --- | --- |
| Event retention cost | 5 | inline once, capacity of every field, nested independent allocations, spare Vec, checked overflow |
| Real mailbox reservation | 14 | count/bytes, cancel, wrong receiver, too-small, close, owned/transfer, lost wake, races |
| Real Worker publication | 1 | A permanent overrun returns the original allocation and is not misclassified as Full/shutdown |
| Real RELEASE handle | 4 | native0/preservation before rejection, normal comparison of the same Event, exact successor, partial failure fence |

The Worker permanent-error test makes even a wrong Full implementation terminate in finite time via the shutdown guard. It distinguishes the snapshots of a permanent error and
shutdown abandonment, so a wrong retry branch must become an assertion failure.
The retained-prefix case for RELEASE is a direct method path, not a claim that the current synchronous run loop accepts that command
during a flush. The normal comparison checks two unsorted owners and the exact Event/wire.

The inputs/expectations of the existing mailbox_tests.rs and actor_ring.rs have change0. The existing 1ms worker wait, remote serve/pump,
wire version, native result bound and product byte settings are unchanged. The count-only constructor is not a declaration of a product byte
limit. Passing a reserved front to a legacy consumer would stall, so the new reservation producer was not
enabled in production. Nor can progress be guaranteed by pre-reserving every cap1 slot for the multiple results of a single work item.

### Verification status and retention

Compile/unit/full/mutation/docs-lint/C++/GPU runs for this change were **all not run**. Only a pre-run
comparison of contract and code and formatting cleanup were done. The last fixed verification round was not used, and no current pass count is produced.
run13's 1254/1/7 is the result of that sealed source. The last round is not used to check this foundation API before the acceptance/return
wiring of the whole candidate is complete. No result logs or hash artifacts were added this time.
At the user's instruction, all non-ignored sources, tests and related documents are committed together as an **unverified WIP checkpoint**.
The current order and the first next action are owned only by the roadmap, and no completion of B3, overall deadlock, or final real-hardware runs is claimed.

## Pinned committed outgoing items — static review WIP (2026-09-07)

The base HEAD is `7f402aba5`. **Compile, tests, mutations and docs-lint for the current changes were not run**, and there is no new source/binary
seal or pass count. Only formatting cleanup and code comparison were done. run13's 1254/1/7 is the result of the pre-fix source.

### Scope of the change and rationale

- `effects.rs::Worker::flush_effects` builds the FIFO head into a complete Event once and, even on final failure, preserves
  the envelope/ID/sequence/payload together with the after-action. The existing Full loop itself already kept the original Event.
  This migrates the boundaries where only the body was returned or only the DTO remained after Closed/shutdown; it is not a discovery about Full behavior.
- `obligations.rs::CommittedEffect::event_count` is the share not yet issued. ForwardObserved's 1+N
  becomes next_event+1 and unallocated N after materialization. Subtracting 1 again from the active N during Full would let a diagnostic use the observation share,
  so it is not subtracted. The preservation of this sum and the per-attempt revalidation of the head ticket were statically reviewed independently.
- The broker makes the acceptance copy after securing the destination's real slot. Full returns the original Event including its spare capacity and
  allocation. The queue/ledger copy on success, not returning the original on Closed, and the global ordering domain are unchanged.
- The representations are split: an ID/serialization failure preserves the original DTO, and a send failure after Event construction preserves the complete Publication.
  The existing tests' inputs, ID boundaries, native/slot/fence and actual-received-item criteria were not changed. An independent reviewer pointed out that the full telemetry
  comparison and the Output envelope comparison could be lost during the migration, so both were reinforced before running.
- The release_notification test states explicitly that the first item after Closed is a pinned Event, and compares the full expected Event built from the original pending provenance
  with the wire. A failure before ID issuance still allows only the original DTO. It was not changed into a loose assertion that accepts
  either representation. The original early rejection and the post-commit rejection are still distinguished.

### New tests written and planned removal mutations — none run

There are 6 `publication_tests.rs` tests on the real `flush_effects`→completion mailbox, and 2 broker tests.
The test-only fence release / receiver swap is not a production reconnect API, and it does not synthesize native completion.

| Test/scope | Pinned verdict | Behavior to remove later |
| --- | --- | --- |
| final_forward_failures | Original Event/ID/allocation/FIFO on Closed/Full-at-shutdown/TooLarge, exactly-once delivery after manual resume | Regenerating a DTO instead of the Event after failure |
| reply_publications | Full reply/envelope/payload for Output/Receipt/Telemetry, re-consumption of already issued IDs0 | Reissuing the ID of a frozen Event |
| unallocated_id_failure | Whole intent preserved before issuance, Observed unallocated 1+N→N | Accounting a Publication as 1+N again |
| active_frozen_forward | On a real Full/invalid ACK, a diagnostic cannot consume the share of the N later observations | Wrongly subtracting 1 from the active N |
| accepted_forward | After a successful forward, observation failure/resume causes no re-forward and keeps timestamp/ID/allocation pinned | Restoring the forward / resetting the time on observation failure |
| native_precondition | On a native pre-authority rejection, the original intent, ungenerated suffix and fence are kept | Removing the intent / materializing later items on rejection |
| broker Full | The same original allocation returned 3 times, then real acceptance and Duplicate | Returning a clone instead of the original on Full |
| broker order domain | Order violations rejected even across different destinations of the same source/correlation; the correct order of the same input succeeds | Changing to per-destination ordering ledgers |

The Full/ACK tests close the mailbox at a test-only observation point after real consumption. A 5-second guard keeps a missing
observation point or regression from turning into an infinite wait, and reaching the real observation point is asserted separately. This is not a clock-independent liveness
proof. The existing checks for real native success/failure/unknown results and ACK retirement are kept by control_dispatch_effect_tests.

The migration of the existing failure representation was applied only to 4 test files:
effect_representation/observe/control_dispatch_effect/release_notification. Instead of the full DTO Debug, they compare the needed full wire Event and the full not-yet-generated suffix/observations.
The ID/delivery order/original allocation/settlement/native oracles that existed before the change are not removed.

### Boundaries not yet wired and limits of proof

This code is synchronous publication, not byte/native result reservation or actor input yielding. SESSION/error
direct responses, EventNode raw consumption, remote serve/pump, delivery grants and Cancel/Drain are still separate.
The cap1/cap8, 14-input/6-result and full recovery oracles in actor_ring.rs were not changed. The RED is not hidden by blocking all
normal acceptance or enlarging the completion queue. The product reservation producer is not enabled yet either.

In the static fan-out calculation, SESSION produces 1, a head issue produces 3 (PHYSICAL+BatchObservation+Span), and a tail physical produces
2 (TAIL+Span). A head TAIL produces per-stopped-owner OUTPUT plus native RELEASE and commands, and
after RELEASED, per-original-submission receipts are needed. Pre-reserving all of this in the cap1 delivery slot would keep even normal work from
starting, so effect retention space must be separated from transfer slots. These values are path calculations for that fake 2-stage fixture
and do not prove general model, real batch count or byte bounds.

A PHYSICAL and the later OUTER observations of the same source/correlation share ordering even if their destinations differ.
So this cannot be solved by bypassing destinations or with an ACK-priority lane alone. The next implementation order is owned by the roadmap.
The last verification round has not been used yet, and no new round is created just to check a partial API.
All non-ignored changes are committed as unverified WIP. No models, raw logs, one-off tools or binaries were added to Git, and
no remote/GPU/C++/push was run.

## Delivery queue and retention space for required results — static review WIP (2026-09-07)

The base HEAD is `bcbadf101`. The changes in this section have **not been compiled, run as tests, mutated or checked with docs-lint**, and
the 1254/1/7 of the pre-fix sealed run13 is not reused as the result for this source. The last verification round is also unused.
The existing actor cap1/cap8, 14-input/6-result and native/recovery oracles were not changed. No new long-term raw data was generated either.

### Code changes and static decisions

- The real mailbox's delivery queue_capacity was separated from retained count/bytes. The two existing constructors
  apply the count bound identically to both, and only the new `completion_mailbox_with_limits` takes separate limits.
  A reserved publication, on a real queue Full, also returns the original Event allocation and the linear reservation unchanged.
  An owned dequeue returns only the queue slot and keeps the retention claim. A transfer failure before successful receipt preserves both
  claims. This is not a receiver-side remote grant or a full acceptance contract for the actor cycle.
- Creating and then discarding a temporary reservation on an ordinary Full could produce a loop that wakes itself to retry, so
  queue admission and securing the ordinary claim were placed in the same push section, in Storage→Budget order. A permanent
  single-Event byte overrun is judged before queue Full. No new queue size or experiment threshold was chosen.
- `mailbox_group.rs` secures count+bytes for a list of known Event footprints in the same critical section.
  Each item, the sum and the actual array backing size are computed with checked arithmetic, and Closed/contention is rechecked after preparation.
  No active Claim is created before the budget commit, so an early failure does not return another owner's space.
  After commit, only claims are installed into the already secured array, with no fallible allocation or caller callback.
- Array capacity is charged separately from claims. The array remains even after group items are taken out, so the new group API
  is not allowed for count-only. Temporary arrays during concurrent preparation are outside this successful retention space bound, and this is not
  called a completed RSS budget. The correctness of the future Event bound is the caller's contract, and there is no real producer wiring yet.
- An independent static review found a double-call counterexample in per-item group notification: the first panic→unwind, then the next notification.
  This was pinned by returning the accounting for unused items/the array first and notifying only once at the end. The same path of the new owned
  dequeue was checked too, and a Claim only returns accounting during unwind. The first panic is not hidden.
  No guarantee is made about delivery/progress after a callback violation or about arbitrary RawWaker destructors being safe.

### New regression oracles and planned removal mutations — before running

| Real path | Written | Verdict / behavior whose removal must fail |
| --- | --- | --- |
| group reservation, sequential cap1 delivery | 1 |Atomically reserve 3 results in real storage, no mutation on Full, independent retirement, empty-array cost kept / recombining queue and retained |
| group input/capacity rejection | 3 |Empty list, last-item overflow, permanent TooLarge, count-only, no mutation on transient Full and re-acceptance of the same input / partial claim installation or missing array charge |
| group contention/close | 3 |close before the final commit, ordinary contention, only 1 concurrent group approved / removing the final check |
| group cleanup / owned dequeue callback | 3 |One notification outside the lock, other owners' claims preserved, first panic propagated with re-calls0 / removing the quiet cleanup or unwind guard |
| queue/retained real delivery | 3 |queue1/retained3, dequeue wake paired control, both claims on destination Full / returning the claim or replacing the original Event on Full |
| ordinary rejection, constructors | 3 |Full 3 times with snapshot/wake0, permanent TooLarge even when Full, queue/retained0 rejected / temporary claim self-wake, reversed check order |

The new tests are 10 in `mailbox_group_tests.rs` and 6 in `mailbox_queue_storage_tests.rs`. They compare original Event equality,
payload allocation, permit identity and the actual storage snapshot together. The callback counterexample
panics only on the first call, so a removal mutation shows up as a failed re-call count assertion rather than a process abort.
The 5-second channel guard in the concurrent group test exists to report a run failure in finite time; it is not clock-independent progress evidence.
The mutations in the table are also **planned**; they have not been run and no pass is reported.

### Current limits and Git inclusion decision

The changes are limited to the backend-neutral `p4-adapter` storage, its real-path tests, and the ownership contract/roadmap/evidence index.
Product producers/owned consumers using the new API, broker dedupe byte cost, remote acceptance grants, native
variable result bounds and an integrated input/capacity/shutdown pump are unfinished. wire/llama/native/models were not changed.
The current HELLO row/seq limits and the frame receive cap are not misread as a preallocated memory bound for native output.
This cannot be promoted to resolving the full cyclic deadlock or to a final wave result. The next order follows only the latest roadmap record.

Only the implementation to keep, the regression tests, and the concise contract/progress records are included in the full WIP checkpoint. Generated logs, duplicate
manifests, one-off tools, models and binaries stay in the existing ignored paths. The new test files are required oracles, so
they are not ignored, and the B8 condition of re-inspecting raw data on another machine is still not met.

## Up-front atomicity for wiring PREFILL acceptance — static review WIP (2026-09-07)

The base HEAD is `d8fff7d2712de2bd90daed4c0de8292662761246`. **Code-level rejection transitions** found while reading
the real producer/owned consumer wiring were separated out first. This time compile, tests, mutations and docs-lint were not
run. There are no new sealed sources/binaries/run logs or pass counts. 2 verification rounds used, the last 1 unused.
run13's 1254/1/7 is attributed only to the pre-fix source sealed at that time.

### Pre-fix path and the fix point

`worker.rs::Worker::prefill` @ `d8fff7d27` checked Tokenize/context/incarnation after remembering the session key.
There was also a path where `admit_pending` rejected after the request was inserted, the incarnation incremented and pending appended.
So wiring the ordinary request handler directly into the acceptance path during saturation would leave the memory/request behind after a failed
admission. The counterexample in this section is **static path analysis, not an executed RED**. Pre-run and removal-mutation proofs remain.

This code virtually appends the new request behind the existing pending entries, checks the whole assignment prefix for this round, and makes the first admission write only after
incarnation, Tokenize and context have all passed. It uses the same validator as the existing ACK prefix check,
without widening the ACK's input or check scope. The committed section has no new Result rejection/handler/yield/publication,
and the ADMITTED record comes after commit. The change in error priority and the resource scope not guaranteed follow the
[L2 ownership contract](../../../../../../../docs/adapter-batching-layers.md#l2-acceptance-and-occupancy-admission).

### 7 real consumer oracles written — not yet run or mutated

`prefill_admission_tests.rs` was wired under the existing SESSION fixture in `worker/session_tests.rs`.
The fixture has no native installed, so apart from the Tokenize request rejection, the tests go through the real prefill/handle with
token input. Events are encoded/decoded with the existing wire codec. Existing test contents/expectations were not changed.

| Test | Rejected/normal input and pinned verdict |
| --- | --- |
| context_refusal | At context32, prompt32+max1 is rejected; the same request is accepted normally with prompt31+max1 and a different session key |
| zero_or_exhausted_incarnation | incarnation0/MAX both rejected without mutation; the same input is accepted once only the value is fixed to7 |
| invalid_free_slot | With a new candidate after a valid existing pending, an out-of-range/duplicate free id rejects the whole prefix; fixing only the slot gives FIFO assignment |
| a_free_slot_that_is_still_owned | Putting an id owned by an existing request back into free gives double assignment0; after fixing free, the existing and new requests own separate slots |
| an_invalid_later_pending_member | If a valid first pending is followed by a missing/already-owned second, the first request is also unassigned; after fixing the cause, 3 FIFO assignments |
| tokenize_failure | Admission preserved on a real tokenize→Empty lifecycle.request rejection; token input for the same request accepted |
| two_available_slots | With 2 pending + 1 new and 2 free, the existing 2 are assigned first and only the new request waits; original Event and incarnation kept |

Each rejection checks together the admission/ledger snapshot and unchanged next_event of the direct call, exactly 1 ERROR
Event and next_event+1 from the real handle, and resubmission after fixing the cause. The status string of the ordinary handle changes to
`failed:<detail>`, so this is not called **full Worker invariance**. The private session_key_order and
the record file output itself are not checked, and global environment variables are not manipulated. The Tokenize case is not native
parser/GPU failure injection or Loaded recovery. The successful token path is also the native-less fixture as-is.

The planned mutations are: moving session key memory/ADMITTED back before validation, moving FIFO validation back after request insertion,
removing the check on later members of the assignment prefix, and assigning the new request before the existing pending entries. Since the current oracles do not observe the record file,
we do not claim they detect a mutation that only changes when ADMITTED is output. At minimum, the early key
memory and post-insert rejection mutations must fail the real consumer's state preservation assertions before this can be promoted.

### Audit of the real wiring paths and scope

An independent static review compared test module visibility/types, the ERROR envelope/ID/detail, the lifecycle
state of Empty Tokenize, and the same prefix check. This is not a substitute for compiler or run results.

- Raw Event receipt remains not only in broker/node but also in `entrypoints/agent/src/event_runtime/{mod,control,transport}.rs`,
  the transport ConnectionSender, and the adapter WorkerInput/held_input. If only the producer is switched to reservations,
  or a claim is dropped at an intermediate raw bridge, the consumer's retention space is not tracked.
- On success, the broker makes a copy for the queue and an exact-duplicate ledger. Attaching the source claim to the duplicate ledger would tie up the producer
  until normal count-window retirement. An independent duplicate cost and retirement outside the ledger lock in the callback are
  needed. A case where bytes are permanently short even after the count window must not be made to wait as destination Full.
- The real remote serve ends the connection on dispatch failure, and the outbound/outer pumps and connection writer
  have not yet been migrated to local owned claims / remote acceptance. Completing a socket write is not evidence of receive space.
- The current fix does not create byte grants, per-cause required output reservations, a blocked worker pump, or a native result pre-bound.
  The actor cap1/cap8 input, completion, native and external recovery oracles are unchanged, and no deadlock resolution is claimed.

The roadmap is the sole owner of the next implementation order. This change preserves only the 2 production function files, the new required regressions/wiring, and the related
ownership contract/progress/evidence as a full unverified WIP. Generated proofs, models, binaries and temporary tools stay in the existing
ignored paths. No remote/GPU/C++ runs or push were done, and there is no promotion of final wave results.

## Original ownership on real delivery rejection — static review WIP (2026-09-07)

The success/failure consumption boundaries were traced at base HEAD `2b1d1d5398e182eeb0a6532f384ce8203257918c`.
The existing `EventBroker::dispatch` returned the original Event only on Full and consumed it otherwise, and the EventNode terminal
also discarded an Event already held in the other direction. The spawned task in `control::create` logged the error and then discarded
the result. This fix covers **these real rejection returns and consumption paths**; wiring space claims into the success path is not done yet.

### Implementation and static comparison

- Every broker Err returns a move-only `DispatchFailure { error, event: Box<Event> }`. The existing order validate→
  exact duplicate/sequence→destination→real queue slot→success ledger commit is kept.
  register/unregister have no input Event, so they remain a pure DispatchError. Behavior classification and wire do not change.
- The real llama `try_offer` returns the original Event even when the sender is absent/Disconnected. Full returns the same
  value as before. The EventNode terminal hands over both held originals via `EventNodeFailure`. It does not copy the payload
  to return instead, or reclassify the failure as Full. Box only reduces the inline size of the failure return value.
- The product `NodeOwner` keeps `JoinHandle<Result<(), EventNodeFailure>>`, and the spawned task borrows the reason
  to log it and then returns the result. This is retention within the lifetime of that handle, not a durable outbox or restart recovery.
  DELETE's abort/handle discard and process shutdown, and draining unread queues and adapter-internal work, are separate matters.
- The final discard in the control reply loop and remote serve/pump remains. That gap is not hidden on the grounds that
  the new broker error carries the original. Display/operational logs print only the reason and do not dump the full user payload.

An independent static review compared error kinds/callers, nested test access, preservation of the original allocation by Box, poison guard lifetime,
and the correlation/sequence of the positive controls. It is not counted as a substitute for compile, run or mutation evidence.

### 9 regressions written — none run

| Path | Count | Pinned verdict / planned removal mutation |
| --- | --- | --- |
| canonical broker dispatch |4| For invalid envelope, missing/stale/Closed, conflict/regression and poison: original Event/value/allocation/cost and full ledger unchanged, real acceptance after correcting the cause where possible / substituting a clone, committing the ledger up front |
| actual EventNode loop |3| Original allocation returned for adapter Closed input, broker Closed output + already held input, and completion Closed held input / discarding either direction at terminal |
| actual LlamaNodeAdapter::try_offer |2| Original returned for sender None/Disconnected, exactly-once acceptance of the same Event after Full / consuming on Closed, returning a copy on Full |

The 4 broker tests are in `event_broker/failure_tests.rs`, the 3 node tests in the existing `event_node/tests.rs`, and the 2 adapter tests in
`node/offer_tests.rs`. The migration of the existing test API kept the exact failure causes and raw comparisons. The real actor
`ObservedAdapter` also forwards the same canonical type, and the 14-input/6-result/cap1/cap8/timeout/normal-progress oracles
were not changed. We do not claim that the original actor RED has become GREEN.

The node tests trigger completion/Closed after confirming via a real loop poll that the input is already held. They compare the allocation actually moved into the receive
channel and do not confuse the value copied on broker success with the caller's original.
The 1-second guard is for reporting failure in finite time, not proof against every CI delay or of clock-independent liveness. Clearing poison is
the test's correction of the cause, not automatic recovery in production. The product lifetime of the NodeOwner handle was checked in code, but
full CREATE/DELETE consumer tests and automatic recovery verification are not within the scope of these 9 new tests.

### Verification status and remaining boundaries

Compile, tests, mutations, docs-lint/C++/GPU/remote were **not run**. Only formatting cleanup and diff comparison are done, and no new run
source/binary seal or result count is produced. The last run13's 1254/1/7 is attributed only to the pre-fix source.
2 verification rounds used, the last 1 unused, and the last round is not spent on checking a partial API before a complete candidate exists.

Owned successful delivery still needs not only producer/held/destination/WorkerInput but also long-term retention of RequestState originals,
an independent exact dedupe cost and callbacks outside the lock, pre-reservation of causal required results/returns, and blocked worker waiting.
Remote must distinguish local acceptance / socket written / receiver acceptance, and the current harness's
multiple nodes in one agent are not used as inter-agent outbound verification. The final goal and next order are owned by the roadmap.

Only production code, real consumer regressions and ownership documents are included in the full unverified checkpoint. Generated proofs and one-off
tools, models and binaries stay in ignored paths and are not pushed. This is not completion of byte admission, the full deadlock or real-hardware TPS.

## Direct response FIFO and notification boundaries — static review WIP (2026-09-07)

Base HEAD `658c9cded723b17d8d1b422748f3c8ff733406cd`. This change is **not a fix fitted to run results but a static comparison of
the real ownership/ordering/shutdown paths**. Compile, tests, mutations and docs-lint were not run, and there are no new
run logs, pass counts or sealed binaries. The pre-fix run13's 1254/1/7 and the status of 2 verification rounds used / the last 1 unused
are unchanged. Static comparison does not replace execution proof, and this checkpoint is unverified WIP.

### Actual code changes

- `EventBroker::dispatch` moves the original allocation into the success queue and keeps an independent copy for exact dedupe.
  The failure and duplicate ordering checks are kept. It is a raw queue, and notification still happens inside the ledger lock, so this is not product completion of owned
  acceptance / out-of-lock notification. The claim is not attached to the duplicate copy, which would tie up the producer until retirement.
- The real mailbox enqueue was separated from the subsequent reader/capacity notifications. The existing immediate publication/transfer also
  uses an explicit notify after the same enqueue. The accounting and visibility constraints on Drop/callback panic are owned by the batching contract.
  The broker does not use this deferred API yet, and the new notification API alone does not release the cyclic wait.
- Direct LOAD/SESSION/UNLOAD/error responses go unnumbered intent → pinned Event at the FIFO head → real mailbox.
  The observation suffix of an earlier ForwardObserved gets its number before the later response. All diagnostics are preserved first so that the first Closed
  does not erase the batch diagnostics of later participants. The synchronous wait on Full is unchanged.
- Even if a native failure has already raised the fence, a new shutdown diagnostic alone can be sent when there is no existing effect prefix.
  If there is a prefix, the diagnostic is only preserved behind it; existing effects are not replayed and the fence is not released. The existing real
  native failure ERROR1 test is unchanged. When sending the diagnostic fails, the original snapshot cause is preserved too.
- The wire preparation round-trips at the maximum issuable ID width. This can reject boundary sizes that fit with the current short IDs,
  so it is not a simple refactor. Rejection before SESSION authority/ID writes and a normal small input are checked
  together. An unrepresentable error preserves the original and the failure reason as `UndeliverableDirect` and requires 0 IDs.

### Consumer oracles written — no runs or removal mutations yet

| Real path | File / conditions to check |
| --- | --- |
| broker success / exact dedupe | The existing Full retry test strengthened through the success allocation; 1 new test checks the original agent/node/outer/outbound allocation, receiver tampering, an independent receipt, and no re-publication of duplicates |
| actual mailbox | 9 in `mailbox_deferred_tests.rs`: immediate/reserved enqueue, transfer, Full/permanent rejection, receipt Drop, reader/source callback panic, publisher shutdown, dequeue before notify; the move-only compile-fail example was not run either |
| actual direct emit/SESSION/FIFO | 8 in `direct_emission_tests.rs`: no overtaking of deferred observations, full Event/allocation on Closed, future ID width, real number exhaustion, empty-prefix diagnostic / existing prefix preserved, all failed owners preserved after the first Closed, atomic rejection on total ID shortage / normal comparison with a single diagnostic |
| existing SESSION handle | The same 140,000-character ID input of `handle_undecodable_ready_does_not_install_session_authority` kept; a successful malformed ERROR publication corrected to no publication, no ID consumption, diagnostic preserved, and fence |

The new SESSION fixture is a real post-LOAD state but does not install a native server. The PHYSICAL at the front of the FIFO is
for codec/order comparison and is not native execution evidence. The in-test publisher swap / fence release after Closed only
compares the same preserved Event; it is not an operational recovery feature. The future ID boundary is computed arithmetically from the codec envelope length,
without searching for the failure length. The number of normal requests and the queue capacities in the broker/node/actor test inputs are not reduced.
There is no dedicated reentrancy oracle for a Direct append during an active publication, and caller-lock reentrancy in the source capacity callback
is not a separate new oracle either. These two are not counted as included in the real passing paths.

The planned removal mutations are: moving a copy instead of the original into the queue, early callbacks during enqueue, returning source accounting twice,
bypassing the direct response FIFO / issuing the ID early, discarding later owners on the first batch diagnostic Closed, releasing the native fence entirely,
and allowing a malformed diagnostic to be published. None of these mutations is claimed to have run this time. The existing actor cap1/cap8
verdicts on 14 inputs/6 results/native/external dequeue0 were not changed.

### Next wiring and Git inclusion scope

An independent static review compared the broker receipt and original allocation, mailbox notification/Drop/panic, and direct shutdown diagnostics against the
intent of the existing regressions and the difference in acceptance scope. A table of the required wiring for the canonical owned success path was
recorded in the batching contract. Claims must not be dropped at raw compatibility bridges, and long-term RequestState, release provenance and parse copy costs
must be tracked as well. Terminal consumption in control/transport and remote acceptance are still unfinished.

Only the production code to keep, the required regressions and the ownership documents are included in the full WIP checkpoint. Generated proofs, temporary tools, models and
binaries stay in the existing ignore. No new protocol documents were created. The remaining order follows only the latest roadmap record,
and C++/remote/GPU/model runs, push, and promotion of final wave results are outside this scope.

## Immutable sharing of request inputs — static review WIP (2026-09-07)

Base HEAD `f5aa09675119f377a92accfb9fe67b87eeef07a0`. This content is a comparison of ownership/lifetimes in the real code and
oracles that were written; it is **not execution evidence**. Compile, tests, mutations and docs-lint were not run. The pre-fix
run13's 1254/1/7 and the status of 2 verification rounds used / the last 1 unused are unchanged. There are no new result counts or sealed binaries.

### Premises before the change and the limited change

- `worker.rs::Worker::handle` cloned the whole Event for each branch and passed it to the handler. The blocked ACK
  path made the same copy. Now the original stays with the caller and the handler borrows it. The original lifetime of failure diagnostics and held_input,
  and the ACK no-flush/no-native ordering, are unchanged.
- `node/state.rs::RequestState` directly owned command (tokens/options)/template/reply, so
  the candidate clones in the real prepare_issue/tail/settled also copied large inputs. They are now bundled into a RequestInput behind a private Arc,
  with a read-only Deref. Progress state is the existing independent candidate, and there is no mutable accessor for production.
- The template and batch_events in `worker/drive.rs::Worker::drive_one_batch` each copied a full Event on every issue.
  It now holds a SharedRequestInput, and observation/error publication refer to the original template.
  `emit_batch_errors` prepares diagnostics for all existing owners in the same FIFO via Borrow<Event>.
- The base of the PHYSICAL/RELEASE/SETTLE follow-up effects is a clone of the same ingress envelope. Tail output and
  PendingRelease still use the authority of the original RequestState/template. They were not switched to the current ACK source.
- PREFILL's `RequestState::new(command, event.clone(), ...)` still performs 1 copy at entry.
  This is not a linear claim transfer of the raw original. ReadyRows/continuation/RowOwner/native result copies and initial
  parse costs remain, and no zero-copy, full completion of T19/T45 costs, memory bound or actor GREEN is claimed.

Direct writes to immutable fields in existing fixtures were migrated to test-only COW, and temporary fixture field moves to explicit clones.
The original inputs, injection counts, assertions and rejection reasons were not changed. The Clone on RequestInput itself exists only in test
builds. These test-only copies are not mixed into figures for removing repeated copies in production.

### 3 new oracles — none run

The following verdicts were written in `node/issue_tests.rs`. Equality alone would also pass the earlier deep clone, so
identity of the Arc and of the tokens/payload/options/reply allocations is also required.

| Test | Consumption and verdict |
| --- | --- |
| `actual_issue_candidates_share_input_through_refusal_and_acceptance` | Real prepare→rejection of a wrong split accept→normal accept; original/candidate allocations shared, values unchanged, whole state preserved on rejection |
| `a_later_invalid_issue_member_preserves_every_original_input_and_progress` | The whole prepare is rejected on a wrong owner after an earlier candidate; original input/progress unchanged, no leftover candidate owners; normal re-preparation once the cause is fixed |
| `shared_input_outlives_independent_progress_and_retires_with_its_last_owner` | Settlement candidate progress isolation and preservation on rejection, separation by the test COW, lifetime while a separate read owner remains, and the last Drop |

These three tests consume the real issue prepare/accept and RequestState settlement, but they are not memory-measurement or progress tests of the full real Worker::run.
Handler borrowing and drive are targets for the next run of the existing consumer tests, and
are not counted as proven by a new run this time. Removal mutations that turn the shared clone into a deep clone or let a candidate modify the original input
are only planned and not yet done. The original actor's cap1/cap8, 14 inputs, 6 results, external dequeue0,
native and timeout oracles were not changed.

### Static comparison and checkpoint scope

An independent static review compared source/target authority in every changed handler, borrow lifetimes, the ACK and native transition order,
whether drive owns the original even while self.state is changing, and the equivalence of the fixture migration. No production DerefMut/
raw Arc/COW bypass was added. Finding no blockers in static review is not a compile or run guarantee.

The product's current queue_capacity/completion_capacity are count declarations and cannot replace a byte/retained/receipt budget.
The contract-owning documents do not define numbers, defaults or a policy for undeclared cases, so no arbitrary bound was made from row counts or queue counts.
This is not written up as product ResourceBudget completion or owned success wiring.
The next run order is owned only by the roadmap. The sources to keep, required regressions and ownership documents all remain as WIP, and generated artifacts stay in
the existing ignore. C++/model/GPU/remote runs, push, and performance / final multi-computer promotion are outside this scope.

## 2026-09-07 stop record — documenting the current state without new verification

The user asked to stop advancing the implementation and to summarize the verified progress and the remaining work. From here on only documents are updated,
with no new source edits, compiles, tests, mutations, deploys or real-hardware runs. The owner of the current summary and the resume conditions is
[roadmap §0](../../../../../../../docs/distributed-batching-roadmap.md#current-status).

### Actual material rechecked

- Git HEAD `6fe10eb104b783f6ea5cacbc2e549a0a7e504677`; 7 WIP commits after the RED-preserving `393a6c23e`.
  For this range, `git diff --stat 393a6c23e..6fe10eb10` shows 55 files +7589/-709; this is not an amount of executed results.
- `target/capacity-slice-20260907-{09,12,13}/workspace-result.json` was reread.
  The first two are 1253/0/7 each, and the last is 1254/1/7 with exit101. Results from different seals are not summed.
- The run13 log was checked for passes of T10/T11/T12, the real simulator malformed arrival, the genuine ACK and speculative
  consumer tests. These are correctness progress at that time, not verification of the current WIP.
- `target/ack-service-mutations-20260907-01/verification.json` records the 25-test baseline/restored 25/0,
  ACK service removed 20/5, the other 4 mutations 24/1 each, and actual recompiles. It was not rerun this time.
- In run13, only the last normal-progress check of cap1 failed. cap8, and passing the 14-input, 6-result,
  8 SESSION_READY and native settlement oracles after the external recovery of the 6 events C1~C6, are not turned into a normal-progress success.

### Independent completion candidate written before the stop — all unverified

The actual fix candidates are `EventNode::forward_independent_front`,
`EventBroker::{reserve_completion,dispatch_completion}`, `EventLedger::inspect_completion_header`,
`NodeAdapter::{peek_completion,try_take_completion_matching}`, and the mailbox/Llama delegation.
For an ordinary front with a different source/correlation, the real destination slot is secured and then the original is moved;
the same ordering domain, Full and a changed front are kept as-is. This includes the exact receipt pin and returning the three originals at terminal.
This static design is different from a new queue limit, a SESSION-only rule, or a full owned/byte guarantee.

The default regressions written are 12 for the broker, 4 for the mailbox and 4 for EventNode. The real API was also wired into the actor wrapper, and
a preventive witness was added that requires, at the same time, capacity0 while head has not been polled, the genuine RELEASED pending identity, and the real OUTER arrival of C1.
This is **code that will check arrival**, not an observed arrival result.
The existing cap1/cap8, 14 inputs/6 results/8 responses, the separation of normal progress from external recovery, and the timeout distinctions are to be kept.

The planned mutations — removing independent progress, removing the same-order check, removing the front-match check — were not run.
There is no compile, full pass, actor GREEN or source/EXE seal for the new candidate. The verification budget is 2 rounds used / 1 unused, and
it is not restarted or consumed because of this documentation. Both root and the static reviewers stopped further runs.

### What preservation means

What is preserved is the production candidate already written, the required regressions, and the ownership documents including this one. The preservation commit is
an **unverified stop snapshot**, not a feature completion commit. After the implementation stop, code expectations, capacities and inputs are not
changed. Generated source copies, logs, EXEs and models stay in the existing ignored paths and are not added to Git.
Long-term retention that would let the local raw material be re-inspected on another machine is still not met. There was no external upload or push this time either.

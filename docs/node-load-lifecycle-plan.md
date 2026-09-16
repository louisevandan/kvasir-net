# Implementation plan: folding node lifetime into LOAD and UNLOAD

Written: 2026-09-15 KST. Status: **completed implementation plan that fixes the user agreement; M4 acceptance complete**.
Code audit baseline HEAD: `c16cbfa2abe8e7bc0bd7ce4f3e4c568f6a6c0569`.
When this plan was written, shared adapter retention, transport, INSPECT and llama/HF had separate uncommitted changes.
This document does not mean the current implementation or tests pass. The overall execution order and progress status
are owned by the [roadmap](distributed-batching-roadmap.md#current-status).

## 1. Agreed goals and responsibilities — do not redesign

- **A node is a loaded instance of a model or of the model segment it serves.** Do not create empty nodes in advance, and do not leave them behind after a normal unload.
- A resident agent exists even without a model. The GPU, the physical machine and the model file are not the node's identity.
- Loading two models on the same GPU gives two nodes. Loading the same model twice, independently, also gives two nodes.
- The external caller supplies the node ID. LOAD rejects an ID that is already in use on that agent.
  "In use" includes instances that hold resources while loading, running, releasing, or with unknown reclaim status.
- The ID check and registration run atomically. Do not introduce a one-node-per-GPU limit, a ban on duplicate model paths,
  a global UUID issuing service, or a rule that permanently exhausts IDs.
- Keep the existing node generation and adapter load generation checks. The duplicate-ID decision is based on current ID occupancy,
  independent of generation. When the same ID is reused after release, follow the existing broker's new-generation rule.
- The P4 external API completes node creation and removal with **individual LOAD and UNLOAD** only. It does not require a separate CREATE or DELETE.
- **OUTER coordinates the multi-node load of a whole model.** OUTER is responsible for issuing the individual commands, judging overall success,
  UNLOADing the nodes that succeeded when any one is rejected or fails, and tracking and reclaiming nodes that are still loading or whose result is unknown.
- P4 is responsible only for the individual load and release on its own agent and for returning the exact result. It does not
  automatically reclaim another agent's nodes, and it does not build whole-model commit/abort, 2PC or a global deployment manager.

## 2. Scope and what completion means

### Included

1. In the current event runtime, fold CREATE's duplicate check, registration and adapter construction into LOAD.
2. Fold native/worker resource release, event ownership cleanup and node route/owner removal into UNLOAD completion.
3. A backend-neutral lifecycle command and completion contract, implemented for both llama.cpp and HF.
4. Remove CREATE and DELETE from Rust event-drive, the HF Python callers and the current acceptance scripts, and move their result checks over.
5. Tests for INSPECT, rejection/failure results, the existing retained/receipt/resource-profile protections, and the real consumption path.

### Separate work

- Automatic reclaim of multi-node failures in OUTER is follow-up work owned by OUTER. This plan migrates the calling API and
  states that responsibility. Do not fill the existing gap with a P4-internal feature or record it as if it were already implemented.
- Batching policy, KV operations, native model computation, distributed performance work and a general failure-recovery platform are out of scope.
- Do not confuse `Agent::create_node/delete_node` on the old service path with the current event path.
  Investigate whether any real product calls remain, but do not make deletion of the whole unrelated legacy API a precondition of this work.
- This document grants no new authority for remote deployment, killing existing processes, or pushing.

Completion means the normal path is `LOAD → SESSION/request → UNLOAD`, and at the moment the last response is received the node and
its native resources have been removed. Receipt of the actual remote response and retirement of the transport receipt are observed separately.
Passing the small conformance suite does not replace [final multi-machine acceptance](distributed-batching-verification.md).

## 3. Procedure for starting a new session

1. Read [AGENTS.md](../AGENTS.md), the [roadmap](distributed-batching-roadmap.md),
   the [verification convention](distributed-batching-verification.md), the [isolation contract](layer-isolation-contract.md)
   and the [document map](document-map.md), then read this document to the end.
2. Check HEAD, branch, all dirty/untracked files and the execution path of `F:\dev\p4`. If it differs from the baseline HEAD,
   audit the differences in the paths below first. Do not revert already-implemented parts to old code.
3. Work that overlapped at the time of writing: `RetainedNodeAdapter::retention_snapshot`, transport cost/remaining capacity,
   INSPECT, and request/native buffer retention in the llama worker. Integrate the latest version of those contracts first.
   Do not copy stale trait definitions, and do not remove the resource profile checks or reservations.
4. Do not touch other authors' changes or measurement arms that are running. If work runs in parallel, work in an independent checkout
   with no conflicts and record the integration baseline of that change. Do not restore the user's working tree with reset/checkout.
5. The first step is the §4 call-path audit and a deterministic pre-review of the §8 tests. In a session that was asked only for documents,
   do not start implementation or model runs for testing. A new session that is given implementation instructions follows the work order in §7.

```powershell
Set-Location F:\dev\p4
git rev-parse HEAD
git branch --show-current
git status --short
git diff --stat
git diff c16cbfa2abe8e7bc0bd7ce4f3e4c568f6a6c0569 -- entrypoints/agent/src/event_runtime layers/agent/src/event_broker layers/agent/src/event_node layers/adapters/adapter/src/node_adapter tools/event-drive/src/run
```

## 4. Baseline code and change locations

These are the observed code locations. The changes in the table are implementation goals.

| Location | Current role / required change |
| --- | --- |
| `entrypoints/agent/src/event_runtime/control.rs` | Owns CREATE/DELETE and `NodeOwner`. Move to LOAD acceptance, duplicate check, per-node asynchronous lifecycle task and final removal |
| `entrypoints/agent/src/event_runtime/adapters.rs` | Supported kinds and factory. Use the same list to advertise LOAD support and to actually construct; reject disabled kinds up front |
| `entrypoints/agent/src/event_runtime/control/inspection/` | Observes registered nodes and task/retention. Distinguish loading/unloading and reclaim failure; remove from nodes after a normal release |
| `entrypoints/agent/src/event_runtime/transport.rs` | Ingress, retained send, reconcile, actual cost. Preserve the return context and the existing B1/B2/B3 cost contract |
| `layers/protocol/src/event/` | Candidate place for the codec and constants of the shared lifecycle request/result. Do not change the existing P4E3 envelope or hop wire unnecessarily |
| `layers/agent/src/event_broker/{mod.rs,retained.rs}` | ID registration, generation, ingress fence. Used for LOAD's atomic occupancy and the UNLOAD shutdown barrier |
| `layers/agent/src/event_node/retained.rs` | Actual input/output delivery and ownership of held Events on failure. Deliver lifecycle completion to the owner and preserve remaining ownership |
| `layers/adapters/adapter/src/node_adapter/mod.rs` | Neutral `RetainedNodeAdapter` boundary. Add a typed lifecycle result; do not use the snapshot string as a completion signal |
| `layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/{control.rs,shutdown.rs}` | Actual LOAD/UNLOAD and busy check. Bind the current "set unloaded, then build the response" order to typed completion |
| `layers/adapters/llamacpp/staged/adapter/src/v2/node/{retained.rs,worker.rs}` | Actual adapter and worker shutdown/retention. Connect the release points of native resources, responses and claims |
| `layers/adapters/hf/adapter/src/{construction,lifecycle,retained}/` | HF actual LOAD/UNLOAD/abort and Python child lifetime. The separate [HF rules](../layers/adapters/hf/AGENTS.md) apply |
| `tools/event-drive/src/run/{mod.rs,load.rs,replies.rs,config.rs}` | CREATE all → per-node LOAD → SESSION, UNLOAD all → DELETE. Move to the new individual lifecycle API with exact result checks |
| `layers/adapters/hf/python/p4hfadapter/models/qwen3_5_0_8b/event_pipeline/__init__.py` | Per-node CREATE/LOAD and shutdown/close. Move to the new lifecycle API; do not bypass through model operations or a direct worker path |
| `layers/adapters/hf/scripts/verification/`, `test/benchmarks/`, `tools/` | Find current callers and fixtures, down to dynamic `node.{op}` strings, and migrate them. Do not modify dated historical artifacts |

Rust LOAD uses `agent_load_waves` to run in parallel across agents and sequentially within one agent. Build compatibility is checked
after all of them succeed. If `load::drive(...).await?` in `execute` fails, the later teardown is never reached.
HF also sends individual commands. This missing partial-failure reclaim is an OUTER problem and is distinct from P4 lacking a whole-model load feature.

## 5. Target command contract

This section is a proposed specification for implementation. M1 fixes it with a codec and literal fixtures and reflects it in the current
[event contract](event-protocol-v2.md). Do not use it as if it were an existing command.

### 5.1 External lifecycle requests

- Both LOAD and UNLOAD target `Endpoint::Agent(the agent in question)`. Before LOAD there is no node, and
  the agent must still be responsible for the UNLOAD result after removal.
- New content types: `application/vnd.p4.node.load-v1`, `application/vnd.p4.node.unload-v1`,
  and for the result `application/vnd.p4.node.lifecycle-result-v1`.
- Payload format: `metadata_len: u32 little-endian` + UTF-8 JSON metadata of that length + the remaining opaque adapter bytes.
  The metadata carries `schema:1`, `node_id`, `node_generation`, `adapter_kind` and `adapter_content_type`.
  LOAD also includes the queue/completion/retained capacity and retained bytes settings that CREATE used to carry.
  Adapter load generation, the actual plan, binary, device, HF job and so on stay inside the opaque bytes in the existing adapter format.
- The metadata limit is stated in the M1 literal fixture. Length overflow, truncation, required fields and unsupported versions are
  checked before any construction or native effect. The actual retained-byte limit applies to the whole payload and to internal delivery.
  The existing LOAD resource profile and transport/edge/receipt checks still run unchanged on the real consumption path.
- Internal delivery preserves the original request's OUTER source, return route, correlation, deadline and causal identity, and
  binds the target to the node endpoint it created. It does not bypass HF's Outer source/topology check.
  This delivery does not re-register the same broker event ID with a different payload. If a derived Event is needed,
  it states a new event ID and the original request's causation, and binds the original cost ownership.
- Ordinary SESSION/inference/settlement Events go to the existing node endpoint. A node is not created automatically because an ordinary Event
  arrived for an unknown node. External node-target LOAD/UNLOAD bypass paths are blocked.

### 5.2 Completion result and retransmission

- The result sender is the agent in question. The result carries node ID/generation, `operation:load|unload`,
  `status:succeeded|rejected|failed`, `resource_state:absent|present|unknown`,
  the first error and the cleanup error, and the content type/opaque bytes of the adapter result.
- Final success means LOAD is ready to execute, or UNLOAD has finished removing resources and the node. A plain receipt ACK or
  a successful socket write is not returned as lifecycle completion. OUTER's source/causation checks change accordingly.
- Transport retransmission of the same Event follows the existing receipt/duplicate-suppression contract. If a new LOAD command arrives with an ID in use,
  it is rejected even for the same model/plan. Do not issue a new UUID internally or overwrite the existing node.
- A new UNLOAD for an ID that does not exist is stated as `rejected/resource_state=absent`. Do not guess at past success.
  If delivery of an already produced original result is uncertain, track it with the existing transport reconcile/INSPECT.
- Do not build a global deployment ledger or an unbounded retry cache for lifecycle results. The result/failure owner is
  the agent's bounded retained store, and the cost lifetime of the actual send and receipt is preserved.

## 6. Local state transitions and ownership

### LOAD

1. Check the envelope, shared metadata, supported kind, settings and result storage space.
2. Check current ID occupancy and register `loading` as one atomic step. For failures before native work, such as adapter construction failure,
   reclaim only the mailbox/route/reservation that was created. An existing node with the same ID is not changed at all.
3. Hand the load to an individual worker. The agent control loop does not wait on a long native LOAD and so does not block
   INSPECT/UNLOAD/reconcile for other nodes. Model-specific payload validation and the actual load are owned by the adapter.
4. On success through typed completion, make the node runnable and return the exact result. Ordinary requests during loading are
   rejected under the existing not-ready contract and do not start native inference.
5. After a load failure, if reclaim of its own resources is confirmed, remove the node; the agent owns the failure result.
   If cleanup is unknown or resources remain, keep the failed instance for that ID isolated and observable.
   Other nodes are not changed. This is not a reusable empty node.

### UNLOAD

1. Check ID/generation and adapter identity. Put up a shutdown barrier so it does not race with already accepted input.
   Acceptance of new work and confirmation of shutdown must not cross. Input from before the barrier is not dropped.
2. Keep the existing busy/settlement/KV/effect/output ownership checks. On a busy rejection, lift the temporary fence so the same node can
   finish its existing requests. Do not create a deadlock that blocks settlement messages while waiting for busy to clear.
3. Once shutdown is possible, the adapter releases its own native/child resources. Failure or unknown status is not turned into success;
   it is isolated and the first error/cleanup error is kept. State the reclaim path for failed nodes too, and do not disguise it as a normal UNLOAD.
4. The adapter returns a typed lifecycle result. Do not delete by watching for `snapshot() == "unloaded"`.
   In particular, llama.cpp currently has a deletion race between `set_snapshot("unloaded") → emit_json`.
5. Hand the original UNLOAD input and the terminal result to agent ownership, and confirm through authoritative retained observation that
   no other queued/held input/output remains. Do not create a **self-wait that keeps UNLOAD's own input/result on the node
   while waiting for count=0**. Do not get past it by clearing or dropping other Events.
6. Remove the route and NodeOwner, and confirm worker shutdown and release of owned resources. Only then does the agent send the
   UNLOAD success result it holds to OUTER. After a normal response, the ID must not appear in INSPECT `nodes`.
7. Output/receipts whose ownership has already passed to the transport remain the responsibility of that owner. Do not use node removal
   as evidence of remote acceptance or KV settlement, and do not wipe the transport records along with it.

Typed lifecycle notification sits on the neutral `RetainedNodeAdapter`/event-node boundary and binds request identity, result and resource state.
The agent does not interpret model-specific JSON. The new notification path also consumes the existing count/byte reservation,
and does not bypass completion retention through a separate unbounded channel or copies of the original Event.
HF's existing explicit abort keeps its failure-cleanup meaning. An abort result alone does not produce a normal UNLOAD success.

## 7. Implementation order and phase deliverables

**2026-09-16 M0 done:** The [call-path and ownership audit](../tests/reports/node-load-lifecycle/20260916_014500.md)
mapped the current control/broker/retained adapter/OUTER callers to NL01–NL14. The first M1 implementation sets up, together,
a per-node supervisor that does not block agent control, an agent-owned terminal result, typed lifecycle completion separate from the snapshot,
and bounded return cost. The stdin join after a bind failure and the failed-LOAD reclaim found in B5 are
reused as NL05/NL08 fixtures.

**2026-09-16 M1 done:** Building on the shared protocol codec and typed adapter completion, implemented the asynchronous agent supervisor,
route registration that starts paused, LOAD construction and UNLOAD removal, the agent-owned terminal result and lifecycle-state INSPECT.
On neutral real TCP, verified malformed and duplicate LOAD, INSPECT during a slow LOAD, normal LOAD/UNLOAD, and the
three retained owned items after an OUTER disconnect. Workspace feature off/on each gave 1,514 passed, 0 failed, 7 real-model tests
ignored, and the owner-removal mutation was detected by the real TCP test.
See the [M1 verification report](../tests/reports/node-load-lifecycle/20260916_023059.md). Making the real llama.cpp/HF workers
emit typed completion and use the new node lifecycle path is M2.

**2026-09-16 M2 done:** The real llama.cpp/HF retained workers return the supervisor Agent's LOAD/UNLOAD
as typed terminals. Native/child cleanup state, busy rejection, input/native response retention,
completion saturation and wrapper headroom beyond the HF Python frame were verified with real worker fixtures. Workspace
feature off/on each gave 1,523 passed, 0 failed, 7 real-model tests ignored, and 3 independent mutations of the final source
were detected. See the [M2 verification report](../tests/reports/node-load-lifecycle/20260916_032621.md).
The next phase is M3: migrating the Rust/HF OUTER and the real-hardware callers to the new lifecycle commands.

**2026-09-16 M3 done:** Migrated Rust event-drive and the HF Qwen controller/real-hardware fixtures to Agent-target
NODE_LOAD/NODE_UNLOAD, and removed the legacy CREATE/DELETE acceptance branch from the event runtime.
Confirmed OUTER per-node reclaim after a partial LOAD rejection, causation-based result binding, no effect from direct node bypass, 12 failure/recovery cases
on a real Agent and HF child, and 1,525 passed for each of workspace feature off/on. Two mutations removing the
Agent target in the final source were also detected. See the [M3 verification report](../tests/reports/node-load-lifecycle/20260916_041848.md).
The next phase, M4, is generation, cancellation, release and reload on small real llama.cpp/HF models, plus the final migration of the owning documents.

**2026-09-16 M4 done:** On real Qwen3.5-0.8B llama.cpp 2-stage and HF single GPU, confirmed, without CREATE/DELETE,
generation, interleaved execution, cancellation, partial LOAD failure reclaim, rejection of the previous generation, reload into a new worker, and final node/child count 0.
Workspace feature off/on each gave 1,526 passed, 0 failed, 7 ignored, and 57 Python tests and 2 independent
recompiled mutations passed. See the [M4 acceptance report](../tests/reports/node-load-lifecycle/20260916_064306.md).
This plan is complete; next in order is Qwen122B H0–H7 on the main roadmap.

The M1 pre-run review and the inputs for at most 3 rounds are
sealed in the [M1 deterministic execution plan](../tests/plans/node-load-lifecycle-m1-20260916.md). This phase
reuses `L001`~`L013` from the [deterministic execution register](deterministic-execution-register.md). If a new failure
appears, add a new lesson ID and an automatic blocking mechanism before re-running the same command.

This order is the work order inside this change. Do not arbitrarily reorder other work on the overall roadmap.

| Phase | Work | Condition for the next phase |
| --- | --- | --- |
| M0 | **DONE** — latest HEAD/dirty audit, exhaustive search for real callers, §8 counterexamples and lifecycle ownership design | [M0 report](../tests/reports/node-load-lifecycle/20260916_014500.md) maps duplicates, failures, completion responses, barrier and byte ownership |
| M1 | **DONE** — shared codec, typed adapter completion, asynchronous agent supervisor, neutral real TCP | [M1 report](../tests/reports/node-load-lifecycle/20260916_023059.md) records NL01·NL02·NL04·NL07·NL10 and the removal mutation |
| M2 | **DONE** — real llama.cpp/HF worker hookup, profile/retention integration, removal on completion/failure | [M2 report](../tests/reports/node-load-lifecycle/20260916_032621.md) records busy, cleanup failure, response/frame saturation, normal removal and 3 mutation kinds |
| M3 | **DONE** — migrate Rust/HF OUTER and real-hardware scripts, remove CREATE/DELETE and direct bypass | [M3 report](../tests/reports/node-load-lifecycle/20260916_041848.md) records partial failure reclaim, bypass rejection, real HF child, workspace and mutations |
| M4 | **DONE** — generation, cancellation, release and reload on real small llama.cpp/HF models; update owning documents | [M4 report](../tests/reports/node-load-lifecycle/20260916_064306.md) records final source, tests, reclaim and the Qwen122B next phase |

At each restorable point in each phase, follow the repository commit rules. Pause other authors, audit all non-ignored changes,
then commit a consistent checkout that you own. Do not arbitrarily include unrelated parallel changes.
The final result records the commit, exact commands, test IDs, exit codes, first failure, remaining work and first next action.

## 8. Required counterexamples and acceptance tests — all planned

Make added tests searchable under the name `node_load_lifecycle`. Pure-function tests alone do not replace the real paths below.
Do not hide the basic required tests behind a new feature.

| ID | Input / real path | Required verdict |
| --- | --- | --- |
| NL01 | Real agent TCP LOAD, with the neutral adapter and each real adapter | loading registration, load completion and INSPECT without CREATE; exactly 1 native load |
| NL02 | Concurrent LOAD of the same ID, including different generation/plan, in each of loading/loaded/unloading | at most 1 accepted; on the rejected side, 0 change to spawn/native/route/existing ledger/claim/output |
| NL03 | Two LOADs with different IDs, same device and same model | two nodes allowed in a declared configuration with enough resources; one UNLOAD has 0 effect on the other node |
| NL04 | Malformed metadata/kind/disabled HF/length/capacity/profile, exact and ±1 | rejected before native work; 0 new empty nodes or reservation leaks; 0 automatic creation from unknown ordinary Events |
| NL05 | Real LOAD initialization failure, partial child start, cleanup failure | removed from nodes if reclaim is confirmed; if unknown, isolated with first/cleanup errors kept; other nodes unchanged |
| NL06 | UNLOAD during mid-stage KV with no request, pending settlement or held output in the real run-loop | busy rejection; 0 native release; existing work/settlement resumes and the same request completes normally |
| NL07 | Stop right after the unloaded state is recorded, completion cap1, held terminal, OUTER Full/disconnect | no deletion race or self-wait before the completion response; owner/original/byte claim kept; exactly 1 success after resume |
| NL08 | Native cleanup failure, unknown result or worker exit on an idle UNLOAD | 0 success responses; later ordinary execution blocked; resource state and failure result observable |
| NL09 | INSPECT and ID reuse after a successful UNLOAD; delayed LOAD/UNLOAD/inference from the old generation | node/worker/owned listener removed; only the new generation accepted; 0 past effects on the new instance |
| NL10 | Slow LOAD on one agent alongside INSPECT/UNLOAD/reconcile of another node | a long model load does not block the shared control loop |
| NL11 | Response via real OUTER; corrupted source/causation/return route, partial frame, lost receipt | only the exact per-request result accepted; uncertain kept; transport ACK not mistaken for lifecycle success |
| NL12 | One of two or more individual LOADs rejected; the OUTER fixture UNLOADs the successful nodes | 0 automatic reclaim of other nodes by P4; full reclaim only through OUTER commands; failure and reclaim results distinguished |
| NL13 | Old CREATE/DELETE/direct node LOAD fed to the current Rust/HF callers and the new protocol | 0 CREATE/DELETE on the normal path; bypass commands rejected with no effect; normal inference/release on both adapters |
| NL14 | Repeated LOAD/UNLOAD and saturation with failure responses | 0 remaining active node/worker/claim; new lifecycle store stays within count/byte limits; existing generation history counted separately |

Do not hide in NL14 the fact that the existing `node_generations` keeps past IDs. This change does not
add an unbounded lifecycle result store. Do not shrink the history by removing the existing generation check.

Minimum independent mutations: remove the duplicate check / check after registration (NL02), early deletion on snapshot alone (NL07),
remove the busy protection (NL06), treat a cleanup error as success (NL08), drop the held result (NL07),
remove generation/response identity checks (NL09/NL11). Each mutation is actually recompiled in an independent copy and a separate target,
with the baseline source/binary/hash recorded. Mutating the user's checkout and restoring it afterwards is forbidden.

## 9. Verification runs and reporting

These are future implementation verification commands. They are not results run when this plan was written. First confirm the actual tool paths and
the HF fixture interpreter. Local build constraints and use of remote real-hardware resources follow the current roadmap.
Do not run the Rust commands below concurrently.

```powershell
cargo test --locked -p p4-agent -p p4-agent-core -p p4-adapter -p p4-protocol node_load_lifecycle
cargo test --locked -p p4-llamacpp-staged-adapter -p p4-hf-adapter -p p4-event-drive
cargo test --locked --workspace --no-fail-fast
cargo test --locked --workspace --no-fail-fast --features hf-transformers
python layers/adapters/hf/scripts/testing/run.py
node tools/scripts/docs-lint.mjs --all
git diff --check
```

- If a targeted filter runs 0 tests, do not treat it as PASS. Also check NL01/NL13 on the real entrypoint with the feature on.
- Use the new lifecycle API to verify normal responses, cancellation/release and reload on small llama.cpp/HF models. Include regressions for HF's existing source/return-route,
  topology/handshake, and native load identity and resource profile.
- If the environment allows two small loads on the same permitted GPU, check NL03 with a real model. If the required environment is not available,
  distinguish neutral/local results from real-model BLOCKED. Do not change the test into a one-node-per-GPU limit.
- In an environment where remote runs are approved, run LOAD/UNLOAD through 2 physical agents, and NL12. Use only
  the resources/namespaces the user allowed. Long-context/8-wave/H0–H7 performance acceptance is a separate gate on the current roadmap.
- Record in `tests/plans/node-load-lifecycle-<date>.md` and `tests/reports/node-load-lifecycle/<timestamp>.md`
  the actual environment, source/binary/model, test IDs/commands/exit, passed/failed/ignored/not run, and raw evidence paths.
  Register new files in the README and the document map. After a failure, do not relax expected values, limits or inputs.

## 10. Document migration and final checklist

- [Event contract](event-protocol-v2.md): replace external CREATE/DELETE with the new LOAD/UNLOAD and result specification.
- [Isolation contract](layer-isolation-contract.md): state the neutral lifecycle notification and OUTER's responsibility for the whole-model load.
- [Verification convention](distributed-batching-verification.md): update the command flow of HF-REGISTER/HF-LIFE while
  keeping the existing no-effect-on-rejection, retention, child reclaim and generation check conditions.
- [Batching contract](adapter-batching-layers.md): connect UNLOAD's local stop point to the final node removal step.
- Update the run READMEs, tool docs and current HF docs; keep the CREATE/DELETE records in historical reports as they are.
- Register in the README and document map, and update roadmap progress. Do not mark phases of this plan complete without actual results.
- Confirm on the real path that the final implementation needs no external CREATE/DELETE, starts from LOAD with no node present, removes the
  node and native resources after UNLOAD, and that OUTER receives the exact final result.
- If automatic whole-model failure reclaim in OUTER remains separately unimplemented, say so. Do not quietly include it in P4's completion criteria or
  describe it as something P4 implemented on OUTER's behalf.

## Execution request to hand to a new session

> In `F:\dev\p4`, read `docs/node-load-lifecycle-plan.md` to the end and implement it.
> A node is a model load instance and is created and removed only by external LOAD/UNLOAD. Reject IDs that are in use.
> Judging overall multi-node success and reclaiming on failure are OUTER's responsibility. First audit the current HEAD and parallel changes,
> keep to the roadmap, verification convention and isolation contract, and verify with the plan's real-consumption counterexamples and independent mutations.
> If the baseline code in this document has changed, keep the latest retention/cost contracts and update the change paths.

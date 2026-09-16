# P4 · adapter · llama.cpp layer isolation contract

Updated 2026-09-07. Initial code observations are based on `a9e1967fc`; each later observation is limited to the source at that time.
Current checkpoint, verification status, unverified candidates and stop conditions follow [roadmap §0](distributed-batching-roadmap.md#current-status).
This document is the **owner of per-layer responsibilities, allowed dependencies and update-impact isolation**.
It does not claim that every target boundary below is implemented. For current status and execution order see the
[roadmap](distributed-batching-roadmap.md); for isolation tests I00~I09 and real-hardware verdicts see the
[verification protocol](distributed-batching-verification.md); for the role of each document see the [document map](document-map.md).

## 1. Design goal and what completion means

Keep multi-computer batched execution of very large models while stopping frequent llama.cpp changes from spreading into the P4 delivery core, the flight ledger
and the batching policy. llama.cpp is not a simple device wrapper. It is an abstraction layer that owns model/graph/memory execution,
and below it the CUDA, CPU, Metal, HIP, Vulkan and other ggml/backend implementations change on their own.

“0 header includes”, “it compiles” or “this pin applied cleanly” alone do not mean isolation is complete.
The build must enforce dependence on allowed surfaces only, conformance must catch semantic changes, and product LOAD must
enforce those verification results and the execution identity. The final performance proof is still the real-hardware wave gate.

## 2. Overall responsibilities and dependency direction

```text
OUTER: product goals, resources/topology, requests/SLO, snapshot triggers
  │  P4-owned envelope + adapter-owned content-type
P4 event transport / broker / node lifecycle
  │  backend-neutral NodeAdapter, preserved backpressure
llamacpp staged adapter: L0 queue → L1 ledger ↔ L2 acceptance → L3 policy → L4 proof → L5 issue
  │  versioned stage commands, capability, identity, results; no pointers
native stage shell / engine facade
  │  P4-owned semantic operations and opaque handles
llama engine bridge / private-common compat
  │  llama public API + isolated private stage hooks
llama.cpp model, graph, memory and backend-scheduler abstraction layer
  │  ggml/backend contract
CUDA / CPU / Metal / ... concrete backends, plugins, devices, buffers
```

Arrows show the direction of semantic dependency. Upward results/telemetry pass through explicit contracts and create no reverse ownership.
Matching the real number of directories or CMake targets to the diagram is not a goal.

**Isolation has three distinct pass conditions.** Dependency isolation keeps upper code from knowing lower implementations
by name; permission isolation keeps transport and policy from finalizing the execution ledger or KV on their own;
semantic isolation keeps an upstream change that still compiles from being promoted if it breaks an existing contract.
A refactor that passes only one of the three is not reported as complete layer isolation.

### Per-layer roles in the P4 repository

| Layer / current path | Owns | Does not own |
| --- | --- | --- |
| OUTER: `tools/event-drive/`, product callers, `test/benchmarks/` | model/topology, SLO, waves, deployment resources, snapshot command triggers, acceptance verdicts | changing engine KV directly, guessing stage completion, treating a wire ACK as generation completion |
| `layers/protocol/` | event envelope, addressing, identity, routing, type boundaries | llama seq / ggml type / CUDA device, per-model batch grammar and memory calculation |
| `layers/agent/` event path | transport/broker/mailbox, ordering, delivery, node lifecycle, generic backpressure, shutdown | prefill/decode selection, KV cell cost, llama context waiting/sampling |
| `layers/adapters/adapter/` | backend-neutral NodeAdapter, offer/take, completion contract, event ownership, Full/Closed distinction | private types, options or bypass casts of llama or any specific engine |
| `layers/service/` | neutral business vocabulary/coordination for the service path and the existing KV coordinator | a hidden scheduler that replaces the current event worker. Tests in this layer do not count as proof of the event path |
| `entrypoints/agent/` | composition root: selected runtime, adapter registration, configuration, process resources | registration shortcuts that make generic libraries reference concrete adapters in reverse |
| concrete adapter | interpreting its own content-type, fulfilling the engine contract, batching/ledger, capability | extending the shared P4 event interpreter per model, or handling another backend's state directly |

The current event boundaries are `layers/adapters/adapter/src/node_adapter/mod.rs::NodeAdapter` and
`layers/agent/src/event_node/mod.rs::EventNode`. Do not mix them with the older service boundary of `Adapter::start(Work)`.
Fixes to 2PC/delivery correctness in the P4 core are allowed, but they must come with neutral-contract tests for the mock and other adapters.

**Naming boundary:** “P4-owned type” in this document means owned by this repository rather than upstream;
it does not mean everything goes into `layers/protocol/`. Semantic types for batch/fragment/KV/shape/capsule and
the stage wire are **owned by the concrete adapter**. The common core carries those payloads opaquely.
Before promoting anything to common, first prove its independent meaning with a second implementation or mock that does not know llama.

### Semantic roles are separate from authentication and control authority

The later SESSION authority check compares the stage role declared by the adapter against the envelope endpoint.
`entrypoints/agent/src/event_runtime/transport.rs::serve` dispatches the source it reads without binding it to an authenticated peer
identity, and `layers/agent/src/event_broker/mod.rs::EventBroker::dispatch` owns target routing and the
delivery ledger. We therefore do not claim that this adapter check can tell apart a sender that spoofs a correct
terminal endpoint field itself.

Real peer authentication and connection policy belong to the neutral transport/composition; who may build, load and release
a pipeline is a product control-authority contract. SESSION being immutable does not prove the authority of whoever set it first.
This is an unimplemented, open trust boundary; even on a private or trusted network the real-hardware spec must state this limitation.
Keep control authority distinct from the legitimate feature of several OUTERs submitting inference to the same pipeline.
Ad hoc fixes that allow only one OUTER, or that put llama sequence rules into the P4 broker, are forbidden.
When adding authentication, verify peer/source spoofing and generation reuse together with neutral mock and other-adapter regressions.

<a id="external-hf-boundary"></a>

### 2.1 Applying the HF concrete adapter (migrated 2026-09-14)

`layers/adapters/hf/` is a P4-owned concrete adapter, not a separate workspace.
The entrypoint registers only `hf-transformers`. Per-model semantics and the Python worker stay inside HF.

- The `adapter/` Rust bridge handles retained/IPC, identity, capacity, output attribution and child lifecycle.
- Per-model Python owns partial load/forward, batch selection, KV/recurrent state, quantization and the tensor codec.
- External control uses only Agent-target `NODE_LOAD` and `NODE_UNLOAD`. LOAD builds the bounded route and
  bridge and completes worker readiness; UNLOAD removes the child, route and owner before it responds.
- Payloads between nodes are opaque to P4, and Python does not bypass retained backpressure through direct communication.
- This source migration does not change the common core, the neutral traits or the existing llama execution boundary.
- Build with the root Cargo.lock and a single P4 commit. Environments and weights are prepared separately through the adapter manifest/lock.
- Keep the isolation, ownership and semantic-compatibility tests in [HF verification](distributed-batching-verification.md#hf-integration-contract).

## 3. Permissions of adapter layers L0~L5

Detailed batching invariants are owned by the [batching contract](adapter-batching-layers.md). This table fixes **who may read and write what**.

| Layer | Input / result | Write permission and prohibitions |
| --- | --- | --- |
| L0 queue | requests, cancellations, cache commands → bounded pending | manages only event ownership/order and the wait budget. Does not turn a drop into success or record KV/flight completion directly |
| L1 ledger | actual accepted issues, stage results, reservation evidence → consistent state | sole writer of resident/flight/credit ticket/generation. Full-return validation and atomic commit; other layers must not update counters independently |
| L2 acceptance/occupancy | limits/commands set by OUTER + actual stage memory → reserve/wait/explicit reject | accounts through prepared and fulfils leases/reservations and quotas. Does not invent automatic TTL/victim/snapshot triggers on its own |
| L3 composition policy | immutable eligible snapshot, budget, age, verified cost profile → candidate allocation | pure, deterministic selection. No I/O, native calls, state mutation or assumed transport success |
| L4 proof / issue preparation | candidate + capabilities/reservations of all stages → validated issue | checks shape, membership, position/phase and byte budget. Forbids batches that are legal only at the head. Does not count a plan as executed work before the issue is confirmed |
| L5 issue/transport | validated issue → stage calls / capsule delivery / normalized results | fulfils binary format, transport and ordering. Does not confuse ACK/terminal/stop point, and does not consume fragments without L1 |

L3 is not “any generic queue unrelated to llama.cpp”. It takes the **normalized semantic constraints** of llama memory/shape as input,
but it does not look at private enums/structs or CUDA APIs directly. Generating capabilities is native compat's responsibility;
if it advertises a capability it cannot fulfil, LOAD/conformance fails.

Several stages on one GPU, several GPUs for one stage, CPU fallback and multiple hosts must all be expressible as input.
A cost profile binds to the build/layout/model/workload family. Do not use GPU number strings or layer counts as a proxy for execution cost.

### Single authority for state changes and external effects

Do not split layers by file name alone. The isolation criterion is **who finalizes state and who only executes effects**.

- L3's result is a **candidate** for allocation and for fairness/resume changes. When it only produces a candidate, or an issue is rejected,
  it does not silently consume executed work, credit or per-request service turns. The commit rule binds to L1's accepted issue.
- L4 produces the verification results that correspond to `ValidatedIssue`/`ValidatedSettlement`. Type names are chosen at
  implementation time, but a bypass entry point through which L5 receives raw candidates, and raw counter writes from other layers, are not allowed.
- L1 finalizes the request delta, flight ledger, settlement receipt and output/settle/release **intents** in one commit.
  It does not run native calls or network sends inside the pure transaction.
- L5 executes the committed effects and returns the results to L1. On Full it keeps the intent; when it is unclear whether execution
  happened, it leaves an `Uncertain`/fenced state. It does not treat a timeout as not executed and run again.
- The engine owns the operation that produces the actual token. L1 owns the semantics of approving token/position/stop as a request result and
  emitting it exactly once. Success in the tail engine is not an output right that bypasses ledger approval.
- Transport redelivery prevention, completion idempotence within a live process, and durable exactly-once after a crash are
  different guarantees. The last one is not claimed without a durable receipt/outbox and a receiver dedup contract.

At the base commit, `worker/drive.rs::Worker::emit_tail_results` @ a9e1967fc published the OUTER output first and then
sent TAIL_BATCH to the head. In a later working tree on the same day, the tail forwards only TAIL_BATCH,
and the output intent is published after head validation/commit. The actual tests and source scope follow the later record in the
[settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).
Any alternative that reintroduces direct tail output must first separately verify a head approval receipt and a duplicate/restart contract.
This is a change of result ownership inside the adapter, not a reason to add llama token interpretation to the P4 broker.

Later per-request release proofs follow the same boundary. The adapter binds the original submission to the native control settlement,
and OUTER consumes its own send record, terminal approval and explicit release set. The owner of the new completion wire is
the [release section of the batching contract](adapter-batching-layers.md). No model-specific fields were added to the shared P4 envelope, and
no llama/backend types were raised into the pure completion ledger. Receipt routing and member validation are not evidence of sender authentication,
state serialization compatibility or actual execution placement.

A return receipt alone does not seal execution authority for every stage. The request incarnation and operation ID must be bound from issue
through each stage's receive/execute/settle/release/ack. When the same load/session/key/slot is reused, a path in which
an earlier RELEASE erases new KV cannot be blocked by the head's ack check alone.
These semantics and the wire version are owned by the concrete adapter. Do not replace them by putting llama sequence rules into the P4 common core
or by arbitrarily forbidding request ID reuse.

## 4. Native isolation layer — where updates are absorbed

| Boundary | Allowed surface | What must be isolated |
| --- | --- | --- |
| Rust adapter ↔ native stage protocol | P4-owned versioned command/result/capability with explicit size and lifetime | leaks of pointers, C++ object layout, serialized common_params, upstream enum ordinals |
| stage shell ↔ engine facade | semantic operations for load/inspect/execute/settle/state/quiesce, opaque owned handles | llama_context internals, ggml buffer internals, sampler ownership details, CLI structs |
| llama engine bridge | the current pin's public `llama.h`/public ggml API, explicit allowlist | do not assume even the public API is a stable ABI; translate changes inside this module |
| private/common compat | stage hooks, internal graph/memory, upstream parser/sampler/speculative/checkpoint calls | do not expose private headers to other native runtime/server/shared headers |
| upstream ggml/backend | device/plugin allocation, kernels, streams, buffer implementation | calling device APIs directly from the P4 ledger/policy, adding CUDA assumptions to the core |

This is a **bounded module boundary**. It does not mean putting all C++ into one giant file.
If needed, split it into private implementation files per engine/common/stage-hook, but keep the list of allowed modules, symbols and build targets
reviewable. Do not report “0 violations” after widening the allowlist.

### Public types and lifetime contracts

- DTOs exposed to the adapter/upper shell carry only the semantics P4 uses. For each function, state ownership/borrowing, thread affinity,
  buffer validity period, error/cancellation, sync guarantees and callback reentrancy.
- synchronize/logits reorder and KV changes on a `llama_context` are serialized by a single owner.
  Several samplers calling the same context do not become safe just because the handle is opaque.
- llama.cpp public types may be used **inside the native bridge**. Some native headers expose them today; that is
  an existing intermediate boundary. They do not rise into the pure policy/ledger or the P4 core/wire.
- Define the criteria for bumping ABI/version, how unknown optional capabilities are handled, and how mixed-version fleets are rejected.
  Unknown values are never interpreted automatically as “default CUDA” or “supported”.
- upstream owns the common plan/options end to end. Instead of copying the few fields read directly, or building 27 getters that
  mirror the struct, isolate **the call sites of the parse/convert operations**. Owning the JSON grammar and owning the call site are separate matters.

### Interface register that must be made concrete — target deliverable

The names below do not claim that these APIs exist today; they are a **per-boundary contract classification** to implement.
Each implementation slice binds the actual files, symbols, inputs/outputs, ownership, errors, versions and consuming test IDs.

| Boundary | What the contract must contain | Where upstream changes are translated |
| --- | --- | --- |
| Policy input / candidate | immutable eligible snapshot, separate row/byte/KV budgets, stable tie-break, distinction between candidate and accepted fairness delta | native generates capabilities; L3 reads only normalized shape constraints |
| Issue / settlement | logical issue ID, physical membership, expected range, accepted/uncertain, atomic delta and effect receipt | engine-agnostic settlement semantics in L1/L4; native IDs and results are mapped in L5 |
| Engine plan / options | opaque full plan, parse/clone/apply/inspect operations, preservation of the original grammar, defaults and indirect options | common/parser compat module; `Impl` and internal accessors stay hidden from consumers |
| Engine execution | owned context, logical→physical split, execution approval point, buffer lifetime, thread affinity, quiesce/settle/release results | llama engine bridge and private stage-hook module |
| Capability / placement | per-feature support, version, limits and evidence; configured request values kept distinct from measured values; actual device/buffer placement | collected from the engine/ggml public abstract APIs; vendor-specific values isolated as optional telemetry |
| State codec | state ABI, layout, identity, position, import/export/trim semantics, support matrix and rejection reasons | state compat module; L3 does not interpret serialized bytes |
| Tensor transport codec | wire dtype/flags, shape/stride/alias/view, endianness, ranges and version, rejection of unknowns | the native codec converts between the ggml representation and the adapter wire representation |

If an opaque class exposes `impl()` publicly and consumers can include the internal header, the boundary is not complete.
If friend/internal accessors are needed, grant access only to the compat private implementation and white-box test targets.
Confirm with a real consumer compile that the internal header is not reopened through the shared include root.

Do not confuse the two kinds of wire. Keep the **normalized semantic DTOs** that policy/ledger read separate from the
**engine-specific tensor payloads** carried between natives. The latter may be an opaque representation bound to a codec identity, but
raw ggml ordinals cannot be read as engine-neutral semantics or assumed to be automatically compatible across pins.
Existing raw representations are either sealed with a versioned codec plus negotiation, or converted to adapter-owned stable dtype/flags.
This choice and the migration are settled in B5. Wrapping them in a plain integer type does not by itself complete isolation.

### Enforced boundaries in CMake and Rust

Native compat's `src/compat/model_support.cpp` is a dedicated module that rejects unsupported artifacts/options using public GGUF metadata.
Only the string values that preserve the existing model rejection semantics are isolated in this file;
it grants no right to use model names as branches in graph, KV or batching algorithms.
Even in this module, architecture enums, casts to model implementations, private headers and any llama/ggml execution calls other than metadata lookup
are rejected. The ban on model names in other runtime files and the limit on modifying upstream model implementations still apply.

- protocol/agent/adapter-contract crates have no normal dependency on concrete backends.
  Registration happens in the entrypoint, and pure scheduler/ledger tests build without a llama checkout, GPU or network.
- The neutral completion store may provide the actual retention cost of opaque Events, move-only space ownership and notification.
  The adapter decides the count/limit of required results that native work produces, and the flight/KV authority. Do not use a store
  reservation as source authentication, a distributed wire grant or KV completion. Detailed ownership transitions follow the local store section of the batching contract.
- An integration-test-only target of a concrete adapter may use, as a dev-dependency, the public API of the neutral EventBroker/EventNode
  and tokio runtime, synchronization and time primitives. This is test assembly of the real delivery boundary; it is
  not a production dependency inversion or permission to reach agent private APIs. Keep it distinct from normal/build dependencies, and
  keep the pure ledger/policy tests independent of llama, GPU and network.
- Do not expose upstream `src/`, `common/` or `ggml/src` to native consumer targets through direct or transitive includes.
  Removing them from a `target_include_directories` statement is incomplete if the INTERFACE of a linked target spreads them again.
- Check **both** the full source build and the imported-library/relink path. Being PRIVATE on one path does not approve the other.
- Also check production sources outside the facade for direct common/private calls, forward declarations and function signature leaks.
  Do not declare header type isolation from include-string checks alone.
- The executable needing the final `llama-common` DLL/library is different from exposing its include/API to consumers.
  A private link dependency from compat is allowed. Also account for link-only propagation of STATIC; direct consumer calls are forbidden.
- White-box tests such as the 27-field comparison of internal options may stay in a compat-only test target.
  Do not delete those tests or bloat the public facade with test getters. Keep runtime consumption tests separately.
- Assertions must actually run in Release too. Confirm that an intentionally failing fixture makes CTest fail.

### Minimum records when implementing the dependency manifest

The manifest is a **B5 implementation deliverable** that does not exist yet. Deliver the following fields together with a working interpreter.
A list that only classifies file names without checking the dependencies the compiler actually used is not accepted.

| Field | What must be stated |
| --- | --- |
| owner / consumers | which responsibility it belongs to — P4 common, adapter L0~L5, stage shell, engine bridge, common/private compat or backend — and the list of allowed consumers |
| exports | public headers, functions, types, wire codecs and versions; ownership/borrowing, post-move state, thread affinity, buffer lifetime, failure semantics |
| imports | allowlist that separates normal/build/dev dependencies and direct/transitive includes, compile definitions and link-only dependencies |
| configurations | full source / imported relink / no-llama, Debug/Release, OS/toolchain and declared backend combinations |
| exceptions | actual path/symbol of each current leak, reason, removal condition, owning phase. An exception is never turned into completion or a permanent allowance |
| checks / provenance | I/T IDs run, normal consumer and forbidden-violation fixtures, source/target graph and result digest |

For Rust, check the actual dependency graph for each workspace feature combination; for C++, check the computed usage requirements of CMake targets and
the actual compile/link commands. Fixing a single `PUBLIC` statement, or only shrinking the scanner allowlist, is not completion.
Keep the link requirement of upstream libraries, the permission to call private APIs and the deployment requirement of DLLs separate from one another.

So that adding layers does not by itself raise hot-path cost, pass large immutable prompts/tensors by reference or move with clear ownership, lifetime and
byte budget, and pass small candidate deltas between L3/L4/L1. There is no need to create new threads, RPC or serialization
queues for each layer inside the adapter. Use transport only where a physical process or host boundary requires it.
Instead of gaining speed by removing global safety checks, keep independent full-comparison tests and use a touched-member index.

## 5. Boundaries and gaps observed in the current implementation

This table records code observations. It is not a completion table for new enforcement gates.

| Observation | Current basis / verdict |
| --- | --- |
| Rust staged adapter | Cargo dependencies are the p4-adapter/p4-protocol/serde family. A base exists in which the policy has no llama native link |
| Later B1 Rust boundary | In the working tree, `node/flight.rs::FlightLedger`, the per-request issue/settle transitions and `worker/outcome.rs::apply_fragment` validate adapter-owned rows, ownership and results. `worker/effects.rs` executes committed intents. No llama types were added to the common protocol/agent |
| Fake stage connection | Drives the real worker methods through the Box-passing implementation of the existing `process/core.rs::ServerControl`. It is a test double at the existing stage command boundary, not a separate scheduler imitation. It does not replace real LOAD negotiation, the full worker loop or native conformance |
| Later execution identity | `node/ownership.rs::StageOwners` and native `runtime/physical_authority.hpp::PhysicalAuthority` check incarnations and control-operation receipts. The semantics of the new wire/BindLoad are owned by the [batching contract](adapter-batching-layers.md) and use no llama/ggml types. PHYSICAL re-execution and crash recovery are not solved |
| Remaining gaps after the later implementation | Redelivery outside the receive-receipt retention window, phase/range ordering for fresh IDs, restart freshness, processing opportunities under a sustained queue and the copy cost of long candidates are incomplete. PHYSICAL replay within retention follows the later receive ledger in the [batching contract](adapter-batching-layers.md). Does not claim full completion of B1/B2 or of the I gates |
| L3 candidate purity | In the later working tree, `Scheduler::prepare_plan_with_physical_capacity`/`validate_prepared`/`commit_plan` separate selection from fairness approval. The real drive and Simulation commit at approval time, and a reject→replan consumption test exists. Unit tests of the older plan convenience API itself do not count as proof of product calls |
| private llama src access | CMake grants llama `src/` directly as PRIVATE only to `p4_llama_compat`. A private include canary also exists |
| common dependency | Later gate run: 78 files, header include debt 0 / source 5. Note that the gate checks specific include patterns only |
| Type leak | `runtime/request_options_grammar.hpp::parse_grammar_triggers` forward-declares `common_grammar_trigger` and uses it in a vector signature. include 0 ≠ common type 0 |
| Transitive build exposure | The CMake runtime links `llama-common` PUBLIC. The imported relink `_p4_inc` contains common/vendor/ggml-src and is propagated to the INTERFACE of several imported targets. Isolation is not complete overall |
| Imported identity verification | The source stamp in the CMake imported branch and file-existence checks in separate build/runtime directories are not evidence that the lib/DLL/plugin actually loaded was built from that source. The stale/shadow injection of I07/T52 in the verification protocol is required |
| opaque plan | `compat/p4_llama_compat.hpp::LlamaPlan` and an internal header exist, and the plan parser call has moved. Raw/internal access still remains |
| Loopholes in opaque | `compat/p4_llama_compat.hpp::LlamaPlan::impl` and `SamplingOptions::impl` are public, and the internal header is reachable through compat's PUBLIC `src` path in CMake. Target and access-permission isolation are still needed |
| Strengthened opaque lifetime test | In the later 2026-09-07 working tree, `runtime/request_options_test_plan.hpp::consume_request_options_plan` takes the sampling snapshot before ownership transfer. The real options E2E and the model-free `plan_lifetime_test` use this test helper. The old ordering and early-return mutations, and Release asserts firing via an intentional failure, were confirmed. The real-model options E2E is still not run, and I04/T03 is not fully complete |
| native public API | The compat header exposes `llama.h`/`ggml-backend.h` and public handles. The proof scope differs between the pure Rust boundary and the native internal boundary |
| Wire semantic leak | Rust `v2/capsule.rs::TensorDescriptor` carries `tensor_type: i32`, and `Invocation` carries `flags: u32`. Native `llama_stage_runtime_physical.cpp::StageRuntime::capture_execution` copies hook flags. Codec semantics and version binding need a separate audit |
| identity | A base for passing the pin/patch/backend inventory exists. An execution-ownership bind for product LOAD was added, but enforcement of actual placement, stage/state ABI and the verification manifest is incomplete. Do not confuse the execution epoch with backend semantic compatibility |

The native path reference is `layers/adapters/llamacpp/staged/server/src/`; the build reference is the
`p4_staged_llama_runtime`, `p4_llama_compat` and `P4_STAGED_LLAMA_BUILD_DIR` branches in `layers/adapters/llamacpp/staged/server/CMakeLists.txt`.
Do not copy the source-debt numbers above into the prose of other documents.

Rust `StageOwners` and C++ `PhysicalAuthority` are different implementations of the same semantics. The two unit suites passing
separately does not guarantee agreement across the language boundary. They must pass the common-transcript and independent-oracle comparison in the verification protocol,
and that comparison does not replace real llama KV/backend conformance either.

Currently `last_load_generation` is initialized in `AdapterState::default`, and `Worker::new` creates a new state.
Therefore do not stretch the highwater/receipt protection within the current lifetime into **a freshness guarantee beyond the same Worker/load lifetime**.
Even while the agent process stays up, recreating the Worker alone can move the boundary. load/run freshness across a new Worker/agent and
reconnection is a separate open item of T18/T24; it does not mean a durable epoch authority already exists.

## 6. Handling very frequent upstream changes

Measure update frequency afresh for the current period. Do not use a past average of commits per day as a permanent constant.
Neither “a pull only changes values” nor “24 patches applied cleanly, so the semantics are identical” is acceptable.

1. **Separate discovery from adoption**: collect candidate commits/releases, but do not move the production pin before approval.
   Experiment with a new pin in a clean, separate worktree. Do not wipe a dirty upstream, and do not move to follow the remote production.
2. **Classify changes**: separate public API, common parser/sampler, private graph/memory, state codec, model features, ggml/backend and build.
   Review not only changed files/symbols but also semantic changes in KV/position/alias, views and stream synchronization.
3. **Replay patches**: record the purpose, dependency order and allowed files/symbols of each stage hook / upstream fix / model feature.
   Detect whether a pure upstream fix has been absorbed upstream, confirm with regression tests, then remove it. If model/hook patches have dependencies,
   state them in the manifest; do not assume every bundle applies independently in any order.
4. **Adapt inside the isolation**: modify the allowed compat modules and the patch queue. If upper-layer feature requirements are unchanged, the protocol/ledger/policy
   sources and deterministic traces must stay unchanged. If they change, review separately whether upstream dragged the change in or it is a new requirement.
5. **Judge identity**: separate build provenance, stage ABI, state ABI and actual layout. Do not break state identity on every build, and
   do not declare automatic compatibility when the upstream state writer has changed. Allow only what passes the matrix in the storage convention.
6. **Gates**: after pristine replay, classification/EOL, I/T/native and N-1 compatibility tests, verify on the declared production backends and fleet.
   CPU success does not stand in for CUDA/Metal success. If no GPU machine is available, leave that promotion BLOCKED.
7. **Waves and adoption**: with the fixed spec of the final model, confirm non-regression in correct responses, useful TPS/GPU, memory and drain.
   Preserve the previous pin/binary/model identity so a safe deployment rollback stays possible, but judge state compatibility separately.

This does not mean deploying every upstream commit immediately or always running full GPU experiments.
Automate candidate checks, and require every applicable gate for **production promotion of the selected pin**.

### Change budget and version boundaries for the adopted pin

- For an adaptation that keeps the same product semantics and the same normalized capabilities, source changes in P4 common and adapter L1/L3
  have **0 as the target contract**. Do not set a C++ compat file count of 1 as the target. Record the amount of change inside the allowed modules and
  the actual manual tracking cost. If an upper-layer change is needed, first judge whether it repairs a leak or is a new product contract.
- A comparison trace is a versioned semantic projection of logical input, selection, issued membership, settlement and rejection. Do not arbitrarily
  delete differences in build ID, wall-clock time or device name to make traces match. Seal the excluded fields and the reasons before comparing.
  If the normalized input itself changed, that is a new capability review, not a test that must be unconditionally identical to the previous policy trace.
- The P4 event version, adapter stage command version, tensor codec, engine execution ABI, state ABI and actual layout
  each have their own reasons to change. Do not invalidate every stored cache because of a parser rename, and do not treat a kernel swap
  as evidence of state compatibility. Concrete storage identity follows the storage convention, which is its sole contract.
- State the supported pin/backend combinations and the mixed-fleet allowance matrix explicitly. The principle is that only verified combinations may LOAD.
  When the backend changes, re-verify the cost profile, but that fact alone does not add CUDA/Metal branches to L3.
- Adopting a candidate pin is not hot-swapping a new library or handle into a running context. It requires
  drain or failure convergence of existing work and identity negotiation for the new load. Even if rolling back to the previous binary is possible, whether
  the previous reader can read the new state is a separate question. A string saying the ABI is the same does not by itself approve backward compatibility.

### Impact absorption and failure location by change type

| Change | Scope usually modified | What to detect/block without modifying upper layers |
| --- | --- | --- |
| common CLI/sampler/option structure and defaults | common compat implementation and its internal tests | lost options or changed default semantics fail I04. Do not add more copies of public getters |
| llama public API / private graph hook | engine bridge / stage-hook patches | I03 keeps upper-layer source and settlement traces unchanged for the same input. Changes in calls or buffer lifetime fail conformance |
| model memory / state writer / split semantics | capability, state codec, model patches | semantic mismatches detected by I05/T53/K03; refuse to advertise or restore unaudited capabilities |
| ggml dtype / buffer / backend interface | tensor codec, engine backend bridge | reject LOAD/execution for unknown codec, alias/view and layout combinations; ordinal reordering is also refuted |
| CUDA/CPU/Metal kernels / plugins / toolchain | upstream backend, build/packaging | check the actual operations, placement, library identity and numeric/wave regressions on that backend. Adding vendor branches to L1/L3 is forbidden |
| New product semantic feature | a separate adapter contract change | explicitly approve the DTO/version and policy changes the new requirement needs. Do not hide them in an upstream adaptation patch |

Reimplementing each backend's best kernels/streams in P4 is not the goal. Use the ggml/backend abstract API, and
raise per-backend lifetime, synchronization and buffer constraints as capabilities confirmed by conformance.
Even for the same model, re-check compatibility and the cost profile when the backend set changes. Runtime discovery and the
“list of available devices” are not evidence of actual tensor placement or of promotion for that combination.

## 7. Items to report for actual tracking cost

Record per pin: drift period and commit count, change classification, number of clean/fuzz/3-way/manual/conflict patches, modified modules/symbols,
number of and reasons for upper policy/ledger/protocol changes, build/test time, manual work time, causes of failures/regressions,
state ABI increases/decreases and remaining debt. A fixed patch file count alone does not prove that cost is absorbed.

Refutation is needed in both directions:

- Changes such as common/private API renames keep the same upper contract/trace with compat adaptation alone.
- Changes in return semantics, KV/alias or state codec fail conformance even when they compile.

Accumulate tracking records across actual consecutive pins. Do not generalize tolerance to frequent updates from one conflict-free sample on a single day.
Even when the isolation fixes are done, existing batch/worker bugs remain separate; completing one side does not close the other.

## 8. Delivery conditions for structural isolation

The roadmap owns execution order. The list below does not create a new order; it names **the deliverables B1~B5/B8 must produce**.

- Dependency manifest: for each actual crate/target/module, the owner, allowed direct/transitive dependencies, public headers/APIs,
  private include roots and the list of allowed upstream hooks. Exceptions carry a rationale, a removal condition and a check ID, and are not auto-approved.
- Contract register: the actual symbols and ownership/lifetime/cancellation/synchronization/version specs for each interface in §4. Do not copy the same text into several documents.
- Build evidence: in each of the llama-free pure build, the full build of the declared backends and the imported relink, allowed consumers
  succeed and forbidden include/type/link violations fail. Directory moves and string-search results are not substitute evidence.
- Change evidence: semantics-preserving API changes modify only compat and keep the upper source/trace; compilable semantic changes are
  rejected by conformance. A working mock and the real engine follow the same normalized result/failure contract.
- Operational evidence: product LOAD checks the required contracts, codec, layout and promotion manifest. Required unknowns are rejected, and
  missing optional telemetry is kept distinct from missing execution-safety capabilities. A pre-check in the drive alone does not close this.

In summary, **P4 owns delivery, the adapter owns execution semantics and batching, the compat layer owns upstream translation, llama.cpp owns model execution,
and ggml/backend owns actual device computation**. Isolation is complete when code, types, builds and failure tests
enforce these responsibilities and they hold across changes to the adopted pin. Only part of that goal is implemented today.

# P4 — the Kvasir distributed inference engine

P4 is the engine behind [Kvasir](https://kvasir-ai.net): the communication layer
that runs very large models across nodes on separate physical machines. Agents
carry work between machines; a concrete adapter (the llama.cpp staged server, or
the HF adapter) executes it. There is one process type — the agent — and no
central controller process.

This branch publishes the engine source from the team's development repository,
with its commit history.

## Status (2026-09-16)

| Area | State | Evidence |
| --- | --- | --- |
| Single-request baseline across **three physical hosts** (NVIDIA GB10 → Mac M4 Pro → Mac M4 Pro), Qwen3.5-122B-A10B Q5 split into 3 stages | **Passed** — short/medium/long requests completed with exact JSON, EOS, deadline and RELEASE; 3.53 GB moved between hosts; zero transport or cleanup errors | [report](tests/reports/release-a/20260916_213700.md) |
| Fixed-length request waves on one host (2× RTX 3090), v0.9.0 candidate | 512 / 512 requests completed and released, 2B and 35B models | [release notes](docs/release/v0.9.0.md) |
| Workspace tests (v0.9.0 gate) | `cargo test --workspace`: 1,374 passed, 0 failed | [release notes](docs/release/v0.9.0.md) |
| Sustained multi-request service across hosts, overload, soak (I1–I4) | **Not yet proven** — the last 64-request run completed 8 of 64 | [roadmap](docs/distributed-batching-roadmap.md#current-status) |

Throughput figures are not sealed yet: a performance baseline is only recorded
after the integrity stages (I1–I4) pass.

## Documentation

All design notes, plans and evidence reports on this branch are in English
(translated from the team's Korean originals). Quoted model prompts and outputs
keep their original Korean text next to an English rendering. Start with the
[overview](docs/overview.md), [architecture](docs/architecture.md),
[wire API](docs/api.md), [implementation map](docs/implementation.md) and the
sections below.

## License

Business Source License 1.1 — the same terms as the
[`kvasir-net`](https://github.com/louisevandan/kvasir-net/tree/kvasir-net) branch.
Non-monetized internal use is permitted; hosted, embedded or revenue-generating
use requires a commercial license. The license converts to Apache 2.0 on the
Change Date stated in [LICENSE](LICENSE). Third-party notices: [NOTICE](NOTICE).

---

## Development notes

## Current development goal and new-session starting point

[Fleet and latest upstream integration evidence](layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md)
[MiniMax M3 dense GGUF 4-stage load evidence](layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-dense-load.md)

**Release:** [v0.9.0 release notes](docs/release/v0.9.0.md) — a seal of what was verified, not completion of the §7 final checklist.

The top priority is **to run very large models on distributed nodes across several physical computers,
return correct responses under strong waves of continuous requests, and maximize useful generation TPS and GPU utilization**.
Unit tests, small models and several processes on one host are not proof of the final outcome.

1. [New-session rules](AGENTS.md)
2. [Current status and full execution roadmap](docs/distributed-batching-roadmap.md)
3. [Deterministic testing and real-hardware wave acceptance protocol](docs/distributed-batching-verification.md)
4. [P4 / adapter / llama.cpp layer isolation contract](docs/layer-isolation-contract.md)
5. [Full document map and authority](docs/document-map.md)

The initial audit baseline is `a9e1967fc`. For **verified progress, unverified changes, implementation stop points and resume conditions**, see
[the current status at the top of the roadmap](docs/distributed-batching-roadmap.md#current-status). This index does not duplicate the status table.
Do not use the old U/P stage tables, success figures or Chain/Hop descriptions as evidence that the current implementation is complete.

The communication layer for distributed inference. Agents carry work between
machines; a concrete adapter runs it.

The llama.cpp adapter and the CUDA, CPU and Metal backends are not the same abstraction layer.
The [layer isolation contract](docs/layer-isolation-contract.md) owns each layer's responsibilities and the scope of changes allowed during updates.
It is verified not only by include counts but also by state-mutation authority, public types, indirect linking, codecs and semantic regressions.

One process type: the agent. There is no controller and no node process — a
node lives inside an agent, and an agent reaching another agent is the same
path as an agent answering the outside.

```
OUTER ──▶ entry agent ──┬──▶ agent ──▶ node ──▶ adapter ──▶ backend
                        ├──▶ agent ──▶ node ──▶ adapter ──▶ backend
                        └──▶ agent ──▶ node ──▶ adapter ──▶ backend
```

## Backend boundary

The backend-specific implementation belongs behind the adapter boundary.
The following registry sketch describes the older service integration; the
current event implementation and its LOAD/identity obligations are mapped in
the current roadmap. Registering a factory alone does not prove integration.

```rust
// entrypoints/agent/src/adapters/mod.rs
registry.register_fn("llamacpp", |node| Arc::new(LlamaCpp::new(node)));
```

See [layers/adapters/README.md](layers/adapters/README.md) for the scoped interface
guide. A new backend must also pass the applicable current conformance and
worker tests; native engine capability is not scheduling policy.

## Running it

```text
p4-agent 0.0.0.0:52001 tcp://THIS_HOST:52001
p4-event-drive CONFIG.json ARTIFACT.json
```

The second argument is what a process calls itself, and every reply is
addressed to it — across machines it has to be an address the others can reach.

The [P4 model-loading module](tools/model-loading/README.md) owns its planner, reference evaluation, tests and local evidence.

Long-lived development and releases use `main`. The [cluster experiment composer](test/benchmarks/cluster-inference/README.md)
combines model templates, cluster placement, runtime identities, policies and workloads without model-specific branches.

`CONFIG.json` must describe the actual fleet/model/workload. The
[development harness](test/benchmarks/p4-4node/README.md) generates existing
development configurations; it does not yet enforce the final multi-host wave
contract. `P4_AGENT_SERVICE_RUNTIME` and `p4-drive` select the older service
comparison path. Its queue statistics are not proof of the default event path.

## Layout

| Path | What it is |
| --- | --- |
| [`layers/protocol`](layers/protocol) | The wire. An envelope every hop reads and a body only its destination does. |
| [`layers/agent`](layers/agent) | The core: one queue, workers, nodes, chains. |
| [`layers/service`](layers/service) | Body vocabulary, the agent's own duties, and the adapter registry. |
| [`layers/adapters`](layers/adapters) | The contract and everyone who implements it: [`adapter/`](layers/adapters/adapter) is what a node asks of a backend, with no dependencies and no backend names; the rest are backends. `mock` ships in every build. |
| [`entrypoints/agent`](entrypoints/agent) | The process. |
| [`tools/event-drive`](tools/event-drive) | Current event-path development driver. |
| [`tools/drive`](tools/drive) | Historical service-path comparison driver. |

## Documents

| Goal | File |
| --- | --- |
| Sole owner of the current goal, audit status and development order | [docs/distributed-batching-roadmap.md](docs/distributed-batching-roadmap.md) |
| Development plan, external HF adapter acceptance priority, release handoff, batching/model/acceleration investment decisions | [docs/external-analysis-improvement-plan.md](docs/external-analysis-improvement-plan.md) |
| MI250 and Hy3 batching diagnosis, implementation, concurrent real-hardware screening | [Integrated diagnosis](layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md) |
| Test constraints, mutation, strong multi-machine waves, correct responses, performance approval | [docs/distributed-batching-verification.md](docs/distributed-batching-verification.md) |
| P4/adapter per-layer responsibilities, native boundary, absorbing the impact of frequent llama.cpp updates | [docs/layer-isolation-contract.md](docs/layer-isolation-contract.md) |
| Status of every document, contract ownership, new-session reading order | [docs/document-map.md](docs/document-map.md) |
| Unifying node creation and removal under LOAD and UNLOAD — user agreement, change paths, verification, new-session implementation plan | [docs/node-load-lifecycle-plan.md](docs/node-load-lifecycle-plan.md) |
| Ledger of repeated-failure root causes, automatic blocks and reuse by later stages | [docs/deterministic-execution-register.md](docs/deterministic-execution-register.md) |
| Code comparison of the micro-batching proposal with the actual batcher, improvement candidates, local reproduction | [docs/batching-code-review.md](docs/batching-code-review.md) |
| Proposed acceptance of Studio observability requirements, extra traffic/performance budget, always-on aggregation and optional diagnostics | [docs/inference-observability-proposal.md](docs/inference-observability-proposal.md) |
| What the layer is and why it is shaped this way | [docs/overview.md](docs/overview.md) |
| Every crate, what it holds, and what is not built | [docs/implementation.md](docs/implementation.md) |
| The wire and the message vocabulary | [docs/api.md](docs/api.md) |
| Replacement self-describing event, node, adapter, llama.cpp and proof contract | [docs/event-protocol-v2.md](docs/event-protocol-v2.md) |
| Protocol audit, return routing, pipeline, options, and KV open decisions | [docs/protocol.md](docs/protocol.md) |
| OUTER sessions, heartbeat, and KV lifecycle boundary | [docs/protocol-outer.md](docs/protocol-outer.md) |
| Sealed staged MTP decision and implementation gate | [docs/protocol-mtp.md](docs/protocol-mtp.md) |
| How a message moves through an agent | [docs/architecture.md](docs/architecture.md) |
| Invariants and what breaks if they go | [docs/constraints.md](docs/constraints.md) |
| Decisions, and the defects behind them | [docs/internals.md](docs/internals.md) |
| What crosses the adapter boundary, and what was measured | [docs/adapter-boundary.md](docs/adapter-boundary.md) |
| llama.cpp stage memory ownership and legal graph cuts | [docs/llamacpp-stage-memory.md](docs/llamacpp-stage-memory.md) |
| Historical U/P backlog, defect history and contracts; not current execution order | [docs/adapter-restructure-plan.md](docs/adapter-restructure-plan.md) |
| Adapter batching layers: ledger, admission, composition, proof, and KV/persistence coupling | [docs/adapter-batching-layers.md](docs/adapter-batching-layers.md) |
| KV persisted-state store convention: record identity, directory layout, lifetime | [docs/kv-state-store-convention.md](docs/kv-state-store-convention.md) |
| Historical pre-event refactor handoff; not current status | [docs/continuous-inference-refactor-handoff.md](docs/continuous-inference-refactor-handoff.md) |
| Historical build plan; current order belongs to the distributed batching roadmap | [docs/plan.md](docs/plan.md) |
| Running it and driving a fleet | [docs/usage.md](docs/usage.md) |
| Testing | [docs/testing.md](docs/testing.md) |
| Distributed mock test plan | [docs/distributed-mock-test-plan.md](docs/distributed-mock-test-plan.md) |
| Deployment-owned adapter submission contract | [docs/deployment-adapter-contract.md](docs/deployment-adapter-contract.md) |
| OUTER acceptance test plan | [docs/outer-acceptance-test-plan.md](docs/outer-acceptance-test-plan.md) |
| 256-session local pipeline optimization evidence | [docs/p4-256-optimization.md](docs/p4-256-optimization.md) |
| Measured behaviour of the backend below | [docs/runtime-evidence.md](docs/runtime-evidence.md) |
| The revision that produced all this | [P4_REVISION_PLAN.md](P4_REVISION_PLAN.md) |

Internal HF build and run, and acceptance on both adapters: [HF integration guide](docs/hf-integration.md).

Release A: [staged test plan](tests/plans/release-a-20260914.md) · [A-RED/trace audit](tests/reports/release-a/20260914_040306.md).
Corpus and spec validation: [preparation results](tests/reports/release-a/20260914_041920.md).
Native preparation and both adapters: [regression results](tests/reports/release-a/20260914_043652.md).
Native cost observation: [actual CPU path and removal mutation](tests/reports/release-a/20260914_050400.md).
Current pin and deployment candidate: [token, CUDA/Metal, both adapters, cost attribution](tests/reports/release-a/20260914_054100.md).
Fleet preflight: [7-host connectivity, 6 native PLANs, model access blocked](tests/reports/release-a/20260914_062025.md).
App runtime environment recheck: [model access, native PLAN 8/8](tests/reports/release-a/20260914_102600.md).
Follow-up real-hardware run preparation: [reclaiming existing fleet nodes](tests/reports/release-a/20260915_005456.md).
Current fleet resumption: [rebuild, both adapters, 550B arm](tests/reports/release-a/20260915_011200.md).
Taking over concurrent HF work: [loading planner reproduction plan](layers/adapters/hf/tests/plans/loading-planner-20260915.md) · [verification](layers/adapters/hf/tests/reports/loading-planner/20260915_013600.md).

## HF adapter documents

| Document | Status |
| --- | --- |
| [layers/adapters/hf/adapter/docs/api.md](layers/adapters/hf/adapter/docs/api.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/docs/architecture.md](layers/adapters/hf/adapter/docs/architecture.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/docs/constraints.md](layers/adapters/hf/adapter/docs/constraints.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/docs/internals.md](layers/adapters/hf/adapter/docs/internals.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/docs/overview.md](layers/adapters/hf/adapter/docs/overview.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/docs/testing.md](layers/adapters/hf/adapter/docs/testing.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/docs/usage.md](layers/adapters/hf/adapter/docs/usage.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/README.md](layers/adapters/hf/adapter/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/AGENTS.md](layers/adapters/hf/AGENTS.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/api.md](layers/adapters/hf/docs/api.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/architecture.md](layers/adapters/hf/docs/architecture.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/constraints.md](layers/adapters/hf/docs/constraints.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/history/initial/api.md](layers/adapters/hf/docs/history/initial/api.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/architecture.md](layers/adapters/hf/docs/history/initial/architecture.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/bootstrap-evidence.md](layers/adapters/hf/docs/history/initial/bootstrap-evidence.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/constraints.md](layers/adapters/hf/docs/history/initial/constraints.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/decisions.md](layers/adapters/hf/docs/history/initial/decisions.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/development-plan.md](layers/adapters/hf/docs/history/initial/development-plan.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/HANDOFF.md](layers/adapters/hf/docs/history/initial/HANDOFF.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/internals.md](layers/adapters/hf/docs/history/initial/internals.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/overview.md](layers/adapters/hf/docs/history/initial/overview.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/references.md](layers/adapters/hf/docs/history/initial/references.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/roadmap.md](layers/adapters/hf/docs/history/initial/roadmap.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/testing.md](layers/adapters/hf/docs/history/initial/testing.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/usage.md](layers/adapters/hf/docs/history/initial/usage.md) | HF historical record |
| [layers/adapters/hf/docs/integration/README.md](layers/adapters/hf/docs/integration/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/internals.md](layers/adapters/hf/docs/internals.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/migration/README.md](layers/adapters/hf/docs/migration/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/models/qwen3_5_0_8b/README.md](layers/adapters/hf/docs/models/qwen3_5_0_8b/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/overview.md](layers/adapters/hf/docs/overview.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/quantization.md](layers/adapters/hf/docs/quantization.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/structure/README.md](layers/adapters/hf/docs/structure/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/testing.md](layers/adapters/hf/docs/testing.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/transport/framing/README.md](layers/adapters/hf/docs/transport/framing/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/usage.md](layers/adapters/hf/docs/usage.md) | HF configuration, contract, verification |
| [layers/adapters/hf/README.md](layers/adapters/hf/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/tests/plans/framing-20260913.md](layers/adapters/hf/tests/plans/framing-20260913.md) | HF configuration, contract, verification |
| [layers/adapters/hf/tests/plans/migration-20260914.md](layers/adapters/hf/tests/plans/migration-20260914.md) | HF configuration, contract, verification |
| [layers/adapters/hf/tests/plans/p4-integration-20260914.md](layers/adapters/hf/tests/plans/p4-integration-20260914.md) | HF configuration, contract, verification |
| [layers/adapters/hf/tests/plans/qwen3_5_0_8b-20260913.md](layers/adapters/hf/tests/plans/qwen3_5_0_8b-20260913.md) | HF configuration, contract, verification |
| [layers/adapters/hf/tests/reports/framing/20260913_174516.md](layers/adapters/hf/tests/reports/framing/20260913_174516.md) | HF historical record |
| [layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md](layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md) | HF historical record |
| [layers/adapters/hf/tests/reports/qwen3_5_0_8b/20260913_220709.md](layers/adapters/hf/tests/reports/qwen3_5_0_8b/20260913_220709.md) | HF historical record |
| [layers/adapters/hf/tests/reports/migration/20260914_120000.md](layers/adapters/hf/tests/reports/migration/20260914_120000.md) | HF internal integration verification and source cleanup results |

Connection reclaim WIP: [verification plan](tests/plans/release-a-transport-20260915.md) · [stop report after 3 failures](tests/reports/release-a/20260915_044735.md).

Routing through the ingress agent: [verification plan](tests/plans/ingress-envelope-20260915.md) · [envelope and both-adapter verification](tests/reports/release-a/20260915_113754.md).

Commission return context: [verification plan](tests/plans/return-context-20260915.md).
Return context implementation verification: [common contract, two adapters, independent mutation](tests/reports/release-a/20260915_121000.md).

Cluster return route: [test plan](tests/plans/cluster-envelope-20260915.md) · [9 hosts and MI250 SSH verification](tests/reports/release-a/20260915_124433.md).

Release A target change: [Qwen122B spec and corpus preparation](tests/reports/release-a/20260915_131432.md).

Release A FINISH resumption: [deterministic review, full regression, both adapters](tests/reports/release-a/20260915_142237.md).

Release A transport settlement: [test plan for unknown outcomes, hop receipts and reconnection](tests/plans/release-a-transport-reconciliation-20260915.md).

Release A transport settlement acceptance: [R1–R9, physical receipt recovery, final both-adapter run](tests/reports/release-a/20260915_183158.md).

Release A Qwen122B A-PLAN: [native PLAN on 3 physical hosts, shared pool, rejection before deployment](tests/reports/release-a/20260915_190631.md).

Release A Qwen122B A-LOAD: [actual allocation and reclaim on 3 physical hosts](tests/reports/release-a/20260915_195106.md).

Release A A-BYTES: [test plan for exact integer bytes of native result, completion, receipt and edge](tests/plans/release-a-bytes-20260915.md).

Release A A-BYTES B0: [Qwen122B physical result upper bound, actual LOAD, removal mutation](tests/reports/release-a/20260915_211000.md).

Release A A-BYTES B1: [versioned LOAD profile, actual remaining capacity, Qwen122B LOAD/reclaim](tests/reports/release-a/20260915_222346.md).

Release A A-BYTES B2: [completion group reservation before native, side-effect-free rejection, independent mutation](tests/reports/release-a/20260915_232600.md).

Release A A-BYTES B3/B4: [separated retention lifetimes, actual boundary, independent mutation, local power incident](tests/reports/release-a/20260915_235900.md).

Release A A-BYTES B5: [both-adapter regression, Qwen122B 3-host correct request, reclaim, remote preflight](tests/reports/release-a/20260916_013500.md).

Node LOAD/UNLOAD lifecycle M0: [current call paths and NL01–NL14 ownership mapping](tests/reports/node-load-lifecycle/20260916_014500.md).
M1: [deterministic execution plan](tests/plans/node-load-lifecycle-m1-20260916.md).
Typed adapter completion: [remote baseline and independent mutation report](tests/reports/node-load-lifecycle/20260916_015702.md).
M1 supervisor complete: [real TCP LOAD, UNLOAD, reclaim retention, workspace off/on, removal mutation](tests/reports/node-load-lifecycle/20260916_023059.md).
M2: [deterministic execution plan](tests/plans/node-load-lifecycle-m2-20260916.md), [llama.cpp/HF real worker, saturation, cleanup and independent mutation report](tests/reports/node-load-lifecycle/20260916_032621.md).
M3: [deterministic execution plan for the Rust/HF OUTER migration and legacy CREATE/DELETE removal](tests/plans/node-load-lifecycle-m3-20260916.md), [real Agent/HF child, workspace and independent mutation verification report](tests/reports/node-load-lifecycle/20260916_041848.md).
M4: [deterministic execution plan for real llama.cpp/HF generation, cancellation, release and reload](tests/plans/node-load-lifecycle-m4-20260916.md), [acceptance report: real models on both adapters, full regression, independent mutation](tests/reports/node-load-lifecycle/20260916_064306.md).

Release A Qwen122B H0 v3: [sealed benchmark-spec](test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v3.json) · [report: stronger H1 latency verdict, second INVALID, clean reclaim](tests/reports/release-a/20260916_104300.md). [H0 v2 spec](test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v2.json) and its [report](tests/reports/release-a/20260916_093739.md), and [H0 v1 spec](test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v1.json) and its [report](tests/reports/release-a/20260916_072100.md), are historical evidence.

Release A Qwen122B H1 run 1: [64-request concurrent spec RED, failure recovery, closed-loop correction implementation](tests/reports/release-a/20260916_084650.md).

Release A Qwen122B H1 run 2: [INVALID for missing judge SLO and absent terminal artifact; clean UNLOAD and task resource reclaim](tests/reports/release-a/20260916_104300.md).

Release A integrity-first resumption: [detailed I0–I4 test plan](tests/plans/release-a-integrity-first-20260916.md) · [execution contract v2](test/benchmarks/cluster-inference/release-a/integrity-test-spec-qwen122b-i0-v2.json). Do not start performance candidates or H5 before I0–I4 are GREEN.

Release A integrity test contract: [report: 14 real-hardware arms, contract/verdict mutation, roadmap rebalancing](tests/reports/release-a/20260916_131446.md).

Release A I0 raw execution contract: [sequential short/medium/long runs within a single LOAD; request/batch/stage/GPU verdicts](tests/reports/release-a/20260916_133038.md).

Release A Qwen122B H0 v5: [deterministic observation barrier, absolute execution window, raw GPU/resource evidence, seal of three remote binaries](test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v5.json) · [verification report](tests/reports/release-a/20260916_143000.md). This is a historical seal, not authority for a new LOAD.

Release A I0 first real-hardware run, historical evidence: [correct answers 1/3 RED, reverse check against the single-host baseline, new LOADs blocked at the time](tests/reports/release-a/20260916_161600.md).

Release A I0 wrong-answer root-cause diagnosis: [pre-sealed observation and verdict plan](tests/plans/release-a-quality-cause-20260916.md).

Release A I0 wrong-answer root cause confirmed: [6 separated remote diagnostics, exact fact extraction versus arithmetic failure, clean reclaim](tests/reports/release-a/20260916_172000.md).

Release A OUTER wrong-answer verdict boundary: [JSON oracle binding, remote regression, product fix incomplete](tests/reports/release-a/20260916_180500.md).

Past Qwen122B H0 v7/I0: [sealed spec](test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v7.json) · [report: single-request correct answers 3/3 on 3 physical hosts, useful TPS correction, clean reclaim](tests/reports/release-a/20260916_204300.md). [H0 v6 past report and correction](tests/reports/release-a/20260916_195500.md).

H0 v8/I1 pre-run contract: [sealed spec](test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v8.json) · [report: 21 non-model gates, full 64-request path, before I0 re-verification](tests/reports/release-a/20260916_210000.md). The I1 real-hardware run has not been run yet.

Current Qwen122B H0 v8/I0: [report: single request 3/3 on 3 physical hosts, clean reclaim, performance measurement](tests/reports/release-a/20260916_213700.md). The I1–I4 real-hardware runs and full integrity remain.

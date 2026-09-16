# P4

## 현재 개발 목표와 새 세션 시작점

[Fleet 및 최신 upstream 통합 증거](layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md)
[MiniMax M3 dense GGUF 4-stage 적재 증거](layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-dense-load.md)

**릴리즈:** [v0.9.0 릴리즈 노트](docs/release/v0.9.0.md) — 검증한 것의 봉인이며 §7 최종 체크리스트의 달성이 아니다.

최우선 목표는 **초대형 모델을 여러 물리 컴퓨터의 분산 노드에서 실행하고,
강한 연속 요청 웨이브에 정상 응답을 내면서 유효 생성 TPS와 GPU 활용을 최대화하는 것**이다.
단위 시험·작은 모델·한 호스트의 여러 프로세스는 최종 성과 증명이 아니다.

1. [새 세션 규칙](AGENTS.md)
2. [현재 상태와 전체 실행 로드맵](docs/distributed-batching-roadmap.md)
3. [결정론적 시험·실기 웨이브 수용 규약](docs/distributed-batching-verification.md)
4. [P4·어댑터·llama.cpp 계층 격리 계약](docs/layer-isolation-contract.md)
5. [전체 문서 안내도와 권위](docs/document-map.md)

최초 감사 기준은 `a9e1967fc`다. **검증된 진전·미검증 변경·구현 중단 지점과 재개 조건**은
[로드맵 맨 앞의 현재 상태](docs/distributed-batching-roadmap.md#current-status)를 확인한다. 이 색인에 상태표를 복제하지 않는다.
과거 U/P 단계표·성공 수치·Chain/Hop 설명을 현재 구현의 완료 증거로 사용하지 않는다.

The communication layer for distributed inference. Agents carry work between
machines; a concrete adapter runs it.

llama.cpp 어댑터와 CUDA·CPU·Metal backend는 같은 추상층이 아니다.
각 층의 책임과 업데이트 수정 허용 범위는 [계층 격리 계약](docs/layer-isolation-contract.md)이 소유하며,
include 수뿐 아니라 상태 변경 권한·public 타입·간접 링크·codec·의미 회귀로 검증한다.

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
| 현재 목표·감사 상태·개발 순서의 단독 소유 | [docs/distributed-batching-roadmap.md](docs/distributed-batching-roadmap.md) |
| 개발 계획·외부 HF 어댑터 수용 우선·릴리즈 인수인계·배치/모델/가속 투자 판단 | [docs/external-analysis-improvement-plan.md](docs/external-analysis-improvement-plan.md) |
| MI250·Hy3 배치 진단·구현·동시 실기 선별 | [통합 진단](layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md) |
| 시험 제약·mutation·다중 머신 강한 웨이브·정상 응답·성능 승인 | [docs/distributed-batching-verification.md](docs/distributed-batching-verification.md) |
| P4/어댑터 층별 책임·native 경계·잦은 llama.cpp 업데이트 충격 흡수 | [docs/layer-isolation-contract.md](docs/layer-isolation-contract.md) |
| 모든 문서의 지위·계약 소유·새 세션 읽기 순서 | [docs/document-map.md](docs/document-map.md) |
| LOAD·UNLOAD로 노드 생성·제거 통합 — 사용자 합의·변경 경로·검증·새 세션 구현계획 | [docs/node-load-lifecycle-plan.md](docs/node-load-lifecycle-plan.md) |
| 반복 실패의 원인·자동 차단·다음 단계 재사용 장부 | [docs/deterministic-execution-register.md](docs/deterministic-execution-register.md) |
| 마이크로 배치 제안과 실제 배처의 코드 대조·개선 후보·로컬 재현 | [docs/batching-code-review.md](docs/batching-code-review.md) |
| Studio 관측 요구 수용안·추가 트래픽/성능 예산·상시 집계와 선택 진단 | [docs/inference-observability-proposal.md](docs/inference-observability-proposal.md) |
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

내부 HF 빌드·실행 및 양쪽 어댑터 수용: [HF 통합 안내](docs/hf-integration.md).

Release A: [단계별 시험 계획](tests/plans/release-a-20260914.md) · [A-RED/trace 감사](tests/reports/release-a/20260914_040306.md).
Corpus와 명세 검증: [준비 결과](tests/reports/release-a/20260914_041920.md).
Native 준비와 양쪽 어댑터: [회귀 결과](tests/reports/release-a/20260914_043652.md).
Native 비용 관측: [CPU 실제 경로와 제거 변이](tests/reports/release-a/20260914_050400.md).
현재 pin과 배포 후보: [token·CUDA/Metal·양쪽 어댑터·비용 귀속](tests/reports/release-a/20260914_054100.md).
Fleet 사전 검사: [7-host 연결·native PLAN6개·모델 접근 차단](tests/reports/release-a/20260914_062025.md).
앱 실행 환경 재확인: [모델 접근·native PLAN8/8](tests/reports/release-a/20260914_102600.md).
후속 실기 준비: [기존 fleet 노드 회수](tests/reports/release-a/20260915_005456.md).
현재 fleet 재개: [재빌드·양쪽 어댑터·550B arm](tests/reports/release-a/20260915_011200.md).
동시 HF 작업 인수: [로딩 계획기 재현 계획](layers/adapters/hf/tests/plans/loading-planner-20260915.md) · [검증](layers/adapters/hf/tests/reports/loading-planner/20260915_013600.md).

## HF 어댑터 문서

| 문서 | 지위 |
| --- | --- |
| [layers/adapters/hf/adapter/docs/api.md](layers/adapters/hf/adapter/docs/api.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/docs/architecture.md](layers/adapters/hf/adapter/docs/architecture.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/docs/constraints.md](layers/adapters/hf/adapter/docs/constraints.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/docs/internals.md](layers/adapters/hf/adapter/docs/internals.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/docs/overview.md](layers/adapters/hf/adapter/docs/overview.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/docs/testing.md](layers/adapters/hf/adapter/docs/testing.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/docs/usage.md](layers/adapters/hf/adapter/docs/usage.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/README.md](layers/adapters/hf/adapter/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/AGENTS.md](layers/adapters/hf/AGENTS.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/api.md](layers/adapters/hf/docs/api.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/architecture.md](layers/adapters/hf/docs/architecture.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/constraints.md](layers/adapters/hf/docs/constraints.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/history/initial/api.md](layers/adapters/hf/docs/history/initial/api.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/architecture.md](layers/adapters/hf/docs/history/initial/architecture.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/bootstrap-evidence.md](layers/adapters/hf/docs/history/initial/bootstrap-evidence.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/constraints.md](layers/adapters/hf/docs/history/initial/constraints.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/decisions.md](layers/adapters/hf/docs/history/initial/decisions.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/development-plan.md](layers/adapters/hf/docs/history/initial/development-plan.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/HANDOFF.md](layers/adapters/hf/docs/history/initial/HANDOFF.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/internals.md](layers/adapters/hf/docs/history/initial/internals.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/overview.md](layers/adapters/hf/docs/history/initial/overview.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/references.md](layers/adapters/hf/docs/history/initial/references.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/roadmap.md](layers/adapters/hf/docs/history/initial/roadmap.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/testing.md](layers/adapters/hf/docs/history/initial/testing.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/usage.md](layers/adapters/hf/docs/history/initial/usage.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/integration/README.md](layers/adapters/hf/docs/integration/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/internals.md](layers/adapters/hf/docs/internals.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/migration/README.md](layers/adapters/hf/docs/migration/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/models/qwen3_5_0_8b/README.md](layers/adapters/hf/docs/models/qwen3_5_0_8b/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/overview.md](layers/adapters/hf/docs/overview.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/quantization.md](layers/adapters/hf/docs/quantization.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/structure/README.md](layers/adapters/hf/docs/structure/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/testing.md](layers/adapters/hf/docs/testing.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/transport/framing/README.md](layers/adapters/hf/docs/transport/framing/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/usage.md](layers/adapters/hf/docs/usage.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/README.md](layers/adapters/hf/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/tests/plans/framing-20260913.md](layers/adapters/hf/tests/plans/framing-20260913.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/tests/plans/migration-20260914.md](layers/adapters/hf/tests/plans/migration-20260914.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/tests/plans/p4-integration-20260914.md](layers/adapters/hf/tests/plans/p4-integration-20260914.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/tests/plans/qwen3_5_0_8b-20260913.md](layers/adapters/hf/tests/plans/qwen3_5_0_8b-20260913.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/tests/reports/framing/20260913_174516.md](layers/adapters/hf/tests/reports/framing/20260913_174516.md) | HF 역사 기록 |
| [layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md](layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md) | HF 역사 기록 |
| [layers/adapters/hf/tests/reports/qwen3_5_0_8b/20260913_220709.md](layers/adapters/hf/tests/reports/qwen3_5_0_8b/20260913_220709.md) | HF 역사 기록 |
| [layers/adapters/hf/tests/reports/migration/20260914_120000.md](layers/adapters/hf/tests/reports/migration/20260914_120000.md) | HF 내부 통합 검증·원본 정리 결과 |

연결 회수 WIP: [검증 계획](tests/plans/release-a-transport-20260915.md) · [실패3회 중단 보고](tests/reports/release-a/20260915_044735.md).

접수 에이전트 경유: [검증 계획](tests/plans/ingress-envelope-20260915.md) · [엔벨롭·양쪽 어댑터 검증](tests/reports/release-a/20260915_113754.md).

의뢰 반환 문맥: [검증 계획](tests/plans/return-context-20260915.md).
반환 문맥 구현 검증: [공통 계약·두 어댑터·독립 변이](tests/reports/release-a/20260915_121000.md).

클러스터 반환 경로: [시험 계획](tests/plans/cluster-envelope-20260915.md) · [9대·MI250 SSH 검증](tests/reports/release-a/20260915_124433.md).

Release A 대상 변경: [Qwen122B 명세·corpus 준비](tests/reports/release-a/20260915_131432.md).

Release A FINISH 재개: [결정론적 검토·전체 회귀·양쪽 어댑터](tests/reports/release-a/20260915_142237.md).

Release A 전송 정산: [불명 결과·hop receipt·재연결 시험 계획](tests/plans/release-a-transport-reconciliation-20260915.md).

Release A 전송 정산 수용: [R1–R9·물리 receipt 복구·최종 양쪽 어댑터](tests/reports/release-a/20260915_183158.md).

Release A Qwen122B A-PLAN: [3물리 host native PLAN·공유 pool·배포 전 거부](tests/reports/release-a/20260915_190631.md).

Release A Qwen122B A-LOAD: [3물리 host 실제 allocation·회수](tests/reports/release-a/20260915_195106.md).

Release A A-BYTES: [native result·completion·receipt·edge 정수 byte 시험 계획](tests/plans/release-a-bytes-20260915.md).

Release A A-BYTES B0: [Qwen122B physical result 상한·실제 LOAD·제거 변이](tests/reports/release-a/20260915_211000.md).

Release A A-BYTES B1: [versioned LOAD profile·실제 잔여 용량·Qwen122B LOAD/회수](tests/reports/release-a/20260915_222346.md).

Release A A-BYTES B2: [native 전 completion group 예약·거부 무효과·독립 변이](tests/reports/release-a/20260915_232600.md).

Release A A-BYTES B3/B4: [보존 수명 분리·실제 경계·독립 변이·로컬 전원 사고](tests/reports/release-a/20260915_235900.md).

Release A A-BYTES B5: [양쪽 어댑터 회귀·Qwen122B 3-host 정상 요청·회수·원격 사전검사](tests/reports/release-a/20260916_013500.md).

노드 LOAD·UNLOAD 수명 M0: [현재 호출 경로·NL01–NL14 소유권 매핑](tests/reports/node-load-lifecycle/20260916_014500.md).
M1: [결정론적 실행계획](tests/plans/node-load-lifecycle-m1-20260916.md).
Typed adapter completion: [원격 baseline·독립 변이 보고](tests/reports/node-load-lifecycle/20260916_015702.md).
M1 supervisor 완료: [실제 TCP LOAD·UNLOAD·회수 보존·workspace off/on·제거 변이](tests/reports/node-load-lifecycle/20260916_023059.md).
M2: [결정론적 실행계획](tests/plans/node-load-lifecycle-m2-20260916.md), [llama.cpp/HF 실제 worker·포화·cleanup·독립 변이 보고](tests/reports/node-load-lifecycle/20260916_032621.md).
M3: [Rust/HF OUTER 이관·legacy CREATE/DELETE 제거 결정론적 실행계획](tests/plans/node-load-lifecycle-m3-20260916.md), [실제 Agent/HF child·workspace·독립 변이 검증 보고](tests/reports/node-load-lifecycle/20260916_041848.md).
M4: [실제 llama.cpp/HF 생성·취소·해제·재적재 결정론적 실행계획](tests/plans/node-load-lifecycle-m4-20260916.md), [두 adapter 실제 모델·전체 회귀·독립 변이 수용 보고](tests/reports/node-load-lifecycle/20260916_064306.md).

Release A Qwen122B H0 v3: [봉인 benchmark-spec](test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v3.json) · [H1 latency 판정 보강·2차 INVALID·정상 회수 보고](tests/reports/release-a/20260916_104300.md). [H0 v2 spec](test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v2.json)과 [보고](tests/reports/release-a/20260916_093739.md), [H0 v1 spec](test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v1.json)과 [보고](tests/reports/release-a/20260916_072100.md)는 역사 증거다.

Release A Qwen122B H1 1차: [64건 동시 명세 RED·실패 recovery·closed-loop 교정 구현](tests/reports/release-a/20260916_084650.md).

Release A Qwen122B H1 2차: [판정기 SLO 누락·terminal artifact 부재 INVALID, 정상 UNLOAD·작업 자원 회수](tests/reports/release-a/20260916_104300.md).

Release A 무결성 우선 재개: [I0–I4 정밀 시험계획](tests/plans/release-a-integrity-first-20260916.md) · [실행 계약](test/benchmarks/cluster-inference/release-a/integrity-test-spec-qwen122b-i0-v1.json). I0–I4 GREEN 전에는 성능 후보와 H5를 시작하지 않는다.

Release A 무결성 시험 계약: [14개 실기 arm·계약/판정 변이·로드맵 재조정 보고](tests/reports/release-a/20260916_131446.md).

Release A I0 원시 실행 계약: [단일 LOAD 단·중·장 순차 실행·요청/배치/stage/GPU 판정](tests/reports/release-a/20260916_133038.md).

Release A Qwen122B H0 v4: [절대 실행창·원시 GPU/자원 증거 builder·세 원격 바이너리 봉인](test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v4.json) · [검증 보고](tests/reports/release-a/20260916_135000.md). 모델 LOAD 전 gate만 GREEN이며 I0 실기는 다음 단계다.

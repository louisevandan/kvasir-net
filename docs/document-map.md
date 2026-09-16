# 전체 문서 안내도와 권위

2026-09-06 정리. 이 파일은 저장소 소유 Markdown 전체의 **분류·읽기 경로·계약 소유**를 관리한다.
이관은 기존 기록을 지우는 것이 아니다. 아래 상태 표시는 모든 과거 본문의 코드 주장을 새로 인증했다는 뜻도 아니다.
현재 코드 감사 범위와 열린 결함은 로드맵과 해당 날짜 evidence에 명시한다.

## 1. 새 세션에서 읽을 것

1. [AGENTS.md](../AGENTS.md): 저장소 작업·검증·보고 규칙.
2. [분산 배치 로드맵의 현재 상태](distributed-batching-roadmap.md#current-status): 검증된 진전/미검증 작업/중단과 재개 조건. 시간순 기록의 옛 “다음”부터 실행하지 않는다.
3. [분산 배치 검증 규약](distributed-batching-verification.md): 결정론적 반례와 실기 승인 조건.
4. [계층 격리 계약](layer-isolation-contract.md): P4/어댑터/native/llama/backend 책임과 upstream 충격 흡수.
5. 새 릴리즈 개발은 [단일 개발 계획](external-analysis-improvement-plan.md#fresh-session)의 인수인계를 읽고 먼저 [HF 수용 계약](distributed-batching-verification.md#hf-integration-contract), 이후 [A 수용 계약](distributed-batching-verification.md#release-a-contract)을 읽는다.
6. 아래 분야 소유 문서와 실제 구현 경로: 지금 고치는 계약만 필요한 만큼 읽는다.

초대형 모델·다중 물리 컴퓨터·강한 요청 웨이브·정상 응답 전문이 최종 성과의 조건이다.
작은 모델과 한 머신 실측, simulator 완주, 과거 U/P 번호를 새로운 완료 기준으로 끌어올리지 않는다.

## 2. 계약 소유와 충돌 해결

| 주제 | 단독 소유 / 사용 규칙 |
| --- | --- |
| 현재 목표·상태·개발 순서·단계 승격 | [로드맵](distributed-batching-roadmap.md). 다른 문서는 순서를 복제하지 않고 링크한다. |
| 시험 입력·mutation·판정·실기 웨이브·보고 지표 | [검증 규약](distributed-batching-verification.md). “실행 명령 있음”과 “현재 통과”를 구분한다. |
| 층별 역할·허용 의존·public 타입·업데이트 적응 경계 | [계층 격리 계약](layer-isolation-contract.md). 기존 include 부채 수치를 전체 구조 격리로 읽지 않는다. |
| 문서의 지위·찾는 경로 | 이 문서. 새 문서를 만들면 아래 전체 목록도 함께 등록한다. |
| Studio 관측 요구와 비용 제한 후보 | [관측 수용안](inference-observability-proposal.md). 설계 제안이며 구현·기본값·실기 승인 또는 기존 실행 순서 변경이 아님. |
| event wire/forwarding의 backend 중립 경계 | [event-protocol-v2](event-protocol-v2.md); 본문의 당시 proof order는 현 로드맵을 대체하지 않는다. |
| 어댑터 배치 계층·메커니즘/정책 소유 | [adapter-batching-layers](adapter-batching-layers.md); 미구현 목표 계약 포함. |
| 영속 identity·namespace·CONTROL·2PC·스냅샷/LCP | [kv-state-store-convention](kv-state-store-convention.md); 기능별 K 분기 구현/장애 게이트 통과 전 활성화 금지. |
| stage memory·합법적 cut | [llamacpp-stage-memory](llamacpp-stage-memory.md); 모델/backend 조합별 실제 conformance 필요. |
| MTP/OUTER/deployment 경계 | 각 분야 문서. 현재 켜진 기능만 완료로 보고하며, 비활성 미감사 기능은 fail-closed 유지. |
| 실행 방법 | [testing](testing.md), 구성요소 README. 현재 event 진입점과 실제 명령 인자를 코드로 재확인한다. |
| 과거 원인·대안·U/P 결함 대장 | [adapter-restructure-plan](adapter-restructure-plan.md) 등 역사 문서. 요구사항은 새 로드맵에 이관됐지만 과거 순서는 폐기됐다. |
| 과거 시험·실기 측정값 | [runtime-evidence](runtime-evidence.md)와 날짜별 evidence. source/binary/model/workload/topology 범위를 넘어 일반화하지 않는다. |

사용자 지시가 최우선이다. 문서끼리 모순되면 마지막 날짜만으로 선택하지 말고 이 표의 소유자와
로드맵의 명시적 이관을 먼저 확인한다. 코드와 계약이 다르면 코드가 자동 정답도, 문서가 구현 증거도 아니다.
차이를 반례/미완 항목으로 기록하고 해당 구현 단계에서 처리한다.

분야 계약 수정 시 소유 문서를 바꾸고 소비 문서는 링크/짧은 적용 범위만 갱신한다.
과거 증거 수치·실패 이력은 삭제하지 않는다. 옛 “then stop”, 고정 4노드, GPU 수만큼만 분할,
홉은 항상 싸다는 결론은 날짜 한정 가설이지 현 제품 규칙이 아니다.

## 3. 전체 목록

현재 등록은 79개다. 기존 추적 문서 73개를 분류하고 이번 인수인계 문서/증거 6개를 추가했다.
아래 파일들의 상세 본문은 각 역할의 범위에서만 읽는다. 수치는 편의 요약이고 실제 누락 여부는 문서 게이트가 검사한다.

| 문서 | 지위 |
| --- | --- |
| [AGENTS.md](../AGENTS.md) | 새 세션 규칙 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md) | fleet/NAS 실측과 최신 upstream 통합의 진행·미승인 게이트 |
| [docs/adapter-batching-layers.md](adapter-batching-layers.md) | 분야 계약·구현과 구별 |
| [docs/adapter-boundary.md](adapter-boundary.md) | 분야 계약·구현과 구별 |
| [docs/adapter-restructure-plan.md](adapter-restructure-plan.md) | 역사·구 계획 |
| [docs/api.md](api.md) | 경로별 참고·재감사 필요 |
| [docs/architecture.md](architecture.md) | 경로별 참고·재감사 필요 |
| [docs/node-load-lifecycle-plan.md](node-load-lifecycle-plan.md) | 완료된 구현계획: M0–M4 실제 두 adapter 모델·문서 수용 완료, 전체 적재 조율은 OUTER 책임 |
| [docs/deterministic-execution-register.md](deterministic-execution-register.md) | 활성 계약: 반복 실패의 원인 증거를 자동 사전 차단과 다음 단계 시험으로 재사용 |
| [docs/batching-code-review.md](batching-code-review.md) | 2026-09-13 배치 selector·worker·native 전달 경계 코드 검토; 제안과의 차이·개선 후보·로컬 재현, 실행 순서/성능 승격 아님 |
| [docs/constraints.md](constraints.md) | 경로별 참고·재감사 필요 |
| [docs/continuous-inference-refactor-handoff.md](continuous-inference-refactor-handoff.md) | 역사·구 계획 |
| [docs/deployment-adapter-contract.md](deployment-adapter-contract.md) | 분야 계약·구현과 구별 |
| [docs/distributed-batching-roadmap.md](distributed-batching-roadmap.md) | 현재 목표·상태·순서 소유 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md) | MI250·Hy3 진단·bounded 선택 구현·동시 실기 선별; H5 성능/서비스 승인 미완 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-dense-load.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-dense-load.md) | MSA가 누락된 구형 MiniMax M3 GGUF의 Windows CUDA 4-stage 적재·추론과 MSA 재감사 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-msa-distributed-rejection.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-msa-distributed-rejection.md) | 정상 MSA MiniMax M3 GGUF의 CUDA·Metal 분산 계획/LOAD 거부와 불완전 opt-in 회수 |
| [docs/distributed-batching-verification.md](distributed-batching-verification.md) | 시험·실기 판정 소유 |
| [docs/distributed-mock-test-plan.md](distributed-mock-test-plan.md) | 부분 시험 계획 |
| [docs/document-map.md](document-map.md) | 문서 분류·권위 소유 |
| [docs/event-protocol-v2.md](event-protocol-v2.md) | 분야 계약·구현과 구별 |
| [docs/external-analysis-improvement-plan.md](external-analysis-improvement-plan.md) | 단일 개발 계획·외부 HF 수용 우선·Release A 구현 범위/새 세션 인수인계, 배치 G1–G6·희소 상태·DFlash/DSpark 투자 근거; 실행 순서는 로드맵, 판정은 검증 규약 소유 |
| [docs/implementation.md](implementation.md) | 경로별 참고·재감사 필요 |
| [docs/inference-observability-proposal.md](inference-observability-proposal.md) | Studio 관측 요구의 상시 집계·요청 요약·선택 진단과 트래픽/성능 예산 제안; 미구현·미실측 |
| [docs/internals.md](internals.md) | 경로별 참고·재감사 필요 |
| [docs/kv-state-store-convention.md](kv-state-store-convention.md) | 분야 계약·구현과 구별 |
| [docs/layer-isolation-contract.md](layer-isolation-contract.md) | 층별 책임·upstream 격리 소유 |
| [docs/llamacpp-stage-memory.md](llamacpp-stage-memory.md) | 분야 계약·구현과 구별 |
| [docs/outer-acceptance-test-plan.md](outer-acceptance-test-plan.md) | 부분 시험 계획 |
| [docs/overview.md](overview.md) | 경로별 참고·재감사 필요 |
| [docs/p4-256-optimization.md](p4-256-optimization.md) | 역사·구 계획 |
| [docs/plan.md](plan.md) | 역사·구 계획 |
| [docs/presentation/p4-structure.md](presentation/p4-structure.md) | 구조 설명 — 일반 개발자용 덱의 Markdown 판 |
| [docs/protocol-mtp.md](protocol-mtp.md) | 분야 계약·구현과 구별 |
| [docs/protocol-outer.md](protocol-outer.md) | 분야 계약·구현과 구별 |
| [docs/protocol.md](protocol.md) | 경로별 참고·재감사 필요 |
| [docs/runtime-evidence.md](runtime-evidence.md) | 증거 색인 |
| [docs/testing.md](testing.md) | 게이트 실행 안내 |
| [docs/usage.md](usage.md) | 경로별 참고·재감사 필요 |
| [entrypoints/README.md](../entrypoints/README.md) | 구성요소 안내 |
| [layers/adapters/adapter/README.md](../layers/adapters/adapter/README.md) | 구성요소 안내 |
| [layers/adapters/llamacpp/README.md](../layers/adapters/llamacpp/README.md) | 구성요소 안내 |
| [layers/adapters/llamacpp/staged/compat/1269cb1ff/README.md](../layers/adapters/llamacpp/staged/compat/1269cb1ff/README.md) | pin별 호환 기록 |
| [layers/adapters/llamacpp/staged/compat/3e3a7a416/README.md](../layers/adapters/llamacpp/staged/compat/3e3a7a416/README.md) | pin별 호환 기록 |
| [layers/adapters/llamacpp/staged/compat/4308a4f03/README.md](../layers/adapters/llamacpp/staged/compat/4308a4f03/README.md) | pin별 호환 기록 |
| [layers/adapters/llamacpp/staged/compat/d7a207411/README.md](../layers/adapters/llamacpp/staged/compat/d7a207411/README.md) | pin별 호환 기록 |
| [layers/adapters/llamacpp/staged/compat/ef6876693/README.md](../layers/adapters/llamacpp/staged/compat/ef6876693/README.md) | pin별 호환 기록 |
| [layers/adapters/llamacpp/staged/compat/fe2adf0e7/README.md](../layers/adapters/llamacpp/staged/compat/fe2adf0e7/README.md) | pin별 호환 기록 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-5000-token-prefill-generation.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-5000-token-prefill-generation.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-cuda-toolchain.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-cuda-toolchain.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-current-nonmtp-options-four-node.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-current-nonmtp-options-four-node.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-final-four-node.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-final-four-node.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-final-live-audit.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-final-live-audit.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-four-node-lap-fix.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-four-node-lap-fix.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-gate5-pass-through.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-gate5-pass-through.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-independent-nonmtp-four-node.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-independent-nonmtp-four-node.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-kv-manifest-p1.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-kv-manifest-p1.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-kv-unified-forwarding.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-kv-unified-forwarding.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-latest-cuda-four-node-regression.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-latest-cuda-four-node-regression.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-logits.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-logits.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-minimax-multishard-partial-stage.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-minimax-multishard-partial-stage.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-model-vram.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-model-vram.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp-auxiliary-ownership-probe.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp-auxiliary-ownership-probe.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp-speculative-minimum-slice-audit.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp-speculative-minimum-slice-audit.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-multi-stage-kv-e2e.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-multi-stage-kv-e2e.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-native-context-checkpoint.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-native-context-checkpoint.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-options-wire-four-node-regression.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-options-wire-four-node-regression.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-remote-four-node.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-remote-four-node.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-request-options-semantics.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-request-options-semantics.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-request-sampling-options.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-request-sampling-options.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-s-model-qwen2.5-1.5b.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-s-model-qwen2.5-1.5b.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-state-store-local.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-state-store-local.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-batched-decode-throughput.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-batched-decode-throughput.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-decode-hop-cost-decomposition.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-decode-hop-cost-decomposition.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-four-node-35b-service-reference.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-four-node-35b-service-reference.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-03-load-and-batching.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-03-load-and-batching.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-04-35b-and-the-width-collapse.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-04-35b-and-the-width-collapse.md) | 날짜·환경 한정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md) | 기준 커밋 검수 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-07-head-verification-and-3090x2-ladder.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-07-head-verification-and-3090x2-ladder.md) | HEAD `2ed9b71d4` 컴파일·시험·변이 판정과 3090×2 실기 사다리 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-08-noalloc-plan-underestimate.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-08-noalloc-plan-underestimate.md) | `no_alloc` 메모리 계획의 compute 과소 보고 결함과 `0025` 수정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-08-model-load-catalog.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-08-model-load-catalog.md) | 모델 33종 실기 적재와 컨텍스트별 메모리 실측 보고 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-measurement-trust-recovery.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-measurement-trust-recovery.md) | 철회된 폭 상관관계 정정과 측정을 가리던 세 결함의 수정 증거 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-control-receipt-budget.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-control-receipt-budget.md) | 해제·정산을 막던 제어 응답 예산의 원인 확정·수정·변이 검증 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-pressure-measured-baseline.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-pressure-measured-baseline.md) | 처음 완주한 `pressure`의 응답·TPS·배치 포화·GPU 실측 기준선 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-saturation-and-utilisation.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-saturation-and-utilisation.md) | 모델 크기·분할·resident·발행 정책·도착 패턴을 바꾼 포화·사용률 실험 10회 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-partial-result-preservation.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-partial-result-preservation.md) | 실패한 실행이 부분 결과를 잃던 결함의 원인 확정·수정·변이 검증 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-noalloc-recurrent-residency.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-noalloc-recurrent-residency.md) | 계획 모드가 recurrent 상태를 실제 할당하던 상류 결함의 원인 확정과 compat 패치 |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-release-gate-v0.9.0.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-release-gate-v0.9.0.md) | v0.9.0 릴리즈 산출물의 결속과 3090×2 실기 게이트 5종 판정 |
| [docs/release/v0.9.0.md](release/v0.9.0.md) | v0.9.0 릴리즈 노트 — 포함 변경·검증된 것·검증하지 않은 것·알려진 제약·지원 구성 |
| [layers/adapters/llamacpp/staged/scripts/validation/README.md](../layers/adapters/llamacpp/staged/scripts/validation/README.md) | 구성요소 안내 |
| [layers/adapters/llamacpp/staged/server/tests/contract/README.md](../layers/adapters/llamacpp/staged/server/tests/contract/README.md) | 구성요소 안내 |
| [layers/adapters/README.md](../layers/adapters/README.md) | 구성요소 안내 |
| [layers/protocol/README.md](../layers/protocol/README.md) | 구성요소 안내 |
| [layers/README.md](../layers/README.md) | 구성요소 안내 |
| [llamaAdaper.md](../llamaAdaper.md) | 역사·구 계획 |
| [P4_REVISION_PLAN.md](../P4_REVISION_PLAN.md) | 역사·구 계획 |
| [README.md](../README.md) | 진입점 |
| [test/benchmarks/p4-4node/README.md](../test/benchmarks/p4-4node/README.md) | 개발 하네스 안내 |
| [test/benchmarks/model-catalog/README.md](../test/benchmarks/model-catalog/README.md) | OUTER 모델 적재 파라미터와 메모리 실측 카탈로그 |
| [test/benchmarks/cluster-inference/README.md](../test/benchmarks/cluster-inference/README.md) | main 공통 실험 구성기·모델/클러스터/runtime/정책/워크로드 분리 |
| [tools/model-loading/README.md](../tools/model-loading/README.md) | P4 모델 로딩 함수·참조/정책 평가·시험·로컬 증거 |
| [tools/README.md](../tools/README.md) | 구성요소 안내 |

## 4. 유지 규칙

- 프로젝트 소유 Markdown을 추가/이동/삭제하면 이 목록과 루트 README의 docs 페이지 색인을 함께 갱신한다.
- 각 문서 상단에 현재 계약/목표/경로별 참고/역사 증거의 지위를 표시한다.
- docs-lint는 이 목록의 파일 누락·깨진 대상, 파일 내 혼합 EOL, 일부 폐기 문구/앵커만 검사한다.
  자연어 의미 충돌, source 검증 여부, 미구현 T/H 시험은 자동 인증하지 않는다.
- 기본 게이트는 Git 추적 파일만 검사한다. 새 문서는 staging 전에도 `--all`로 별도 검사한다.
  무관한 untracked 초안을 공식 목록/커밋에 강제로 넣지 않는다.
- 빌드/벤더/임시 증거는 전수 목록 대상이 아니다. 장기 증거는 날짜별 문서와 복구 가능한 artifact 링크로 남긴다.
- 현재 상태 갱신은 로드맵에만, 실측 상세는 evidence에만 남긴다. 복제한 상태표를 여러 README에 늘리지 않는다.

내부 HF 빌드·실행 및 양쪽 어댑터 수용: [HF 통합 안내](hf-integration.md).

Release A: [단계별 시험 계획](../tests/plans/release-a-20260914.md) · [A-RED/trace 감사](../tests/reports/release-a/20260914_040306.md).
Corpus와 명세 검증: [준비 결과](../tests/reports/release-a/20260914_041920.md).
Native 준비와 양쪽 어댑터: [회귀 결과](../tests/reports/release-a/20260914_043652.md).
Native 비용 관측: [CPU 실제 경로와 제거 변이](../tests/reports/release-a/20260914_050400.md).
현재 pin과 배포 후보: [token·CUDA/Metal·양쪽 어댑터·비용 귀속](../tests/reports/release-a/20260914_054100.md).
Fleet 사전 검사: [7-host 연결·native PLAN6개·모델 접근 차단](../tests/reports/release-a/20260914_062025.md).
앱 실행 환경 재확인: [모델 접근·native PLAN8/8](../tests/reports/release-a/20260914_102600.md).
후속 실기 준비: [기존 fleet 노드 회수](../tests/reports/release-a/20260915_005456.md).
현재 fleet 재개: [재빌드·양쪽 어댑터·550B arm](../tests/reports/release-a/20260915_011200.md).
동시 HF 작업 인수: [로딩 계획기 재현 계획](../layers/adapters/hf/tests/plans/loading-planner-20260915.md) · [검증](../layers/adapters/hf/tests/reports/loading-planner/20260915_013600.md).

## HF 어댑터 문서

| 문서 | 지위 |
| --- | --- |
| [layers/adapters/hf/adapter/docs/api.md](../layers/adapters/hf/adapter/docs/api.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/docs/architecture.md](../layers/adapters/hf/adapter/docs/architecture.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/docs/constraints.md](../layers/adapters/hf/adapter/docs/constraints.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/docs/internals.md](../layers/adapters/hf/adapter/docs/internals.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/docs/overview.md](../layers/adapters/hf/adapter/docs/overview.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/docs/testing.md](../layers/adapters/hf/adapter/docs/testing.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/docs/usage.md](../layers/adapters/hf/adapter/docs/usage.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/adapter/README.md](../layers/adapters/hf/adapter/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/AGENTS.md](../layers/adapters/hf/AGENTS.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/api.md](../layers/adapters/hf/docs/api.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/architecture.md](../layers/adapters/hf/docs/architecture.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/constraints.md](../layers/adapters/hf/docs/constraints.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/history/initial/api.md](../layers/adapters/hf/docs/history/initial/api.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/architecture.md](../layers/adapters/hf/docs/history/initial/architecture.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/bootstrap-evidence.md](../layers/adapters/hf/docs/history/initial/bootstrap-evidence.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/constraints.md](../layers/adapters/hf/docs/history/initial/constraints.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/decisions.md](../layers/adapters/hf/docs/history/initial/decisions.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/development-plan.md](../layers/adapters/hf/docs/history/initial/development-plan.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/HANDOFF.md](../layers/adapters/hf/docs/history/initial/HANDOFF.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/internals.md](../layers/adapters/hf/docs/history/initial/internals.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/overview.md](../layers/adapters/hf/docs/history/initial/overview.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/references.md](../layers/adapters/hf/docs/history/initial/references.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/roadmap.md](../layers/adapters/hf/docs/history/initial/roadmap.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/testing.md](../layers/adapters/hf/docs/history/initial/testing.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/history/initial/usage.md](../layers/adapters/hf/docs/history/initial/usage.md) | HF 역사 기록 |
| [layers/adapters/hf/docs/integration/README.md](../layers/adapters/hf/docs/integration/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/internals.md](../layers/adapters/hf/docs/internals.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/migration/README.md](../layers/adapters/hf/docs/migration/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/models/qwen3_5_0_8b/README.md](../layers/adapters/hf/docs/models/qwen3_5_0_8b/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/overview.md](../layers/adapters/hf/docs/overview.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/quantization.md](../layers/adapters/hf/docs/quantization.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/structure/README.md](../layers/adapters/hf/docs/structure/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/testing.md](../layers/adapters/hf/docs/testing.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/transport/framing/README.md](../layers/adapters/hf/docs/transport/framing/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/docs/usage.md](../layers/adapters/hf/docs/usage.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/README.md](../layers/adapters/hf/README.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/tests/plans/framing-20260913.md](../layers/adapters/hf/tests/plans/framing-20260913.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/tests/plans/migration-20260914.md](../layers/adapters/hf/tests/plans/migration-20260914.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/tests/plans/p4-integration-20260914.md](../layers/adapters/hf/tests/plans/p4-integration-20260914.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/tests/plans/qwen3_5_0_8b-20260913.md](../layers/adapters/hf/tests/plans/qwen3_5_0_8b-20260913.md) | HF 구성·계약·검증 |
| [layers/adapters/hf/tests/reports/framing/20260913_174516.md](../layers/adapters/hf/tests/reports/framing/20260913_174516.md) | HF 역사 기록 |
| [layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md](../layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md) | HF 역사 기록 |
| [layers/adapters/hf/tests/reports/qwen3_5_0_8b/20260913_220709.md](../layers/adapters/hf/tests/reports/qwen3_5_0_8b/20260913_220709.md) | HF 역사 기록 |
| [layers/adapters/hf/tests/reports/migration/20260914_120000.md](../layers/adapters/hf/tests/reports/migration/20260914_120000.md) | HF 내부 통합 검증·원본 정리 결과 |

연결 회수 WIP: [검증 계획](../tests/plans/release-a-transport-20260915.md) · [실패3회 중단 보고](../tests/reports/release-a/20260915_044735.md).

접수 에이전트 경유: [검증 계획](../tests/plans/ingress-envelope-20260915.md) · [엔벨롭·양쪽 어댑터 검증](../tests/reports/release-a/20260915_113754.md).

의뢰 반환 문맥: [검증 계획](../tests/plans/return-context-20260915.md).
반환 문맥 구현 검증: [공통 계약·두 어댑터·독립 변이](../tests/reports/release-a/20260915_121000.md).

클러스터 반환 경로: [시험 계획](../tests/plans/cluster-envelope-20260915.md) · [9대·MI250 SSH 검증](../tests/reports/release-a/20260915_124433.md).

Release A 대상 변경: [Qwen122B 명세·corpus 준비](../tests/reports/release-a/20260915_131432.md).

Release A FINISH 재개: [결정론적 검토·전체 회귀·양쪽 어댑터](../tests/reports/release-a/20260915_142237.md).

Release A 전송 정산: [불명 결과·hop receipt·재연결 시험 계획](../tests/plans/release-a-transport-reconciliation-20260915.md).
Release A A-BYTES: [native result·completion·receipt·edge 정수 byte 시험 계획](../tests/plans/release-a-bytes-20260915.md).

Release A A-BYTES B0: [Qwen122B physical result 상한·실제 LOAD·제거 변이](../tests/reports/release-a/20260915_211000.md).

Release A A-BYTES B1: [versioned LOAD profile·실제 잔여 용량·Qwen122B LOAD/회수](../tests/reports/release-a/20260915_222346.md).

Release A A-BYTES B2: [native 전 completion group 예약·거부 무효과·독립 변이](../tests/reports/release-a/20260915_232600.md).

Release A A-BYTES B3/B4: [보존 수명 분리·실제 경계·독립 변이·로컬 전원 사고](../tests/reports/release-a/20260915_235900.md).

Release A A-BYTES B5: [양쪽 어댑터 회귀·Qwen122B 3-host 정상 요청·회수·원격 사전검사](../tests/reports/release-a/20260916_013500.md).

노드 LOAD·UNLOAD 수명 M0: [현재 호출 경로·NL01–NL14 소유권 매핑](../tests/reports/node-load-lifecycle/20260916_014500.md).
M1: [결정론적 실행계획](../tests/plans/node-load-lifecycle-m1-20260916.md).
Typed adapter completion: [원격 baseline·독립 변이 보고](../tests/reports/node-load-lifecycle/20260916_015702.md).
M1 supervisor 완료: [실제 TCP LOAD·UNLOAD·회수 보존·workspace off/on·제거 변이](../tests/reports/node-load-lifecycle/20260916_023059.md).
M2: [결정론적 실행계획](../tests/plans/node-load-lifecycle-m2-20260916.md), [llama.cpp/HF 실제 worker·포화·cleanup·독립 변이 보고](../tests/reports/node-load-lifecycle/20260916_032621.md).
M3: [Rust/HF OUTER 이관·legacy CREATE/DELETE 제거 결정론적 실행계획](../tests/plans/node-load-lifecycle-m3-20260916.md), [실제 Agent/HF child·workspace·독립 변이 검증 보고](../tests/reports/node-load-lifecycle/20260916_041848.md).
M4: [실제 llama.cpp/HF 생성·취소·해제·재적재 결정론적 실행계획](../tests/plans/node-load-lifecycle-m4-20260916.md), [두 adapter 실제 모델·전체 회귀·독립 변이 수용 보고](../tests/reports/node-load-lifecycle/20260916_064306.md).

Release A Qwen122B H0 v3: [봉인 benchmark-spec](../test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v3.json) · [H1 latency 판정 보강·2차 INVALID·정상 회수 보고](../tests/reports/release-a/20260916_104300.md). [H0 v2 spec](../test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v2.json)과 [보고](../tests/reports/release-a/20260916_093739.md), [H0 v1 spec](../test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v1.json)과 [보고](../tests/reports/release-a/20260916_072100.md)는 역사 증거다.

Release A Qwen122B H1 1차: [64건 동시 명세 RED·실패 recovery·closed-loop 교정 구현](../tests/reports/release-a/20260916_084650.md).

Release A Qwen122B H1 2차: [판정기 SLO 누락·terminal artifact 부재 INVALID, 정상 UNLOAD·작업 자원 회수](../tests/reports/release-a/20260916_104300.md).

현재 Release A 실행 순서: [I0–I4 무결성 우선 시험계획](../tests/plans/release-a-integrity-first-20260916.md), [실행 계약](../test/benchmarks/cluster-inference/release-a/integrity-test-spec-qwen122b-i0-v1.json), [계약 검사기](../test/benchmarks/cluster-inference/release-a/validate-integrity-test-spec.py), [결과 판정기](../test/benchmarks/cluster-inference/release-a/judge-integrity.py). 무결성 GREEN 뒤에만 P0–P3 성능 개선으로 이동한다.

무결성 우선 계약 보고: [14개 실기 arm과 계약·결과 판정 변이](../tests/reports/release-a/20260916_131446.md).

Release A 전송 정산 수용: [R1–R9·물리 receipt 복구·최종 양쪽 어댑터](../tests/reports/release-a/20260915_183158.md).

Release A Qwen122B A-PLAN: [3물리 host native PLAN·공유 pool·배포 전 거부](../tests/reports/release-a/20260915_190631.md).

Release A Qwen122B A-LOAD: [3물리 host 실제 allocation·회수](../tests/reports/release-a/20260915_195106.md).

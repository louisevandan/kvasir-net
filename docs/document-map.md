# 전체 문서 안내도와 권위

2026-09-06 정리. 이 파일은 저장소 소유 Markdown 전체의 **분류·읽기 경로·계약 소유**를 관리한다.
이관은 기존 기록을 지우는 것이 아니다. 아래 상태 표시는 모든 과거 본문의 코드 주장을 새로 인증했다는 뜻도 아니다.
현재 코드 감사 범위와 열린 결함은 로드맵과 해당 날짜 evidence에 명시한다.

## 1. 새 세션에서 읽을 것

1. [AGENTS.md](../AGENTS.md): 저장소 작업·검증·보고 규칙.
2. [분산 배치 로드맵의 현재 상태](distributed-batching-roadmap.md#current-status): 검증된 진전/미검증 작업/중단과 재개 조건. 시간순 기록의 옛 “다음”부터 실행하지 않는다.
3. [분산 배치 검증 규약](distributed-batching-verification.md): 결정론적 반례와 실기 승인 조건.
4. [계층 격리 계약](layer-isolation-contract.md): P4/어댑터/native/llama/backend 책임과 upstream 충격 흡수.
5. 아래 분야 소유 문서와 실제 구현 경로: 지금 고치는 계약만 필요한 만큼 읽는다.

초대형 모델·다중 물리 컴퓨터·강한 요청 웨이브·정상 응답 전문이 최종 성과의 조건이다.
작은 모델과 한 머신 실측, simulator 완주, 과거 U/P 번호를 새로운 완료 기준으로 끌어올리지 않는다.

## 2. 계약 소유와 충돌 해결

| 주제 | 단독 소유 / 사용 규칙 |
| --- | --- |
| 현재 목표·상태·개발 순서·단계 승격 | [로드맵](distributed-batching-roadmap.md). 다른 문서는 순서를 복제하지 않고 링크한다. |
| 시험 입력·mutation·판정·실기 웨이브·보고 지표 | [검증 규약](distributed-batching-verification.md). “실행 명령 있음”과 “현재 통과”를 구분한다. |
| 층별 역할·허용 의존·public 타입·업데이트 적응 경계 | [계층 격리 계약](layer-isolation-contract.md). 기존 include 부채 수치를 전체 구조 격리로 읽지 않는다. |
| 문서의 지위·찾는 경로 | 이 문서. 새 문서를 만들면 아래 전체 목록도 함께 등록한다. |
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
| [docs/implementation.md](implementation.md) | 경로별 참고·재감사 필요 |
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

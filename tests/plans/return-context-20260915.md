# 의뢰 반환 문맥 공통 계약 검증 계획

2026-09-15. 기준 `434bd97fc`; 사용자 지시로 엔벨롭과 앞선 분석의 일치성을 구현한다.
[공개 계약](../../docs/event-protocol-v2.md#required-request-return-context).
기존 FINISH 중단과 전체 로드맵 미완료는 이 범위와 구분한다.

## 계약과 절차

1. OUTER는 전체 배치를 알며 의뢰별 접수 에이전트를 지정한다. source/target/return_route를 분리한다.
2. 모든 유효 이벤트에 반환 경로를 요구한다. OUTER source/target과 충돌하는 경로 및 P4E3 absent flag를 거부한다.
3. 실제 retained broker 거부에서 원본 pointer·보유 비용과 큐/예약/receipt 무변화를 확인한다. 같은 ID의 정상 입력은 이후 수용한다.
4. 공통 ReturnContext를 양쪽 어댑터가 사용한다. llama.cpp의 혼합 결과는 owner별 경로·correlation·deadline을 선택하고 carrier causation을 유지한다.
5. 실제 retained node/llama worker 두 단계와 TCP 접수 A→worker B→A→OUTER 경로를 검사한다. HF는 실제 bridge/IPC와 OUTER reader도 검증한다.
6. 독립 worktree에서 공통 검증·next 계승·owner 선택을 각각 제거한다. 별도 build 디렉터리의 실제 재컴파일·바이너리 hash와 실패를 기록한다.
7. `cargo test --workspace --features hf-transformers --no-fail-fast` 종료 후 전체 summary를 집계한다. 알려진 FINISH 실패를 숨기지 않는다.
8. 기존 llama.cpp와 HF Qwen0.8B 실제 생성·회수 및 최종 A/B nodes=[]를 검사한다. 소유 시험 프로세스만 종료한다.

## 범위와 증거

Windows PowerShell, 로컬 두 agent; OUTER는 A에만 연결하고 노드는 B에 둔다. 다중 물리 컴퓨터/VPC·대형 모델 성능 수용은 아니다.
원자료 `target/return-context-20260915/`: 소스 봉인, 명령/로그/exit, 변이, 실제 모델 결과와 정리 증거.
정상 fixture는 명시적 반환 문맥으로 갱신한다. 기존 malformed 입력은 삭제하지 않고 더 이른 wire 거부와 직접 consumer 검사를 함께 보존한다.

# 접수 에이전트 엔벨롭·반환 경로 검증 계획

2026-09-15 생성. 기준 P4 `4f2db5cda`. 사용자의 접수 에이전트 경유 요구에 한정한 후속 수정이다.
FINISH 실패3회 중단을 전체 로드맵 재개로 해석하지 않는다.
[계약](../../docs/event-protocol-v2.md#reception-agent-and-an-outer-reachable-only-through-a-gateway).

## 환경·목표

Windows PowerShell, 실제 로컬 TCP 에이전트 두 개, 현재 HF feature 및 llama.cpp/HF Qwen0.8B.
OUTER는 A에만 접속하고 모델 노드는 B에 생성한다. 실제 VPC/다중 컴퓨터 배포 수용은 아니다.
네트워크 반환 대상은 접수 A이고 OUTER 채널·세대는 A의 로컬 배달 정보다. payload·identity를 보존한다.

## 절차·기대값

1. 실제 Runtime A→B control→A→OUTER 왕복 후 A의 OUTER binding1/B0을 검사한다. 수정 전 B1 반례 보존.
2. B의 OUTER 출력은 A로 outbound, A만 local OUTER mailbox로 전달한다. Full이면 원본 pointer·비용·receipt를 보존한다.
3. 독립 worktree와 별도 빈 build 경로에서 registration guard 제거와 OUTER 조기 local 전달 변이를 각각 검출한다.
4. `cargo test --workspace --features hf-transformers --no-fail-fast`의 모든 summary·최종 exit를 집계한다.
5. OUTER→A→B 실제 llama.cpp/HF 생성·EOS·해제·UNLOAD/DELETE와 최종 두 agent의 빈 node를 확인한다.
6. 소유 시험 agent만 종료하고 source/binary hash·명령·log·exit와 미검증 범위를 기록한다.

원자료 `target/ingress-envelope-20260915/`. 모델·기존 설치 앱·Studio 코드는 변경하지 않는다.
기존 FINISH RED는 별도 보존하며 기대값을 완화하지 않는다. 이 범위의 구현 검증 실패가3회이면 중단한다.
